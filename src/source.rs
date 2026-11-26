use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::error::{invalid, Error, Result};
use crate::models::TemplateInfo;

#[derive(Clone, Debug)]
pub struct Distfile {
    pub filename: String,
    pub url: String,
    pub expected: Option<String>,
}

pub fn distfiles(info: &TemplateInfo) -> Vec<Distfile> {
    let checksums: Vec<String> = info.list("checksum");
    info.list("distfiles")
        .into_iter()
        .enumerate()
        .map(|(index, token)| {
            let (filename, url) = match token.split_once('>') {
                Some((name, url)) => (name.to_string(), url.to_string()),
                None => {
                    let name = token
                        .rsplit('/')
                        .next()
                        .unwrap_or(&token)
                        .split('?')
                        .next()
                        .unwrap_or(&token)
                        .to_string();
                    (name, token)
                }
            };
            Distfile {
                filename,
                url,
                expected: checksums.get(index).map(|value| value.to_string()),
            }
        })
        .collect()
}

pub fn cache_dir(workspace: &Path, info: &TemplateInfo) -> PathBuf {
    workspace.join("hostdir").join("sources").join(format!(
        "{}-{}",
        info.name(),
        info.get("version").unwrap_or("unknown")
    ))
}

fn sha256(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    if !root.is_dir() {
        return None;
    }
    let direct = root.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, name) {
                return Some(found);
            }
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            return Some(path);
        }
    }
    None
}

/// Distfile names come from a template that may be edited by anyone. Never
/// let one escape the `hostdir/sources` cache directory.
fn safe_filename(name: &str) -> Result<&str> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(invalid(format!("unsafe distfile name: {name}")));
    }
    Ok(name)
}

pub fn download(workspace: &Path, info: &TemplateInfo, index: usize) -> Result<PathBuf> {
    let files = distfiles(info);
    let item = files
        .get(index)
        .ok_or_else(|| invalid("distfile index is out of range"))?;
    let filename = safe_filename(&item.filename)?;
    let directory = cache_dir(workspace, info);
    std::fs::create_dir_all(&directory)?;
    let destination = directory.join(filename);
    let output = Command::new("curl")
        .args(["-fL", "--retry", "2", "-o"])
        .arg(&destination)
        .arg(&item.url)
        .output()?;
    if !output.status.success() {
        return Err(Error::Command {
            cmd: format!("curl {}", item.url),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            code: output.status.code(),
        });
    }
    Ok(destination)
}

pub fn verify(workspace: &Path, info: &TemplateInfo) -> Vec<(Distfile, Option<String>, bool)> {
    let directory = cache_dir(workspace, info);
    distfiles(info)
        .into_iter()
        .map(|item| {
            let actual = find_file(&directory, &item.filename).and_then(|path| sha256(&path).ok());
            let matches = match (&item.expected, &actual) {
                (Some(expected), Some(actual)) => expected.eq_ignore_ascii_case(actual),
                _ => false,
            };
            (item, actual, matches)
        })
        .collect()
}

pub fn extract(workspace: &Path, info: &TemplateInfo, filename: &str) -> Result<PathBuf> {
    let filename = safe_filename(filename)?;
    let directory = cache_dir(workspace, info);
    let archive = find_file(&directory, filename)
        .ok_or_else(|| invalid(format!("archive not found: {filename}")))?;
    let target = directory.join("extracted");
    std::fs::create_dir_all(&target)?;
    let output = Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&target)
        .output()?;
    if !output.status.success() {
        return Err(Error::Command {
            cmd: format!("tar -xf {}", archive.display()),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            code: output.status.code(),
        });
    }
    Ok(target)
}

pub fn github_latest(repo_url: &str) -> Result<String> {
    let trimmed = repo_url.trim_end_matches('/');
    let prefix = "https://github.com/";
    let path = trimmed
        .strip_prefix(prefix)
        .ok_or_else(|| invalid("expected a https://github.com/owner/repo URL"))?;
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(invalid("GitHub URL must include owner and repository"));
    }
    let api = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        parts[0], parts[1]
    );
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "20", &api])
        .output()?;
    if !output.status.success() {
        return Err(Error::Command {
            cmd: format!("curl {api}"),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            code: output.status.code(),
        });
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    value
        .get("tag_name")
        .and_then(|tag| tag.as_str())
        .map(str::to_string)
        .ok_or_else(|| invalid("GitHub response has no tag_name"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn rejects_unsafe_distfile_names() {
        assert!(safe_filename("foo.tar.gz").is_ok());
        assert!(safe_filename("../escape").is_err());
        assert!(safe_filename("a/b").is_err());
        assert!(safe_filename("..").is_err());
    }

    #[test]
    fn parses_named_distfile() {
        let info = TemplateInfo {
            vars: HashMap::from([
                (
                    "distfiles".into(),
                    "foo.tar.gz>https://example.com/foo.tar.gz".into(),
                ),
                ("checksum".into(), "abc".into()),
            ]),
            ..Default::default()
        };
        let files = distfiles(&info);
        assert_eq!(files[0].filename, "foo.tar.gz");
        assert_eq!(files[0].expected.as_deref(), Some("abc"));
    }
}
