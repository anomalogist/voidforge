use std::path::{Component, Path, PathBuf};

use crate::error::{invalid, Result};

pub fn expand_home(input: &str) -> Result<PathBuf> {
    let raw = if input == "~" {
        home_dir()?.to_string_lossy().into_owned()
    } else if let Some(rest) = input.strip_prefix("~/") {
        home_dir()?.join(rest).to_string_lossy().into_owned()
    } else {
        input.to_string()
    };
    normalize(PathBuf::from(raw))
}

pub fn normalize(path: PathBuf) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid("path must be absolute"));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push("/"),
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(invalid("parent directory traversal is not allowed"))
            }
            Component::Prefix(_) => return Err(invalid("windows paths are not supported")),
        }
    }
    Ok(normalized)
}

pub fn data_dir() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| invalid("HOME and XDG_DATA_HOME are not set"))?;
    Ok(base.join("voidforge"))
}

/// Check containment before using a path from a template or a command.
///
/// Lexical containment is not enough: a symlink inside the workspace can point
/// outside it, so resolve both sides when possible. If either side does not
/// exist yet (for example a file that is about to be created), fall back to
/// the lexical check on path components.
pub fn in_workspace(path: &Path, root: &Path) -> bool {
    let Ok(real_root) = root.canonicalize() else {
        return path.starts_with(root);
    };
    match canonicalize_existing_prefix(path) {
        Some(real_path) => real_path.starts_with(real_root),
        None => path.starts_with(root),
    }
}

/// Resolve symlinks in every existing part of `path`, even when the final
/// component does not exist yet. Otherwise a symlink pointing outside the
/// workspace would let a caller write outside it just because the target file
/// has not been created.
fn canonicalize_existing_prefix(path: &Path) -> Option<PathBuf> {
    let mut suffix = Vec::new();
    let mut current = path;
    loop {
        if let Ok(mut real) = current.canonicalize() {
            for component in suffix.iter().rev() {
                real.push(component);
            }
            return Some(real);
        }
        let name = current.file_name()?;
        suffix.push(name.to_os_string());
        current = current.parent()?;
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| invalid("HOME is not set"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_and_normalizes() {
        std::env::set_var("HOME", "/home/tester");
        assert_eq!(
            expand_home("~/void-packages").unwrap(),
            PathBuf::from("/home/tester/void-packages")
        );
    }

    #[test]
    fn rejects_traversal() {
        assert!(normalize(PathBuf::from("/home/a/../b")).is_err());
        assert!(normalize(PathBuf::from("relative")).is_err());
    }

    #[test]
    fn checks_containment() {
        let root = Path::new("/home/u/void-packages");
        assert!(in_workspace(&root.join("srcpkgs/x/template"), root));
        assert!(!in_workspace(Path::new("/etc/passwd"), root));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!("vf-path-test-{}", std::process::id()));
        let root = base.join("workspace");
        let outside = base.join("outside");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("link")).unwrap();

        assert!(in_workspace(&root.join("normal/file"), &root));
        assert!(!in_workspace(&root.join("link/file"), &root));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
