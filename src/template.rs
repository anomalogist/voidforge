use std::collections::HashSet;

use crate::models::{Finding, Severity, TemplateInfo, Workspace};

const REQUIRED: [&str; 7] = [
    "pkgname",
    "version",
    "revision",
    "short_desc",
    "maintainer",
    "license",
    "homepage",
];

// Names from void-packages/common/build-style (without the .sh suffix),
// plus common aliases. Keeping this list in sync avoids false warnings on
// perfectly valid upstream templates.
const KNOWN_STYLES: &[&str] = &[
    "R-cran",
    "cabal",
    "cargo",
    "cmake",
    "configure",
    "fetch",
    "fetch-only",
    "gem",
    "gemspec",
    "gnu-configure",
    "gnu-makefile",
    "gnu",
    "go",
    "haskell-stack",
    "make",
    "meson",
    "meta",
    "perl-ModuleBuild",
    "perl-module",
    "python-module",
    "python2-module",
    "python3-module",
    "python3-pep517",
    "qmake",
    "raku-dist",
    "ruby-module",
    "scons",
    "sip-build",
    "slashpackage",
    "texmf",
    "tree-sitter",
    "void-cross",
    "waf",
    "waf3",
    "zig-build",
];

fn dep_name(raw: &str) -> &str {
    raw.split(['<', '>', '=', '(', ')'])
        .next()
        .unwrap_or(raw)
        .trim()
}

pub fn validate(info: &TemplateInfo, workspace: &Workspace) -> Vec<Finding> {
    let mut findings = Vec::new();
    for field in REQUIRED {
        let value = info.get(field).unwrap_or("").trim();
        let finding = if value.is_empty() {
            Finding::error(
                format!("missing {field}"),
                format!("Add a real value for {field} before running xbps-src"),
            )
        } else {
            Finding::info(format!("{field} present"), format!("{field} = {value}"))
        };
        findings.push(finding.at_line(*info.lines.get(field).unwrap_or(&0)));
    }

    let name = info.get("pkgname").unwrap_or("").trim();
    if !name.is_empty()
        && !name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        findings.push(
            Finding::error(
                "invalid pkgname",
                "Package names must start with a lowercase letter or digit",
            )
            .at_line(*info.lines.get("pkgname").unwrap_or(&0)),
        );
    }

    if let Some(version) = info.version() {
        if version.is_empty() || version.contains(char::is_whitespace) {
            findings.push(
                Finding::error(
                    "suspicious version",
                    "version must be a single non-empty token",
                )
                .at_line(*info.lines.get("version").unwrap_or(&0)),
            );
        }
    }

    if let Some(revision) = info.get("revision") {
        if !revision.is_empty() && !revision.chars().all(|c| c.is_ascii_digit()) {
            findings.push(
                Finding::warning(
                    "unusual revision",
                    "revision is normally a positive integer",
                )
                .at_line(*info.lines.get("revision").unwrap_or(&0)),
            );
        }
    }

    if let Some(style) = info.get("build_style") {
        if !style.is_empty() && !KNOWN_STYLES.contains(&style) {
            findings.push(Finding::warning(
                "unrecognized build_style",
                format!("Verify that {style} exists in common/build-style"),
            ));
        }
    }

    let has_distfiles = !info.list("distfiles").is_empty();
    let checksum = info.get("checksum").unwrap_or("").trim();
    if has_distfiles {
        if checksum.is_empty() {
            findings.push(
                Finding::error(
                    "missing checksum",
                    "distfiles are declared but checksum is empty",
                )
                .at_line(*info.lines.get("checksum").unwrap_or(&0)),
            );
        } else {
            let first = checksum.split_whitespace().next().unwrap_or("");
            if first.len() != 64 || !first.chars().all(|c| c.is_ascii_hexdigit()) {
                findings.push(
                    Finding::error(
                        "invalid checksum",
                        "checksum must start with a 64-character SHA256 hex value",
                    )
                    .at_line(*info.lines.get("checksum").unwrap_or(&0)),
                );
            }
        }
    } else if !checksum.is_empty() {
        findings.push(Finding::warning(
            "checksum without distfiles",
            "No distfiles were declared for this checksum",
        ));
    }

    if info.get("version").is_some_and(|value| value.contains('$')) {
        findings.push(Finding::warning(
            "unresolved variable",
            "version still contains a shell variable after expansion",
        ));
    }

    let mut seen: HashSet<String> = HashSet::new();
    for kind in ["depends", "makedepends", "hostmakedepends", "checkdepends"] {
        let line = *info.lines.get(kind).unwrap_or(&0);
        for item in info.list(kind) {
            let dependency = dep_name(&item);
            if dependency.is_empty() {
                continue;
            }
            if !seen.insert(format!("{kind}:{dependency}")) {
                findings.push(
                    Finding::warning(
                        "duplicate dependency",
                        format!("{dependency} appears more than once in {kind}"),
                    )
                    .at_line(line),
                );
            }
            if dependency != name && !workspace.path.join("srcpkgs").join(dependency).is_dir() {
                findings.push(
                    Finding::info(
                        "dependency not in repository",
                        format!("{dependency} is referenced by {kind} but has no srcpkgs entry; it may be installed or virtual"),
                    )
                    .at_line(line)
                    .with_evidence(dependency),
                );
            }
        }
    }

    // Duplicate detection already happened in the loop above; repeating it
    // here produced two identical warnings for every duplicated `depends` entry.

    let errors = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .count();
    findings.push(Finding::info(
        "static validation only",
        format!(
            "{} error(s); static checks cannot guarantee an xbps-src build succeeds",
            errors
        ),
    ));
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn template(text: &str) -> TemplateInfo {
        TemplateInfo {
            path: PathBuf::from("template"),
            raw: text.into(),
            vars: HashMap::from([
                ("pkgname".into(), "foo".into()),
                ("version".into(), "1.0".into()),
                ("revision".into(), "1".into()),
                ("short_desc".into(), "test".into()),
                ("maintainer".into(), "test <t@e>".into()),
                ("license".into(), "MIT".into()),
                ("homepage".into(), "https://example.com".into()),
            ]),
            lines: HashMap::new(),
            functions: vec![],
            line_count: 7,
        }
    }

    #[test]
    fn reports_missing_fields() {
        let workspace = Workspace {
            id: 0,
            name: "w".into(),
            path: PathBuf::from("/tmp/w"),
            branch: String::new(),
            remotes: vec![],
            valid: true,
            created_at: String::new(),
        };
        let findings = validate(&TemplateInfo::default(), &workspace);
        assert!(findings.iter().any(|f| f.title == "missing pkgname"));
    }

    #[test]
    fn accepts_complete_template() {
        let workspace = Workspace {
            id: 0,
            name: "w".into(),
            path: PathBuf::from("/tmp/w"),
            branch: String::new(),
            remotes: vec![],
            valid: true,
            created_at: String::new(),
        };
        let findings = validate(&template(""), &workspace);
        assert!(!findings.iter().any(|f| f.severity == Severity::Error));
    }
}
