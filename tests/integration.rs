//! End-to-end tests against a fixture `void-packages` tree.
//!
//! These tests exercise the real library code paths (workspace validation,
//! template scanning, dependency analysis, patches, git metadata, release
//! listing and build-process management). A stub `xbps-src` script stands in
//! for the official tool so the suite stays fast, offline and portable.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use voidforge_lib::events::UiEvent;
use voidforge_lib::models::{BuildOp, Severity, Workspace};
use voidforge_lib::{build, deps, git, package, patches, release, template, workspace};

const CHECKSUM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn template(pkgname: &str, depends: &str) -> String {
    format!(
        "pkgname={pkgname}\n\
         version=1.0\n\
         revision=1\n\
         short_desc=Test package {pkgname}\n\
         maintainer=Test <test@example.com>\n\
         license=MIT\n\
         homepage=https://example.com\n\
         distfiles={pkgname}-1.0.tar.gz\n\
         checksum={CHECKSUM}\n\
         build_style=configure\n\
         depends=\"{depends}\"\n"
    )
}

struct Fixture {
    root: PathBuf,
    ws: Workspace,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("vf-integration-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let path = root.join("void-packages");
        for dir in [
            "srcpkgs/hello",
            "srcpkgs/dep",
            "srcpkgs/a",
            "srcpkgs/b",
            "common",
            "hostdir/binpkgs",
        ] {
            fs::create_dir_all(path.join(dir)).unwrap();
        }
        fs::write(
            path.join("srcpkgs/hello/template"),
            template("hello", "dep"),
        )
        .unwrap();
        fs::write(path.join("srcpkgs/dep/template"), template("dep", "")).unwrap();
        fs::write(path.join("srcpkgs/a/template"), template("a", "b")).unwrap();
        fs::write(path.join("srcpkgs/b/template"), template("b", "a")).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = path.join("xbps-src");
            fs::write(
                &script,
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > xbps-args.txt\nprintf 'stub: running %s\\n' \"$*\"\n",
            )
            .unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(not(unix))]
        fs::write(path.join("xbps-src"), "@echo off\r\n").unwrap();

        assert!(Command::new("git")
            .args(["init", "-q"])
            .current_dir(&path)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["add", "-A"])
            .current_dir(&path)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                "initial"
            ])
            .current_dir(&path)
            .status()
            .unwrap()
            .success());

        let ws = Workspace {
            id: 1,
            name: "fixture".into(),
            path,
            branch: String::new(),
            remotes: Vec::new(),
            valid: true,
            created_at: String::new(),
        };
        Self { root, ws }
    }

    fn path(&self) -> &Path {
        &self.ws.path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn collect_finished(rx: &Receiver<UiEvent>, seconds: u64) -> Vec<UiEvent> {
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut events = Vec::new();
    while Instant::now() < end {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(event) => {
                let finished = matches!(event, UiEvent::BuildFinished { .. });
                events.push(event);
                if finished {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    events
}

#[test]
fn workspace_registration_and_validation() {
    let fixture = Fixture::new("workspace");
    let findings = workspace::validate(fixture.path());
    assert!(
        findings.iter().all(|item| item.severity != Severity::Error),
        "fixture should be valid: {findings:?}"
    );
    assert!(workspace::is_valid(fixture.path()));

    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../migrations/001_init.sql"))
        .unwrap();
    let registered = workspace::add(&conn, &fixture.path().to_string_lossy()).unwrap();
    assert_eq!(registered.name, "void-packages");
    assert!(registered.valid);
    let listed = voidforge_lib::db::workspaces(&conn).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].path, fixture.path());
}

#[test]
fn template_scan_validation_and_dependencies() {
    let fixture = Fixture::new("templates");
    let packages = package::scan(&fixture.ws).unwrap();
    assert_eq!(packages.len(), 4);
    let hello = packages.iter().find(|item| item.name == "hello").unwrap();
    assert_eq!(hello.version.as_deref(), Some("1.0"));

    let info = package::parse_template(&hello.path.join("template")).unwrap();
    let findings = template::validate(&info, &fixture.ws);
    assert!(
        findings.iter().all(|item| item.severity != Severity::Error),
        "complete template should pass: {findings:?}"
    );

    let deps = deps::for_package(&fixture.ws, &info, false).unwrap();
    assert!(deps
        .iter()
        .any(|dep| dep.name == "dep" && dep.in_repository));

    let cycles = deps::possible_cycles(&fixture.ws);
    assert!(
        cycles
            .iter()
            .any(|cycle| cycle.contains(&"a".to_string()) && cycle.contains(&"b".to_string())),
        "expected a <-> b cycle, got {cycles:?}"
    );
}

#[test]
fn patches_git_metadata_and_release_listing() {
    let fixture = Fixture::new("patches");
    let path = patches::create_draft(&fixture.ws, "hello", "fix.patch").unwrap();
    assert!(path.is_file());
    assert_eq!(patches::list(&fixture.ws, "hello").unwrap().len(), 1);

    let diff = "--- a/hello.c\n+++ b/hello.c\n@@ -1 +1 @@\n-old\n+new\n";
    assert!(patches::validate(diff)
        .iter()
        .all(|item| item.severity != Severity::Error));
    assert!(patches::validate("not a patch")
        .iter()
        .any(|item| item.severity == Severity::Error));

    let snapshot = git::snapshot(fixture.path()).unwrap();
    assert!(!snapshot.branch.is_empty());
    // `git status --short` reports an untracked directory as one entry, so
    // accept either the directory or the file name here.
    assert!(
        snapshot
            .changes
            .iter()
            .any(|change| change.path.contains("patches") || change.path.contains("fix.patch")),
        "expected the new patch in git snapshot: {:?}",
        snapshot.changes
    );

    let output = fixture
        .path()
        .join("hostdir/binpkgs/hello-1.0_1.x86_64.xbps");
    fs::write(&output, b"xbps").unwrap();
    let built = release::list(&fixture.ws).unwrap();
    assert_eq!(built.len(), 1);
    assert_eq!(built[0].name, "hello");
    assert_eq!(built[0].version, "1.0_1");
    assert_eq!(built[0].path, output);
}

#[cfg(unix)]
#[test]
fn build_manager_invokes_xbps_src_and_streams_logs() {
    let fixture = Fixture::new("build");
    let (tx, rx) = mpsc::channel();
    let manager = build::BuildManager::new(tx);
    let log_dir = fixture.root.join("logs");
    fs::create_dir_all(&log_dir).unwrap();

    let log = manager
        .start(7, &fixture.ws, "hello", BuildOp::Fetch, 2, &log_dir)
        .unwrap();
    let events = collect_finished(&rx, 10);
    let finished = events
        .iter()
        .find_map(|event| match event {
            UiEvent::BuildFinished { status, code, .. } => Some((status.as_str(), *code)),
            _ => None,
        })
        .expect("build should finish");
    assert_eq!(finished, ("success", Some(0)));
    assert!(events.iter().any(|event| matches!(
        event,
        UiEvent::BuildLog { line, .. } if line.contains("stub: running")
    )));

    let log_text = fs::read_to_string(&log).unwrap();
    assert!(log_text.contains("stub: running -j 2 fetch hello"));
    let args = fs::read_to_string(fixture.path().join("xbps-args.txt")).unwrap();
    assert_eq!(args, "-j\n2\nfetch\nhello\n");
    assert_eq!(manager.active(), 0);
}

#[cfg(unix)]
#[test]
fn build_manager_cancels_a_running_build() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new("cancel");
    fs::write(
        fixture.path().join("xbps-src"),
        "#!/bin/sh\necho 'started'\nsleep 30\n",
    )
    .unwrap();
    fs::set_permissions(
        fixture.path().join("xbps-src"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let (tx, rx) = mpsc::channel();
    let manager = build::BuildManager::new(tx);
    let log_dir = fixture.root.join("logs");
    fs::create_dir_all(&log_dir).unwrap();
    manager
        .start(9, &fixture.ws, "hello", BuildOp::Build, 1, &log_dir)
        .unwrap();
    assert_eq!(manager.active(), 1);

    assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
    manager.cancel(9).unwrap();
    let events = collect_finished(&rx, 10);
    assert!(
        events.iter().any(|event| matches!(
            event,
            UiEvent::BuildFinished { status, .. } if status == "failed"
        )),
        "cancelled build should report failure"
    );
    assert_eq!(manager.active(), 0);
}
