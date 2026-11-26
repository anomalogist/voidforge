use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{invalid, Error, Result};
use crate::git;
use crate::models::{Finding, TemplateInfo, Workspace};
use crate::utils::path::in_workspace;

pub fn patch_dir(workspace: &Workspace, package: &str) -> PathBuf {
    workspace.path.join("srcpkgs").join(package).join("patches")
}

/// Patch names come straight from user commands. Keep them as plain file
/// names so `:patch remove ../../important.patch` cannot escape srcpkgs.
pub fn validate_name(name: &str) -> Result<()> {
    if !name.ends_with(".patch")
        || name.starts_with('.')
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err(invalid("patch name must be a simple file ending in .patch"));
    }
    Ok(())
}

pub fn patch_path(workspace: &Workspace, package: &str, name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    let path = patch_dir(workspace, package).join(name);
    if !in_workspace(&path, &workspace.path) {
        return Err(invalid("patch path escapes the workspace"));
    }
    Ok(path)
}

pub fn list(workspace: &Workspace, package: &str) -> Result<Vec<PathBuf>> {
    let dir = patch_dir(workspace, package);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    Ok(files)
}

pub fn validate(content: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    if !content.lines().any(|line| line.starts_with("--- ")) {
        findings.push(Finding::error(
            "missing original file header",
            "A unified patch needs a `--- a/...` line",
        ));
    }
    if !content.lines().any(|line| line.starts_with("+++ ")) {
        findings.push(Finding::error(
            "missing new file header",
            "A unified patch needs a `+++ b/...` line",
        ));
    }
    if !content.lines().any(|line| line.starts_with("@@")) {
        findings.push(Finding::error(
            "missing hunk header",
            "No `@@` hunk was found in the patch",
        ));
    }
    if content.trim().is_empty() {
        findings.push(Finding::warning(
            "empty patch",
            "The patch file has no content",
        ));
    }
    findings
}

pub fn create_draft(workspace: &Workspace, package: &str, name: &str) -> Result<PathBuf> {
    let path = patch_path(workspace, package, name)?;
    let dir = path
        .parent()
        .ok_or_else(|| invalid("invalid patch directory"))?;
    fs::create_dir_all(dir)?;
    if path.exists() {
        return Err(invalid("patch already exists"));
    }
    fs::write(&path, "")?;
    Ok(path)
}

pub fn from_git_diff(
    workspace: &Workspace,
    package: &str,
    name: &str,
    file: &str,
) -> Result<PathBuf> {
    let diff = git::diff_head(&workspace.path, Some(file))?;
    if diff.trim().is_empty() {
        return Err(invalid("git diff is empty; nothing to save as a patch"));
    }
    let path = create_draft(workspace, package, name)?;
    fs::write(&path, diff)?;
    Ok(path)
}

/// Read a diff header path. Git quotes paths containing spaces or escapes,
/// for example `+++ "b/foo bar.c"\t2026-01-01`.
fn header_path(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?.trim();
    let path = if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.rfind('"')?;
        let escaped = &quoted[..end];
        let mut decoded = String::new();
        let mut chars = escaped.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('n') => decoded.push('\n'),
                    Some('t') => decoded.push('\t'),
                    Some('"') => decoded.push('"'),
                    Some('\\') => decoded.push('\\'),
                    Some(other) => {
                        decoded.push('\\');
                        decoded.push(other);
                    }
                    None => decoded.push('\\'),
                }
            } else {
                decoded.push(ch);
            }
        }
        decoded
    } else {
        rest.split('\t').next().unwrap_or(rest).trim().to_string()
    };
    let relative = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(&path);
    (!relative.is_empty() && relative != "/dev/null").then(|| relative.to_string())
}

/// `---` and `+++` paths for each hunk, with `a/` and `b/` prefixes removed.
fn affected_files(content: &str) -> Vec<(PathBuf, PathBuf)> {
    let mut original = String::new();
    let mut files = Vec::new();
    for line in content.lines() {
        if let Some(path) = header_path(line, "--- ") {
            original = path;
        } else if let Some(path) = header_path(line, "+++ ") {
            if !original.is_empty() {
                files.push((PathBuf::from(&original), PathBuf::from(&path)));
                original.clear();
            }
        }
    }
    files
}

fn safe_relative(source: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        })
    {
        return Err(invalid(format!(
            "unsafe path in patch: {}",
            relative.display()
        )));
    }
    let target = source.join(relative);
    if !in_workspace(&target, source) {
        return Err(invalid(format!(
            "patch path escapes the source tree: {}",
            relative.display()
        )));
    }
    Ok(target)
}

pub fn source_dir(workspace: &Workspace, info: &TemplateInfo) -> PathBuf {
    let pkg = info.name();
    let version = info.get("version").unwrap_or("unknown");
    workspace
        .path
        .join("masterdir")
        .join("builddir")
        .join(format!("{pkg}-{version}"))
}

fn run_patch(source: &Path, content: &str, reverse: bool) -> Result<String> {
    let mut command = Command::new("patch");
    command.arg("-p1").arg("--batch").arg("--forward");
    if reverse {
        command.arg("-R");
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(source);
    let mut child = command.spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(content.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(Error::Command {
            cmd: "patch -p1".into(),
            message: format!(
                "{} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .trim()
            .to_string(),
            code: output.status.code(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn apply(
    workspace: &Workspace,
    info: &TemplateInfo,
    patch: &Path,
    reverse: bool,
) -> Result<Vec<PathBuf>> {
    if !in_workspace(patch, &workspace.path) {
        return Err(invalid("patch path escapes the workspace"));
    }
    let content = fs::read_to_string(patch)?;
    let findings = validate(&content);
    if findings
        .iter()
        .any(|f| f.severity == crate::models::Severity::Error)
    {
        return Err(invalid("patch is not a valid unified diff"));
    }
    let source = source_dir(workspace, info);
    if !source.is_dir() {
        return Err(invalid(format!(
            "source directory not found: {}",
            source.display()
        )));
    }
    // Snapshot every file the patch can touch before starting. `patch` is not
    // atomic: a failure in a later hunk can leave earlier files modified.
    let mut backups: Vec<(PathBuf, PathBuf)> = Vec::new();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for (original, target) in affected_files(&content) {
        for relative in [&target, &original] {
            let file = safe_relative(&source, relative)?;
            if !backups.iter().any(|(_, location)| location == &file) && file.is_file() {
                let name = file
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "file".into());
                let backup =
                    file.with_file_name(format!("{name}.vf-backup-{stamp}-{}", backups.len()));
                fs::copy(&file, &backup)?;
                backups.push((backup, file));
            }
        }
    }
    if let Err(error) = run_patch(&source, &content, reverse) {
        // Restore the exact bytes we replaced and remove our own backups.
        // Reject files (`.rej`) are left in place for diagnosis.
        for (backup, location) in &backups {
            if let Err(restore_error) = fs::copy(backup, location) {
                return Err(invalid(format!(
                    "patch failed ({error}); could not restore {}: {restore_error}. Backup kept at {}",
                    location.display(),
                    backup.display()
                )));
            }
            let _ = fs::remove_file(backup);
        }
        return Err(error);
    }
    Ok(backups.into_iter().map(|(backup, _)| backup).collect())
}

pub fn remove(path: &Path) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| invalid("invalid patch path"))?;
    validate_name(name)?;
    if !path.is_file() {
        return Err(invalid("patch file does not exist"));
    }
    fs::remove_file(path)?;
    Ok(())
}

pub fn rename(path: &Path, name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    let target = path.with_file_name(name);
    if target.exists() {
        return Err(invalid("target patch already exists"));
    }
    fs::rename(path, &target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_diff_headers() {
        assert_eq!(
            header_path("+++ \"b/foo bar.c\"\t2026-01-01", "+++ "),
            Some("foo bar.c".into())
        );
        assert_eq!(
            header_path("--- a/src/main.c", "--- "),
            Some("src/main.c".into())
        );
        assert_eq!(header_path("+++ /dev/null", "+++ "), None);
    }

    #[test]
    fn rejects_unsafe_patch_names_and_paths() {
        assert!(validate_name("../evil.patch").is_err());
        assert!(validate_name("nested/evil.patch").is_err());
        assert!(validate_name("good.patch").is_ok());
        let source = Path::new("/tmp/source");
        assert!(safe_relative(source, Path::new("../outside")).is_err());
        assert!(safe_relative(source, Path::new("src/main.c")).is_ok());
    }
}
