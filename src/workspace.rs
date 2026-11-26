use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::db;
use crate::error::{invalid, not_found, Result};
use crate::git;
use crate::models::{Finding, Workspace};
use crate::utils::path::expand_home;

pub fn validate(path: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let check = |exists: bool, title: &str, detail: &str, evidence: String| {
        if exists {
            Finding::info(title, detail)
        } else {
            Finding::error(title, detail).with_evidence(evidence)
        }
    };
    findings.push(check(
        path.is_dir(),
        "directory exists",
        "The selected path must be a directory",
        path.display().to_string(),
    ));
    findings.push(check(
        path.join(".git").exists(),
        "Git repository",
        "A void-packages tree must be a Git repository",
        path.join(".git").display().to_string(),
    ));
    findings.push(check(
        path.join("srcpkgs").is_dir(),
        "srcpkgs directory",
        "Package templates live under srcpkgs",
        path.join("srcpkgs").display().to_string(),
    ));
    findings.push(check(
        path.join("xbps-src").is_file(),
        "xbps-src script",
        "The official build script should be present",
        path.join("xbps-src").display().to_string(),
    ));
    findings.push(check(
        path.join("common").is_dir(),
        "common directory",
        "Void build helpers should be present",
        path.join("common").display().to_string(),
    ));
    if path.is_dir() && !git::is_repository(path) {
        findings.push(
            Finding::error(
                "Git metadata is unreadable",
                "Run `git status` in the directory",
            )
            .with_evidence(path.display().to_string()),
        );
    }
    findings
}

pub fn is_valid(path: &Path) -> bool {
    validate(path)
        .iter()
        .all(|finding| finding.severity != crate::models::Severity::Error)
}

pub fn add(conn: &Connection, input: &str) -> Result<Workspace> {
    let path = expand_home(input)?;
    if !path.is_dir() {
        return Err(not_found(format!(
            "directory does not exist: {}",
            path.display()
        )));
    }
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM workspaces WHERE path = ?1",
            params![path.to_string_lossy()],
            |row| row.get(0),
        )
        .optional()?;
    if existing.is_some() {
        return Err(invalid("workspace is already registered"));
    }
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "void-packages".into());
    let branch = git::current_branch(&path).unwrap_or_default();
    let remotes = git::remotes(&path).unwrap_or_default();
    let valid = is_valid(&path);
    let id = db::insert_workspace(
        conn,
        &name,
        &path.to_string_lossy(),
        &branch,
        &remotes,
        valid,
    )?;
    db::record_activity(conn, "workspace", &format!("added {name}"))?;
    Ok(Workspace {
        id,
        name,
        path,
        branch,
        remotes,
        valid,
        created_at: chrono::Local::now().to_rfc3339(),
    })
}

pub fn clone(conn: &Connection, url: &str, name: Option<&str>) -> Result<Workspace> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| invalid("HOME is not set"))?;
    let name = name
        .filter(|value| !value.is_empty())
        .unwrap_or("void-packages");
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(invalid("workspace name contains unsupported characters"));
    }
    let destination = home.join(name);
    if destination.exists() {
        return Err(invalid(format!(
            "destination already exists: {}",
            destination.display()
        )));
    }
    if let Err(error) = git::clone(url, &destination) {
        let _ = std::fs::remove_dir_all(&destination);
        return Err(error);
    }
    match add(conn, &destination.to_string_lossy()) {
        Ok(workspace) => Ok(workspace),
        Err(error) => {
            // Do not leave an unregistered half-cloned tree behind: it would
            // make a corrected retry fail with "destination already exists".
            let _ = std::fs::remove_dir_all(&destination);
            Err(error)
        }
    }
}

pub fn refresh(conn: &Connection, workspace: &Workspace) -> Result<Workspace> {
    let branch = git::current_branch(&workspace.path).unwrap_or_default();
    let remotes = git::remotes(&workspace.path).unwrap_or_default();
    let valid = is_valid(&workspace.path);
    db::update_workspace(conn, workspace.id, &branch, &remotes, valid)?;
    Ok(Workspace {
        branch,
        remotes,
        valid,
        ..workspace.clone()
    })
}

pub fn remove(conn: &Connection, id: i64) -> Result<()> {
    db::delete_workspace(conn, id)
}
