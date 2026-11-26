use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use crate::error::{invalid, not_found, Result};
use crate::models::{PackageEntry, TemplateInfo, Workspace};

fn assignment_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)=(.*)$").unwrap())
}

fn function_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([A-Za-z0-9_]+)\(\)\s*\{").unwrap())
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-_.".contains(c))
}

fn strip_comment(line: &str) -> String {
    let mut quote: Option<char> = None;
    let mut out = String::new();
    for ch in line.chars() {
        match quote {
            Some(expected) => {
                out.push(ch);
                if ch == expected {
                    quote = None;
                }
            }
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                out.push(ch);
            }
            None if ch == '#' => break,
            None => out.push(ch),
        }
    }
    out
}

fn parse_value(raw: &str) -> String {
    let text = raw.trim();
    if text.len() >= 2 {
        let bytes = text.as_bytes();
        if (bytes[0] == b'\'' && bytes[text.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[text.len() - 1] == b'"')
        {
            return text[1..text.len() - 1].to_string();
        }
        if bytes[0] == b'\'' || bytes[0] == b'"' {
            let quote = bytes[0] as char;
            if let Some(end) = text[1..].find(quote) {
                return text[1..1 + end].to_string();
            }
        }
    }
    text.split_whitespace().next().unwrap_or("").to_string()
}

fn expand_variables(vars: HashMap<String, String>) -> HashMap<String, String> {
    let mut result = vars;
    for _ in 0..4 {
        let snapshot = result.clone();
        // Build the search patterns once. Doing `format!("${key}")` inside the
        // per-value loop allocated millions of Strings when scanning a real
        // void-packages tree (15k templates) and made startup take minutes.
        let patterns: Vec<(String, String)> = snapshot
            .iter()
            .map(|(key, value)| (format!("${{{key}}}"), value.clone()))
            .chain(
                snapshot
                    .iter()
                    .map(|(key, value)| (format!("${key}"), value.clone())),
            )
            .collect();
        let mut changed = false;
        for value in result.values_mut() {
            // The vast majority of variables contain no `$` at all.
            if !value.contains('$') {
                continue;
            }
            let mut next = value.clone();
            for (pattern, replacement) in &patterns {
                next = next.replace(pattern, replacement);
            }
            if next != *value {
                *value = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    result
}

pub fn parse_template(path: &Path) -> Result<TemplateInfo> {
    let raw = fs::read_to_string(path)?;
    if raw.len() > 2_000_000 {
        return Err(invalid("template is unexpectedly large"));
    }

    let mut logical = Vec::new();
    let mut buffer = String::new();
    let mut start_line = 1usize;
    for (index, line) in raw.lines().enumerate() {
        let clean = strip_comment(line);
        let trimmed = clean.trim_end();
        if buffer.is_empty() {
            start_line = index + 1;
            buffer.push_str(trimmed);
        } else {
            buffer.push(' ');
            buffer.push_str(trimmed.trim());
        }
        if buffer.trim_end().ends_with('\\') {
            buffer.pop();
            continue;
        }
        logical.push((start_line, buffer.clone()));
        buffer.clear();
    }
    if !buffer.is_empty() {
        logical.push((start_line, buffer));
    }

    let mut vars = HashMap::new();
    let mut functions = Vec::new();
    let mut line_map: HashMap<String, usize> = HashMap::new();
    for (line_number, line) in &logical {
        let trimmed = line.trim();
        if let Some(captures) = function_regex().captures(trimmed) {
            functions.push(captures[1].to_string());
            continue;
        }
        if let Some(captures) = assignment_regex().captures(trimmed) {
            let key = captures[1].to_string();
            let value = parse_value(&captures[2]);
            line_map.entry(key.clone()).or_insert(*line_number);
            vars.insert(key, value);
        }
    }
    vars = expand_variables(vars);
    let info = TemplateInfo {
        path: path.to_path_buf(),
        raw,
        vars,
        lines: line_map,
        functions,
        line_count: logical.len(),
    };
    Ok(info)
}

/// Read one field directly from the raw text. Building the full
/// `TemplateInfo` (regexes, logical lines, variable expansion) for all
/// ~15.000 templates just to fill a list costs many seconds; this cheap
/// lookup is enough for the package browser.
pub fn summary_value(raw: &str, key: &str) -> Option<String> {
    for line in raw.lines() {
        let clean = strip_comment(line);
        if let Some((name, value)) = clean.split_once('=') {
            if name.trim() == key {
                return Some(parse_value(value));
            }
        }
    }
    None
}

pub fn scan(workspace: &Workspace) -> Result<Vec<PackageEntry>> {
    let srcpkgs = workspace.path.join("srcpkgs");
    if !srcpkgs.is_dir() {
        return Err(not_found(format!(
            "no srcpkgs in {}",
            workspace.path.display()
        )));
    }
    let mut entries = Vec::new();
    for item in fs::read_dir(&srcpkgs)? {
        let item = item?;
        let path = item.path();
        let template = path.join("template");
        if !path.is_dir() || !template.is_file() {
            continue;
        }
        let raw = fs::read_to_string(&template)?;
        entries.push(PackageEntry {
            name: path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default(),
            version: summary_value(&raw, "version"),
            description: summary_value(&raw, "short_desc"),
            build_style: summary_value(&raw, "build_style"),
            path,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

pub fn create(workspace: &Workspace, name: &str) -> Result<PathBuf> {
    if !valid_name(name) {
        return Err(invalid(format!("invalid package name: {name}")));
    }
    let dir = workspace.path.join("srcpkgs").join(name);
    if dir.exists() {
        return Err(invalid(format!("package already exists: {name}")));
    }
    fs::create_dir_all(&dir)?;
    let template = format!(
        "pkgname={name}\nversion=0.0.0\nrevision=1\nbuild_style=configure\nshort_desc=\nmaintainer=\nlicense=\nhomepage=\ndistfiles=\nchecksum=\ndepends=\n"
    );
    fs::write(dir.join("template"), template)?;
    Ok(dir)
}

pub fn duplicate(workspace: &Workspace, source: &str, target: &str) -> Result<PathBuf> {
    if !valid_name(target) {
        return Err(invalid(format!("invalid package name: {target}")));
    }
    let src = workspace.path.join("srcpkgs").join(source);
    let dst = workspace.path.join("srcpkgs").join(target);
    if !src.is_dir() {
        return Err(not_found(format!("package not found: {source}")));
    }
    if dst.exists() {
        return Err(invalid(format!("package already exists: {target}")));
    }
    copy_tree(&src, &dst)?;
    let template_path = dst.join("template");
    if let Ok(text) = fs::read_to_string(&template_path) {
        let replaced = text.replace(&format!("pkgname={source}"), &format!("pkgname={target}"));
        fs::write(template_path, replaced)?;
    }
    Ok(dst)
}

pub fn rename(workspace: &Workspace, source: &str, target: &str) -> Result<PathBuf> {
    if !valid_name(target) {
        return Err(invalid(format!("invalid package name: {target}")));
    }
    let src = workspace.path.join("srcpkgs").join(source);
    let dst = workspace.path.join("srcpkgs").join(target);
    if !src.is_dir() {
        return Err(not_found(format!("package not found: {source}")));
    }
    if dst.exists() {
        return Err(invalid(format!("package already exists: {target}")));
    }
    fs::rename(&src, &dst)?;
    let template_path = dst.join("template");
    if let Ok(text) = fs::read_to_string(&template_path) {
        let replaced = text.replace(&format!("pkgname={source}"), &format!("pkgname={target}"));
        fs::write(template_path, replaced)?;
    }
    Ok(dst)
}

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_void_template() {
        let dir = std::env::temp_dir().join(format!("vf-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("template");
        fs::write(
            &path,
            "# comment\npkgname=foo\nversion=1.2.3\ndepends=\"bar baz \\\n  qux\"\nfoo_package() {\n\tpkgdesc=\"sub\"\n}\n",
        )
        .unwrap();
        let info = parse_template(&path).unwrap();
        assert_eq!(info.get("pkgname"), Some("foo"));
        assert_eq!(info.list("depends"), vec!["bar", "baz", "qux"]);
        assert!(info.subpackages().contains(&"foo_package".to_string()));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validates_names() {
        assert!(valid_name("foo-bar+git"));
        assert!(!valid_name("Foo"));
        assert!(!valid_name("../evil"));
    }
}
