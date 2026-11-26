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

/// Void package names may start with an uppercase letter (`R-cran-*`,
/// `AppStream`, ...), so validation uses the same rules as xbps-src.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || "+-_.".contains(c))
}

/// Strip a shell comment while remembering quote state across physical lines.
/// Void templates commonly continue a quoted value on the next line; a `#`
/// inside that value is data, not a comment.
fn strip_comment(line: &str, quote: &mut Option<char>, param_depth: &mut usize) -> String {
    let mut out = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\\' {
            out.push(ch);
            if index + 1 < chars.len() {
                index += 1;
                out.push(chars[index]);
            }
            index += 1;
            continue;
        }
        match *quote {
            Some(expected) => {
                out.push(ch);
                if ch == expected {
                    *quote = None;
                }
                index += 1;
            }
            None if ch == '\'' || ch == '"' => {
                *quote = Some(ch);
                out.push(ch);
                index += 1;
            }
            // `${version%.*}` contains `#`-like shell operators; never treat a
            // `#` inside a parameter expansion as the start of a comment.
            None if ch == '$' && chars.get(index + 1) == Some(&'{') => {
                *param_depth += 1;
                out.push(ch);
                out.push('{');
                index += 2;
            }
            None if *param_depth > 0 && ch == '{' => {
                *param_depth += 1;
                out.push(ch);
                index += 1;
            }
            None if *param_depth > 0 && ch == '}' => {
                *param_depth = param_depth.saturating_sub(1);
                out.push(ch);
                index += 1;
            }
            None if ch == '#' && *param_depth == 0 => break,
            None => {
                out.push(ch);
                index += 1;
            }
        }
    }
    out
}

fn parse_value(raw: &str) -> String {
    let text = raw.trim();
    let Some(first) = text.chars().next() else {
        return String::new();
    };
    if first != '\'' && first != '"' {
        return text.split_whitespace().next().unwrap_or("").to_string();
    }
    let mut out = String::new();
    let mut escaped = false;
    for ch in text.chars().skip(1) {
        if escaped {
            // Inside double quotes, backslash only escapes these characters;
            // anywhere else it is literal shell text.
            if first == '"' && matches!(ch, '"' | '\\' | '$' | '`') {
                out.push(ch);
            } else {
                out.push('\\');
                out.push(ch);
            }
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if ch == first {
            break;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Join physical lines into shell logical lines. Continuation happens on a
/// trailing backslash *or* while a quote is still open, which is how upstream
/// templates wrap long `depends`, `distfiles`, and `checksum` values.
fn logical_lines(raw: &str) -> Vec<(usize, String)> {
    let mut lines = Vec::new();
    let mut buffer = String::new();
    let mut start = 1usize;
    let mut quote = None;
    let mut param_depth = 0usize;
    for (index, physical) in raw.lines().enumerate() {
        let clean = strip_comment(physical, &mut quote, &mut param_depth);
        let clean = clean.trim_end();
        // Comments and blank lines inside a continued value are not data.
        if clean.trim().is_empty() {
            continue;
        }
        if buffer.is_empty() {
            start = index + 1;
            buffer.push_str(clean);
        } else {
            buffer.push(' ');
            buffer.push_str(clean.trim());
        }
        let trailing_slashes = buffer.chars().rev().take_while(|ch| *ch == '\\').count();
        if trailing_slashes % 2 == 1 || quote.is_some() {
            if trailing_slashes % 2 == 1 {
                buffer.pop();
                let trimmed = buffer.trim_end().len();
                buffer.truncate(trimmed);
            }
            continue;
        }
        lines.push((start, std::mem::take(&mut buffer)));
    }
    if !buffer.is_empty() {
        lines.push((start, buffer));
    }
    lines
}

fn brace_delta(line: &str) -> i32 {
    let mut delta = 0;
    let mut quote = None;
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        match quote {
            Some(expected) => {
                if ch == expected {
                    quote = None;
                }
            }
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch == '{' => delta += 1,
            None if ch == '}' => delta -= 1,
            None => {}
        }
    }
    delta
}

/// Only top-level assignments count as template variables. Assignments inside
/// `foo_package() { ... }` are function-local shell variables; treating them
/// as globals produced bogus dependencies and checksum errors.
fn assignments_from_logical(logical: &[(usize, String)]) -> Vec<(usize, String, String)> {
    let mut assignments = Vec::new();
    let mut depth = 0i32;
    for (line_number, line) in logical {
        let trimmed = line.trim();
        if depth > 0 {
            depth += brace_delta(trimmed);
            if depth < 0 {
                depth = 0;
            }
            continue;
        }
        if function_regex().is_match(trimmed) {
            depth = brace_delta(trimmed).max(1);
            continue;
        }
        if let Some(captures) = assignment_regex().captures(trimmed) {
            assignments.push((
                *line_number,
                captures[1].to_string(),
                parse_value(&captures[2]),
            ));
        }
    }
    assignments
}

/// Normalise a dependency token to its package name. `virtual?foo`,
/// `foo>=1.0_1` and `foo-1.2.3_1` are all common in Void templates.
pub fn dependency_name(raw: &str) -> String {
    let token = raw.trim().strip_prefix("virtual?").unwrap_or(raw.trim());
    let base = token
        .split(['<', '>', '=', '(', ')', '?'])
        .next()
        .unwrap_or(token)
        .trim();
    if let Some(revision_at) = base.rfind('_') {
        if revision_at + 1 < base.len()
            && base[revision_at + 1..]
                .chars()
                .all(|ch| ch.is_ascii_digit())
        {
            if let Some(version_at) = base[..revision_at].rfind('-') {
                if base[version_at + 1..revision_at]
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_digit())
                {
                    return base[..version_at].to_string();
                }
            }
        }
    }
    base.to_string()
}

fn expand_variables(vars: HashMap<String, String>) -> HashMap<String, String> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)").unwrap()
    });
    let mut result = vars;
    for _ in 0..4 {
        let snapshot = result.clone();
        let mut changed = false;
        for value in result.values_mut() {
            if !value.contains('$') {
                continue;
            }
            let next = pattern.replace_all(value, |caps: &regex::Captures| -> String {
                let key = caps
                    .get(1)
                    .or_else(|| caps.get(2))
                    .map(|part| part.as_str())
                    .unwrap_or_default();
                snapshot.get(key).cloned().unwrap_or_else(|| {
                    caps.get(0)
                        .map(|all| all.as_str().to_string())
                        .unwrap_or_default()
                })
            });
            if next != *value {
                *value = next.into_owned();
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

    let logical = logical_lines(&raw);
    let mut vars = HashMap::new();
    let mut line_map: HashMap<String, usize> = HashMap::new();
    for (line_number, key, value) in assignments_from_logical(&logical) {
        line_map.insert(key.clone(), line_number);
        vars.insert(key, value);
    }
    let mut functions = Vec::new();
    for (_, line) in &logical {
        if let Some(captures) = function_regex().captures(line.trim()) {
            functions.push(captures[1].to_string());
        }
    }
    vars = expand_variables(vars);
    Ok(TemplateInfo {
        path: path.to_path_buf(),
        raw,
        vars,
        lines: line_map,
        functions,
        line_count: logical.len(),
    })
}

/// Read one field directly from the raw text. Building the full
/// `TemplateInfo` (regexes, logical lines, variable expansion) for all
/// ~15.000 templates just to fill a list costs many seconds; this cheap
/// lookup is enough for the package browser.
/// Cheap top-level lookup used by the package browser and cycle detector.
pub fn summary_value(raw: &str, key: &str) -> Option<String> {
    summary_values(raw, &[key]).remove(key)
}

/// Parse several fields in one pass over the logical lines.
pub fn summary_values(raw: &str, keys: &[&str]) -> HashMap<String, String> {
    let logical = logical_lines(raw);
    let mut values = HashMap::new();
    for (_, key, value) in assignments_from_logical(&logical) {
        if keys.contains(&key.as_str()) {
            values.entry(key).or_insert(value);
        }
    }
    values
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
        // One unreadable entry must not hide the whole repository.
        let Ok(item) = item else { continue };
        let path = item.path();
        let template = path.join("template");
        if !path.is_dir() || !template.is_file() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&template) else {
            continue;
        };
        let values = summary_values(&raw, &["version", "short_desc", "build_style"]);
        entries.push(PackageEntry {
            name: path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default(),
            version: values.get("version").cloned(),
            description: values.get("short_desc").cloned(),
            build_style: values.get("build_style").cloned(),
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

/// Replace one top-level assignment without touching comments or functions.
fn replace_assignment(text: &str, key: &str, value: &str) -> String {
    let mut replaced = false;
    let prefix = format!("{key}=");
    let mut output: Vec<String> = text
        .lines()
        .map(|line| {
            if !replaced && line.trim_start().starts_with(&prefix) {
                replaced = true;
                format!("{key}={value}")
            } else {
                line.to_string()
            }
        })
        .collect();
    let mut result = output.join("\n");
    if text.ends_with('\n') {
        result.push('\n');
    }
    output.clear();
    result
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
    // Void uses symlinks for subpackages. Copy the real template, never the
    // link, and rewrite the pkgname actually found in that template.
    let real_src = fs::canonicalize(&src)?;
    copy_tree(&real_src, &dst)?;
    let template_path = dst.join("template");
    if let Ok(text) = fs::read_to_string(&template_path) {
        let updated = if summary_value(&text, "pkgname").is_some() {
            replace_assignment(&text, "pkgname", target)
        } else {
            format!("pkgname={target}\n{text}")
        };
        fs::write(&template_path, updated)?;
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
    if fs::symlink_metadata(&src)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(invalid(
            "cannot rename a symlinked subpackage; rename its real package instead",
        ));
    }
    if dst.exists() {
        return Err(invalid(format!("package already exists: {target}")));
    }
    fs::rename(&src, &dst)?;
    let template_path = dst.join("template");
    if let Ok(text) = fs::read_to_string(&template_path) {
        let updated = if summary_value(&text, "pkgname").is_some() {
            replace_assignment(&text, "pkgname", target)
        } else {
            format!("pkgname={target}\n{text}")
        };
        fs::write(template_path, updated)?;
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
        assert!(valid_name("R-cran-foo"));
        assert!(valid_name("AppStream"));
        assert!(!valid_name("../evil"));
        assert!(!valid_name("has space"));
    }

    #[test]
    fn parses_multiline_quotes_and_ignores_function_locals() {
        let raw = r#"pkgname=foo
version=1.0
depends="bar \
  baz"
checksum="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
foo_package() {
	pkgdesc="subpackage"
	depends=wrong
}
short_desc="a # kept inside quotes"
_tag=${version%.*}_${version##*.}
"#;
        let values = summary_values(
            raw,
            &["depends", "checksum", "pkgdesc", "short_desc", "_tag"],
        );
        assert_eq!(values["depends"], "bar baz");
        assert_eq!(values["checksum"].split_whitespace().count(), 2);
        assert!(!values.contains_key("pkgdesc"));
        assert_eq!(values["short_desc"], "a # kept inside quotes");
        assert_eq!(values["_tag"], "${version%.*}_${version##*.}");
    }

    #[test]
    fn normalises_dependency_names() {
        assert_eq!(dependency_name("virtual?python3"), "python3");
        assert_eq!(dependency_name("foo>=1.2_1"), "foo");
        assert_eq!(dependency_name("foo-1.2.3_1"), "foo");
        assert_eq!(dependency_name("foo-bar"), "foo-bar");
    }
}
