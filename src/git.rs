use std::path::Path;
use std::process::Command;

use crate::error::{invalid, Error, Result};
use crate::models::{GitChange, GitSnapshot};

/// Keep network operations bounded and never block on an interactive password
/// prompt while the TUI is running.
fn git_command() -> Command {
    let mut command = Command::new("git");
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_HTTP_LOW_SPEED_LIMIT", "1024")
        .env("GIT_HTTP_LOW_SPEED_TIME", "30");
    command
}

pub fn run(args: &[&str], dir: &Path) -> Result<String> {
    let output = git_command().arg("-C").arg(dir).args(args).output()?;
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

/// Like `run`, but keeps NUL separators and trailing spaces from file names.
fn run_raw(args: &[&str], dir: &Path) -> Result<String> {
    let output = git_command().arg("-C").arg(dir).args(args).output()?;
    if !output.status.success() {
        return Err(Error::Command {
            cmd: format!("git {}", args.join(" ")),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            code: output.status.code(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Refuse option-like values before handing them to git. `--` already stops
/// option parsing for remote/refspec arguments; this check gives a clear error
/// and protects commands that cannot take a separator.
fn check_argument(label: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_whitespace) {
        return Err(invalid(format!("invalid {label}: {value:?}")));
    }
    Ok(())
}

fn check_branch(branch: &str) -> Result<()> {
    check_argument("branch name", branch)?;
    let dir = std::env::current_dir()?;
    // `check-ref-format` does not need a repository, but run() expects one.
    if run(&["check-ref-format", "--branch", branch], &dir).is_err() {
        return Err(invalid(format!("invalid branch name: {branch:?}")));
    }
    Ok(())
}

fn non_empty_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn is_repository(dir: &Path) -> bool {
    git_command()
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn current_branch(dir: &Path) -> Result<String> {
    let branch = run(&["branch", "--show-current"], dir)?;
    Ok(if branch.is_empty() {
        "detached".into()
    } else {
        branch
    })
}

pub fn remotes(dir: &Path) -> Result<Vec<String>> {
    let text = run(&["remote"], dir)?;
    Ok(non_empty_lines(&text))
}

pub fn branches(dir: &Path) -> Result<Vec<String>> {
    let text = run(&["branch", "--format=%(refname:short)"], dir)?;
    Ok(non_empty_lines(&text))
}

/// Parse `git status --porcelain=v1 -z`. Renames are encoded as two NUL-
/// separated records; the second one holds the new path.
fn parse_status(raw: &str) -> Vec<GitChange> {
    let mut fields = raw.split('\0');
    let mut changes = Vec::new();
    while let Some(record) = fields.next() {
        if record.len() < 4 {
            continue;
        }
        let status = record[..2].trim().to_string();
        let path = record[3..].to_string();
        if (status.starts_with('R') || status.starts_with('C')) && path.contains(" -> ") {
            // Non `-z` porcelain emits `old -> new`; we keep the new path.
            let new = path.split(" -> ").nth(1).unwrap_or(&path).to_string();
            changes.push(GitChange { status, path: new });
            continue;
        }
        if status.starts_with('R') || status.starts_with('C') {
            if let Some(new) = fields.next() {
                if !new.is_empty() {
                    changes.push(GitChange {
                        status,
                        path: new.to_string(),
                    });
                    continue;
                }
            }
        }
        changes.push(GitChange { status, path });
    }
    changes
}

pub fn status_changes(dir: &Path) -> Result<Vec<GitChange>> {
    // Plain `--short` reports untracked directories as one entry (e.g.
    // `?? srcpkgs/hello/patches/`), hiding the actual new files. Asking for
    // `--untracked-files=all` globally walks masterdir/ and hostdir/ and used
    // to freeze the UI for tens of seconds. Expand only small untracked
    // directories and skip the generated build trees.
    let changes = parse_status(&run_raw(&["status", "--porcelain=v1", "-z"], dir)?);
    let mut result = Vec::new();
    for change in changes {
        if change.status == "??" && change.path.ends_with('/') && !is_generated_dir(&change.path) {
            // `git status ... -- <dir>` is cheap for source directories; it
            // never descends into the generated build trees.
            if let Ok(detail) = run_raw(
                &[
                    "status",
                    "--porcelain=v1",
                    "-z",
                    "--untracked-files=all",
                    "--",
                    &change.path,
                ],
                dir,
            ) {
                let nested = parse_status(&detail);
                if !nested.is_empty() {
                    result.extend(nested);
                    continue;
                }
            }
        }
        result.push(change);
    }
    Ok(result)
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

/// Include staged changes as well as unstaged ones, so saving a patch does not
/// silently drop edits the user already staged.
pub fn diff_head(dir: &Path, path: Option<&str>) -> Result<String> {
    match path {
        Some(value) => run(&["diff", "HEAD", "--", value], dir),
        None => run(&["diff", "HEAD"], dir),
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
    let output = git_command().args(["clone", "--", url, &path]).output()?;
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
    check_branch(branch)?;
    run(&["checkout", &format!("refs/heads/{branch}")], dir)?;
    Ok(())
}

pub fn create_branch(dir: &Path, branch: &str) -> Result<()> {
    check_branch(branch)?;
    run(&["branch", &format!("refs/heads/{branch}")], dir)?;
    Ok(())
}

pub fn fetch(dir: &Path, remote: &str) -> Result<String> {
    check_argument("remote name", remote)?;
    run(&["fetch", "--", remote], dir)
}

pub fn pull(dir: &Path, remote: &str, branch: &str) -> Result<String> {
    check_argument("remote name", remote)?;
    check_argument("branch name", branch)?;
    run(&["pull", "--ff-only", "--", remote, branch], dir)
}

pub fn push(dir: &Path, remote: &str, branch: &str) -> Result<String> {
    check_argument("remote name", remote)?;
    check_argument("branch name", branch)?;
    run(&["push", "--", remote, branch], dir)
}

pub fn tags(dir: &Path) -> Result<Vec<String>> {
    Ok(non_empty_lines(&run(
        &["tag", "--list", "--sort=-creatordate"],
        dir,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nul_status_with_renames() {
        let changes = parse_status("R  old name.txt\0new name.txt\0?? srcpkgs/foo/\0");
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].path, "new name.txt");
        assert_eq!(changes[0].status, "R");
        assert_eq!(changes[1].path, "srcpkgs/foo/");
    }

    #[test]
    fn rejects_option_like_branch_names() {
        assert!(check_branch("-f").is_err());
        assert!(check_branch("feature/test").is_ok());
    }
}
