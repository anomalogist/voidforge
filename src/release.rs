use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::models::{OutputPackage, Workspace};

pub fn output_dir(workspace: &Workspace) -> PathBuf {
    workspace.path.join("hostdir").join("binpkgs")
}

fn parse_filename(name: &str) -> (String, String) {
    let stem = name.strip_suffix(".xbps").unwrap_or(name);
    let without_arch = stem.rsplit_once('.').map(|(left, _)| left).unwrap_or(stem);
    let (name_version, revision) = match without_arch.rsplit_once('_') {
        Some((left, revision)) => (left, revision),
        None => (without_arch, ""),
    };
    let package = name_version
        .rsplit_once('-')
        .map(|(left, _)| left)
        .unwrap_or(name_version);
    let version_core = name_version
        .strip_prefix(&format!("{package}-"))
        .unwrap_or(name_version);
    let version = if revision.is_empty() {
        version_core.to_string()
    } else {
        format!("{version_core}_{revision}")
    };
    (package.to_string(), version)
}

pub fn list(workspace: &Workspace) -> Result<Vec<OutputPackage>> {
    let dir = output_dir(workspace);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut packages = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("xbps") {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().into_owned();
        let (name, version) = parse_filename(&filename);
        packages.push(OutputPackage {
            name,
            path: path.clone(),
            size: fs::metadata(path)?.len(),
            version,
        });
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packages)
}

pub fn index(workspace: &Workspace) -> Result<String> {
    let dir = output_dir(workspace);
    if !dir.is_dir() {
        return Err(crate::error::invalid("no binpkgs directory"));
    }
    let output = Command::new("xbps-rindex").arg("-a").arg(&dir).output()?;
    if !output.status.success() {
        return Err(Error::Command {
            cmd: format!("xbps-rindex -a {}", dir.display()),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            code: output.status.code(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn metadata(dir: &Path, name: &str) -> Result<String> {
    let repo = format!("--repository={}", dir.display());
    let output = Command::new("xbps-query")
        .arg(&repo)
        .args(["-p", "pkgver,size,arch,license,shortdesc", name])
        .output()?;
    if !output.status.success() {
        return Err(Error::Command {
            cmd: format!("xbps-query {repo} {name}"),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            code: output.status.code(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_package_filename() {
        let (name, version) = parse_filename("foo-1.2.3_1.x86_64.xbps");
        assert_eq!(name, "foo");
        assert_eq!(version, "1.2.3_1");
    }
}
