use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use sysinfo::{Disks, System};

use crate::models::{DiskInfo, HostInfo, SystemMetrics, ToolStatus};

const TOOLS: [&str; 10] = [
    "xbps-src",
    "xbps",
    "xbps-query",
    "xbps-rindex",
    "git",
    "patch",
    "curl",
    "tar",
    "bash",
    "ninja",
];

pub fn parse_os_release(contents: &str) -> HashMap<String, String> {
    contents
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            ))
        })
        .collect()
}

fn detect_libc() -> String {
    for candidate in [
        "/lib/ld-musl-x86_64.so.1",
        "/lib/ld-linux-x86-64.so.2",
        "/lib64/ld-linux-x86-64.so.2",
    ] {
        if PathBuf::from(candidate).exists() {
            return if candidate.contains("musl") {
                "musl".into()
            } else {
                "glibc".into()
            };
        }
    }
    "unknown".into()
}

pub fn detect_host() -> HostInfo {
    let release = std::fs::read_to_string("/etc/os-release")
        .map(|text| parse_os_release(&text))
        .unwrap_or_default();
    let arch = std::env::consts::ARCH.to_string();
    let libc = detect_libc();
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_default()
        .trim()
        .to_string();
    let is_void = release.get("ID").is_some_and(|id| id == "void");
    let mut reasons = Vec::new();
    if !is_void {
        reasons.push("host is not Void Linux".to_string());
    }
    if arch != "x86_64" {
        reasons.push(format!("architecture {arch} is experimental"));
    }
    if libc != "glibc" {
        reasons.push(format!("{libc} support is experimental"));
    }
    HostInfo {
        os_pretty: release
            .get("PRETTY_NAME")
            .cloned()
            .unwrap_or_else(|| "Unknown".into()),
        os_id: release.get("ID").cloned().unwrap_or_default(),
        os_version: release.get("VERSION_ID").cloned().unwrap_or_default(),
        kernel,
        arch,
        libc,
        supported: reasons.is_empty(),
        reason: if reasons.is_empty() {
            None
        } else {
            Some(reasons.join("; "))
        },
    }
}

#[derive(Clone)]
pub enum ToolScope {
    Path,
    Workspace(PathBuf),
}

pub fn detect_tools(scopes: &[ToolScope]) -> Vec<ToolStatus> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    for scope in scopes {
        match scope {
            ToolScope::Path => {}
            ToolScope::Workspace(path) => dirs.push(path.clone()),
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("void-packages"));
    }

    TOOLS
        .iter()
        .map(|name| {
            let found = dirs.iter().map(|dir| dir.join(name)).find(|path| {
                std::fs::metadata(path)
                    .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
            });
            match found {
                Some(path) => ToolStatus {
                    name: (*name).to_string(),
                    found: true,
                    version: version_of(&path),
                    path: Some(path),
                },
                None => ToolStatus {
                    name: (*name).to_string(),
                    found: false,
                    version: None,
                    path: None,
                },
            }
        })
        .collect()
}

fn version_of(path: &PathBuf) -> Option<String> {
    let output = Command::new(path).arg("--version").output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(90).collect())
}

pub fn sample_metrics(system: &mut System, disks: &mut Disks) -> SystemMetrics {
    system.refresh_cpu_all();
    system.refresh_memory();
    disks.refresh(false);
    SystemMetrics {
        cpu_percent: system.global_cpu_usage(),
        memory_total: system.total_memory(),
        memory_used: system.used_memory(),
        swap_total: system.total_swap(),
        swap_used: system.used_swap(),
        disks: disks
            .iter()
            .filter(|disk| !disk.name().to_string_lossy().starts_with("loop"))
            .map(|disk| DiskInfo {
                name: disk.name().to_string_lossy().into_owned(),
                mount_point: disk.mount_point().to_string_lossy().into_owned(),
                total_bytes: disk.total_space(),
                available_bytes: disk.available_space(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release() {
        let values = parse_os_release("NAME=\"Void Linux\"\nID=void\n");
        assert_eq!(values.get("NAME").unwrap(), "Void Linux");
        assert_eq!(values.get("ID").unwrap(), "void");
    }

    #[test]
    fn host_is_detected() {
        // Runs on Void and on generic CI runners alike: detection must always
        // succeed, and the `supported` flag must agree with the detected ID.
        let host = detect_host();
        assert!(!host.os_id.is_empty());
        assert_eq!(host.supported, host.os_id == "void", "{:?}", host.reason);
        if host.supported {
            assert!(host.reason.is_none());
        }
    }
}
