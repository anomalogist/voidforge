use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{invalid, Error, Result};
use crate::git;
use crate::models::{Finding, TemplateInfo, Workspace};

pub fn patch_dir(workspace: &Workspace, package: &str) -> PathBuf {
    workspace.path.join("srcpkgs").join(package).join("patches")
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
    if !name.ends_with(".patch") || name.contains('/') || name.contains("..") {
        return Err(invalid("patch name must be a simple file ending in .patch"));
    }
    let dir = patch_dir(workspace, package);
    fs::create_dir_all(&dir)?;
    let path = dir.join(name);
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
    let diff = git::diff(&workspace.path, Some(file))?;
    if diff.trim().is_empty() {
        return Err(invalid("git diff is empty; nothing to save as a patch"));
    }
    let path = create_draft(workspace, package, name)?;
    fs::write(&path, diff)?;
    Ok(path)
}

fn affected_files(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| line.strip_prefix("+++ b/"))
        .map(|line| line.split('\t').next().unwrap_or(line).trim().to_string())
        .filter(|line| !line.is_empty() && line != "/dev/null")
        .collect()
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
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
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
    let mut backups = Vec::new();
    for relative in affected_files(&content) {
        let file = source.join(&relative);
        if file.is_file() {
            let backup = file.with_extension(format!(
                "vf-backup-{}",
                chrono::Local::now().format("%Y%m%d%H%M%S")
            ));
            fs::copy(&file, &backup)?;
            backups.push(backup);
        }
    }
    run_patch(&source, &content, reverse)?;
    Ok(backups)
}

pub fn remove(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Err(invalid("patch file does not exist"));
    }
    fs::remove_file(path)?;
    Ok(())
}

pub fn rename(path: &Path, name: &str) -> Result<PathBuf> {
    if !name.ends_with(".patch") || name.contains('/') || name.contains("..") {
        return Err(invalid("patch name must be a simple file ending in .patch"));
    }
    let target = path.with_file_name(name);
    if target.exists() {
        return Err(invalid("target patch already exists"));
    }
    fs::rename(path, &target)?;
    Ok(target)
}
