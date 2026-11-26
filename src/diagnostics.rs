use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use crate::error::Result;
use crate::models::Finding;

struct Rule {
    pattern: &'static str,
    title: &'static str,
    cause: &'static str,
    suggestion: &'static str,
}

const RULES: &[Rule] = &[
    Rule {
        pattern: r"(?i)(missing.*dependenc|cannot find -l|no package .* found|unresolved dependency)",
        title: "Missing dependency",
        cause: "A build-time or runtime dependency is not available",
        suggestion: "Check makedepends/hostmakedepends and confirm the package exists in srcpkgs or the repository",
    },
    Rule {
        pattern: r"(?i)(cmake error|could not find a package configuration file|cmake could not find)",
        title: "CMake configuration error",
        cause: "CMake could not locate a required package or tool",
        suggestion: "Inspect CMakeLists.txt and the corresponding *-devel package in makedepends",
    },
    Rule {
        pattern: r"(?i)(fatal error:|error: .*undeclared|expected .* before)",
        title: "Compiler error",
        cause: "Source code failed to compile with the selected toolchain",
        suggestion: "Read the full compiler output and check compiler flags and version requirements",
    },
    Rule {
        pattern: r"(?i)(undefined reference|cannot find -l|ld: .*error)",
        title: "Linker error",
        cause: "A library was not linked or a symbol is missing",
        suggestion: "Verify LDFLAGS and that the providing -devel package is in makedepends",
    },
    Rule {
        pattern: r"(?i)(hunk #[0-9]+ failed|patch does not apply|reversed \(or previously applied\) patch)",
        title: "Patch failure",
        cause: "A patch no longer matches the extracted source",
        suggestion: "Compare the patch with the current upstream source and regenerate it from a clean tree",
    },
    Rule {
        pattern: r"(?i)(checksum mismatch|sha256.*mismatch|verification failed)",
        title: "Checksum mismatch",
        cause: "The downloaded file differs from the template checksum",
        suggestion: "Verify the upstream file and update checksum deliberately, never to bypass a corrupted download",
    },
    Rule {
        pattern: r"(?i)(no such file or directory|cannot open|not found)",
        title: "Missing file",
        cause: "A required file or directory does not exist at the expected path",
        suggestion: "Check do_install paths, distfiles, and extracted source layout",
    },
    Rule {
        pattern: r"(?i)(configure: error|checking for .* no$)",
        title: "Configure failure",
        cause: "The configure step rejected the build environment or dependencies",
        suggestion: "Inspect configure_args and the preceding checks in the log",
    },
    Rule {
        pattern: r"(?i)(permission denied|operation not permitted)",
        title: "Build environment problem",
        cause: "A file or directory has incompatible ownership or permissions",
        suggestion: "Confirm the build runs as the unprivileged user and that masterdir is writable",
    },
];

fn compiled() -> &'static Vec<(Regex, &'static Rule)> {
    static RULES_CACHE: OnceLock<Vec<(Regex, &'static Rule)>> = OnceLock::new();
    RULES_CACHE.get_or_init(|| {
        RULES
            .iter()
            .map(|rule| (Regex::new(rule.pattern).unwrap(), rule))
            .collect()
    })
}

pub fn analyze(path: &Path) -> Result<Vec<Finding>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut findings = Vec::new();
    let mut tail: VecDeque<String> = VecDeque::with_capacity(12);
    let mut counts: Vec<usize> = vec![0; RULES.len()];

    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        tail.push_back(line.clone());
        if tail.len() > 12 {
            tail.pop_front();
        }
        for (rule_index, (regex, rule)) in compiled().iter().enumerate() {
            if counts[rule_index] >= 4 || !regex.is_match(&line) {
                continue;
            }
            counts[rule_index] += 1;
            findings.push(
                Finding::error(rule.title, format!("{}. {}", rule.cause, rule.suggestion))
                    .at_line(index + 1)
                    .with_evidence(line.trim().chars().take(240).collect::<String>()),
            );
        }
    }

    if findings.is_empty() {
        findings.push(Finding::info(
            "No known error signature",
            "No diagnostic pattern matched this log. Inspect the final log lines manually.",
        ));
    }
    for line in tail {
        findings.push(Finding::info(
            "Log tail",
            line.chars().take(240).collect::<String>(),
        ));
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_compile() {
        assert!(!compiled().is_empty());
        assert!(compiled()[0].0.is_match("missing dependency foo"));
    }
}
