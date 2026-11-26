use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, mpsc::Sender, Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{invalid, Result};
use crate::events::UiEvent;
use crate::models::{BuildOp, Workspace};
use crate::utils::path::in_workspace;

struct RunningBuild {
    pid: i32,
    started: Instant,
}

pub struct BuildManager {
    tx: Sender<UiEvent>,
    running: Arc<Mutex<HashMap<i64, RunningBuild>>>,
}

impl BuildManager {
    pub fn new(tx: Sender<UiEvent>) -> Self {
        Self {
            tx,
            running: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn event_sender(&self) -> Sender<UiEvent> {
        self.tx.clone()
    }

    pub fn start(
        &self,
        build_id: i64,
        workspace: &Workspace,
        package: &str,
        op: BuildOp,
        jobs: usize,
        log_dir: &Path,
    ) -> Result<PathBuf> {
        if jobs == 0 {
            return Err(invalid("parallel jobs must be at least 1"));
        }
        let executable = workspace.path.join("xbps-src");
        if !executable.is_file() {
            return Err(invalid(format!(
                "xbps-src not found in {}",
                workspace.path.display()
            )));
        }
        if !in_workspace(&executable, &workspace.path) {
            return Err(invalid("xbps-src path escapes the workspace"));
        }
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
        // The numeric id guarantees two builds in the same millisecond never
        // overwrite each other's log.
        let log_path = log_dir.join(format!("{stamp}-{package}-{}-{build_id}.log", op.as_str()));
        let log_file = File::create(&log_path)?;
        let reader_file = log_file.try_clone()?;

        let mut command = Command::new(&executable);
        command
            .current_dir(&workspace.path)
            .arg("-j")
            .arg(jobs.to_string())
            .arg(op.as_str());
        if !matches!(op, BuildOp::Bootstrap | BuildOp::BinaryBootstrap) {
            command.arg(package);
        }
        command
            .env("XBPS_JOBS", jobs.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn()?;
        let pid = child.id() as i32;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| invalid("failed to capture stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| invalid("failed to capture stderr"))?;

        self.running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                build_id,
                RunningBuild {
                    pid,
                    started: Instant::now(),
                },
            );

        let shared_file = Arc::new(Mutex::new(reader_file));
        let stdout_thread = spawn_stream(stdout, shared_file.clone(), self.tx.clone(), build_id);
        let stderr_thread = spawn_stream(stderr, shared_file, self.tx.clone(), build_id);

        let tx = self.tx.clone();
        let running = self.running.clone();
        let finished_log = log_path.clone();
        std::thread::spawn(move || {
            let status = child.wait();
            // Normally the readers are done by now. Give them a brief chance to
            // flush so diagnostics see the complete log, but never let a
            // grandchild holding a pipe hostage keep a build "running" forever.
            let (flushed_tx, flushed_rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                let _ = flushed_tx.send(());
            });
            let _ = flushed_rx.recv_timeout(Duration::from_millis(750));
            let (seconds, _) = match running
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&build_id)
            {
                Some(job) => (job.started.elapsed().as_secs() as i64, job.pid),
                None => (0, pid),
            };
            let (status_text, code) = match status {
                Ok(status) if status.success() => ("success".to_string(), Some(0)),
                Ok(status) => ("failed".to_string(), status.code()),
                Err(_) => ("failed".to_string(), None),
            };
            let _ = tx.send(UiEvent::BuildFinished {
                build_id,
                status: status_text,
                code,
                duration_seconds: seconds,
                log_path: finished_log,
            });
        });

        Ok(log_path)
    }

    pub fn cancel(&self, build_id: i64) -> Result<()> {
        let running = self
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let job = running
            .get(&build_id)
            .ok_or_else(|| invalid("build is not running"))?;
        #[cfg(unix)]
        unsafe {
            if libc::kill(-job.pid, libc::SIGTERM) != 0 {
                libc::kill(job.pid, libc::SIGTERM);
            }
        }
        Ok(())
    }

    /// Signal every active build and its children. Used when quitting so no
    /// `xbps-src` process is left behind in the background.
    pub fn cancel_all(&self) {
        let running = self
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        #[cfg(unix)]
        for job in running.values() {
            unsafe {
                if libc::kill(-job.pid, libc::SIGTERM) != 0 {
                    libc::kill(job.pid, libc::SIGTERM);
                }
            }
        }
    }

    pub fn active(&self) -> usize {
        self.running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

fn spawn_stream<R: std::io::Read + Send + 'static>(
    reader: R,
    file: Arc<Mutex<File>>,
    tx: Sender<UiEvent>,
    build_id: i64,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buffered = BufReader::new(reader);
        let mut bytes = Vec::new();
        let mut failures = 0usize;
        loop {
            bytes.clear();
            match buffered.read_until(b'\n', &mut bytes) {
                Ok(0) => break,
                Ok(_) => {
                    failures = 0;
                    // xbps-src and their compilers routinely emit non-UTF-8
                    // bytes. Losing the rest of a log because of one such line
                    // would make diagnosis unreliable.
                    let line = String::from_utf8_lossy(&bytes)
                        .trim_end_matches(['\r', '\n'])
                        .to_string();
                    if let Ok(mut handle) = file.lock() {
                        let _ = writeln!(handle, "{line}");
                        let _ = handle.flush();
                    }
                    let _ = tx.send(UiEvent::BuildLog {
                        build_id,
                        line: line.clone(),
                    });
                }
                Err(_) => {
                    failures += 1;
                    if failures >= 10 {
                        break;
                    }
                }
            }
        }
    })
}
