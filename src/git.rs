use std::path::Path;
use std::process::Command;

use crate::error::{invalid, Error, Result};
use crate::models::{GitChange, GitSnapshot};

pub fn run(args: &[&str], dir: &Path) -> Result<String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    if !output.status.success() {
        return Err(Error::Command {
            cmd: format!("git {}", args.join(" ")),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            code: output.status.code(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

fn non_empty_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn is_repository(dir: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn current_branch(dir: &Path) -> Result<String> {
    run(&["branch", "--show-current"], dir)
}

pub fn remotes(dir: &Path) -> Result<Vec<String>> {
    let text = run(&["remote"], dir)?;
    Ok(non_empty_lines(&text))
}

pub fn branches(dir: &Path) -> Result<Vec<String>> {
    let text = run(&["branch", "--format=%(refname:short)"], dir)?;
    Ok(non_empty_lines(&text))
}

pub fn status_changes(dir: &Path) -> Result<Vec<GitChange>> {
    // Plain `--short` reports untracked directories as one entry (e.g.
    // `?? srcpkgs/hello/patches/`), hiding the actual new files. Asking for
    // `--untracked-files=all` globally walks masterdir/ and hostdir/ and used
    // to freeze the UI for tens of seconds. Expand only small untracked
    // directories and skip the generated build trees.
    let text = run(&["status", "--short"], dir)?;
    let mut changes = Vec::new();
    for line in text.lines().filter(|line| line.len() >= 4) {
        let status = line[..2].trim().to_string();
        let path = line[3..].to_string();
        if status == "??" && path.ends_with('/') && !is_generated_dir(&path) {
            // `git status --short --untracked-files=all -- <dir>` is cheap for
            // source directories; it never descends into the skipped trees.
            if let Ok(detail) = run(
                &["status", "--short", "--untracked-files=all", "--", &path],
                dir,
            ) {
                let nested: Vec<GitChange> = detail
                    .lines()
                    .filter(|line| line.len() >= 4)
                    .map(|line| GitChange {
                        status: line[..2].trim().to_string(),
                        path: line[3..].to_string(),
                    })
                    .collect();
                if !nested.is_empty() {
                    changes.extend(nested);
                    continue;
                }
            }
        }
        changes.push(GitChange { status, path });
    }
    Ok(changes)
}

fn is_generated_dir(path: &str) -> bool {
    let first = path.split('/').next().unwrap_or(path);
    matches!(first, "masterdir" | "hostdir" | ".git")
}

pub fn log(dir: &Path, limit: usize) -> Result<Vec<String>> {
    let count = limit.to_string();
    Ok(non_empty_lines(&run(
        &["log", "--oneline", "--decorate", "-n", &count],
        dir,
    )?))
}

pub fn diff(dir: &Path, path: Option<&str>) -> Result<String> {
    match path {
        Some(value) => run(&["diff", "--", value], dir),
        None => run(&["diff"], dir),
    }
}

pub fn snapshot(dir: &Path) -> Result<GitSnapshot> {
    Ok(GitSnapshot {
        branch: current_branch(dir).unwrap_or_else(|_| "detached".into()),
        remotes: remotes(dir).unwrap_or_default(),
        changes: status_changes(dir).unwrap_or_default(),
        log: log(dir, 40).unwrap_or_default(),
        diff: diff(dir, None).unwrap_or_default(),
    })
}

pub fn clone(url: &str, destination: &Path) -> Result<()> {
    if url.trim().is_empty() {
        return Err(invalid("clone URL is empty"));
    }
    let path = destination.to_string_lossy();
    let output = Command::new("git")
        .args(["clone", "--", url, &path])
        .output()?;
    if !output.status.success() {
        return Err(Error::Command {
            cmd: format!("git clone {url}"),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            code: output.status.code(),
        });
    }
    Ok(())
}

pub fn stage(dir: &Path, path: &str) -> Result<()> {
    run(&["add", "--", path], dir)?;
    Ok(())
}

pub fn unstage(dir: &Path, path: &str) -> Result<()> {
    run(&["restore", "--staged", "--", path], dir)?;
    Ok(())
}

pub fn commit(dir: &Path, message: &str) -> Result<String> {
    if message.trim().is_empty() {
        return Err(invalid("commit message is empty"));
    }
    run(&["commit", "-m", message], dir)
}

pub fn checkout(dir: &Path, branch: &str) -> Result<()> {
    run(&["checkout", branch], dir)?;
    Ok(())
}

pub fn create_branch(dir: &Path, branch: &str) -> Result<()> {
    run(&["branch", branch], dir)?;
    Ok(())
}

pub fn fetch(dir: &Path, remote: &str) -> Result<String> {
    run(&["fetch", remote], dir)
}

pub fn pull(dir: &Path, remote: &str, branch: &str) -> Result<String> {
    run(&["pull", "--ff-only", remote, branch], dir)
}

pub fn push(dir: &Path, remote: &str, branch: &str) -> Result<String> {
    run(&["push", remote, branch], dir)
}

pub fn tags(dir: &Path) -> Result<Vec<String>> {
    Ok(non_empty_lines(&run(
        &["tag", "--list", "--sort=-creatordate"],
        dir,
    )?))
}
