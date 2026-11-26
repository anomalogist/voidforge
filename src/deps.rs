use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::error::Result;
use crate::git;
use crate::models::{DepKind, Dependency, TemplateInfo, Workspace};
use crate::package;

fn clean_name(raw: &str) -> String {
    raw.split(['<', '>', '=', '(', ')'])
        .next()
        .unwrap_or(raw)
        .trim()
        .to_string()
}

pub fn repository_names(workspace: &Workspace) -> HashSet<String> {
    let srcpkgs = workspace.path.join("srcpkgs");
    fs::read_dir(srcpkgs)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().join("template").is_file())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

pub fn installed_packages() -> Vec<String> {
    let output = Command::new("xbps-query").arg("-l").output();
    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1))
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn is_installed(name: &str, installed: &[String]) -> bool {
    installed
        .iter()
        .any(|entry| entry == name || entry.starts_with(&format!("{name}-")))
}

pub fn for_package(
    workspace: &Workspace,
    info: &TemplateInfo,
    check_installed: bool,
) -> Result<Vec<Dependency>> {
    let repo = repository_names(workspace);
    let installed = if check_installed {
        installed_packages()
    } else {
        Vec::new()
    };
    let mut result = Vec::new();
    for (kind, key) in [
        (DepKind::Depends, "depends"),
        (DepKind::Makedepends, "makedepends"),
        (DepKind::Hostmakedepends, "hostmakedepends"),
        (DepKind::Checkdepends, "checkdepends"),
    ] {
        for raw in info.list(key) {
            let name = clean_name(&raw);
            if name.is_empty() {
                continue;
            }
            let detail = if raw != name {
                Some(format!("requested as {raw}"))
            } else {
                None
            };
            let in_repository = repo.contains(&name);
            let installed_package = is_installed(&name, &installed);
            result.push(Dependency {
                name,
                kind: kind.clone(),
                in_repository,
                installed: installed_package,
                detail,
            });
        }
    }
    result.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.kind.as_str().cmp(b.kind.as_str()))
    });
    Ok(result)
}

pub fn possible_cycles(workspace: &Workspace) -> Vec<Vec<String>> {
    let srcpkgs = workspace.path.join("srcpkgs");
    let entries = match fs::read_dir(&srcpkgs) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for entry in entries.filter_map(|entry| entry.ok()) {
        let template = entry.path().join("template");
        if !template.is_file() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&template) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        // Full template parsing runs shell expansion and is far too slow for
        // the 15k templates in a real void-packages tree. Cycle detection
        // only needs literal dependency names, so read those directly.
        let deps: Vec<String> = package::summary_value(&raw, "depends")
            .unwrap_or_default()
            .split_whitespace()
            .map(clean_name)
            .filter(|value| !value.is_empty() && srcpkgs.join(value).is_dir())
            .collect();
        graph.insert(name, deps);
    }
    // The old code silently returned "no cycles" for any real tree (>400
    // packages). An iterative DFS keeps this usable for ~10k templates
    // without risking a stack overflow from recursive calls.
    let mut state: HashMap<String, u8> = HashMap::new();
    let mut path: Vec<String> = Vec::new();
    let mut cycles: Vec<Vec<String>> = Vec::new();
    let roots: Vec<String> = graph.keys().cloned().collect();

    for root in roots {
        if state.get(&root).copied().unwrap_or(0) != 0 {
            continue;
        }
        state.insert(root.clone(), 1);
        path.push(root.clone());
        // (node, index of the next dependency to visit)
        let mut stack: Vec<(String, usize)> = vec![(root, 0)];

        while !stack.is_empty() {
            let top = stack.len() - 1;
            let node = stack[top].0.clone();
            let index = stack[top].1;
            let deps: &[String] = graph.get(&node).map(Vec::as_slice).unwrap_or(&[]);

            if index >= deps.len() {
                state.insert(node, 2);
                path.pop();
                stack.pop();
                continue;
            }

            let dep = deps[index].clone();
            stack[top].1 += 1;
            match state.get(&dep).copied().unwrap_or(0) {
                0 => {
                    state.insert(dep.clone(), 1);
                    path.push(dep.clone());
                    stack.push((dep, 0));
                }
                1 => {
                    if let Some(position) = path.iter().position(|item| item == &dep) {
                        let mut cycle = path[position..].to_vec();
                        cycle.push(dep);
                        if !cycles.contains(&cycle) {
                            cycles.push(cycle);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    cycles
}

pub fn summary(workspace: &Workspace, info: &TemplateInfo, check_installed: bool) -> String {
    let deps = for_package(workspace, info, check_installed).unwrap_or_default();
    let missing: Vec<&Dependency> = deps
        .iter()
        .filter(|dep| !dep.in_repository && !dep.installed)
        .collect();
    format!(
        "{} dependencies, {} unresolved, {} in repository",
        deps.len(),
        missing.len(),
        deps.iter().filter(|dep| dep.in_repository).count()
    )
}

pub fn diagnostics(deps: &[Dependency]) -> Vec<crate::models::Finding> {
    use crate::models::Finding;
    let mut findings = Vec::new();
    let mut seen = HashSet::new();
    for dep in deps {
        let key = format!("{}:{}", dep.kind.as_str(), dep.name);
        if !seen.insert(key) {
            findings.push(Finding::warning(
                "duplicate dependency",
                format!("{} appears twice in {}", dep.name, dep.kind.as_str()),
            ));
        }
        if !dep.in_repository && !dep.installed {
            findings.push(
                Finding::warning(
                    "unresolved dependency",
                    format!(
                        "{} ({}) is not in srcpkgs and not installed",
                        dep.name,
                        dep.kind.as_str()
                    ),
                )
                .with_evidence(dep.detail.clone().unwrap_or_else(|| dep.name.clone())),
            );
        }
    }
    findings
}

pub fn git_diff_for(workspace: &Workspace, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(&workspace.path).unwrap_or(path);
    git::diff(&workspace.path, Some(&relative.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Workspace;

    #[test]
    fn detects_cycles() {
        let dir = std::env::temp_dir().join(format!("vf-deps-test-{}", std::process::id()));
        let srcpkgs = dir.join("srcpkgs");
        let make = |name: &str, depends: &str| {
            let package = srcpkgs.join(name);
            fs::create_dir_all(&package).unwrap();
            fs::write(
                package.join("template"),
                format!("pkgname={name}\nversion=1\ndepends=\"{depends}\"\n"),
            )
            .unwrap();
        };
        make("a", "b");
        make("b", "c");
        make("c", "a");
        make("unrelated", "");

        let workspace = Workspace {
            id: 0,
            name: "test".into(),
            path: dir.clone(),
            branch: String::new(),
            remotes: vec![],
            valid: true,
            created_at: String::new(),
        };
        let cycles = possible_cycles(&workspace);
        assert!(
            cycles
                .iter()
                .any(|cycle| cycle.contains(&"a".to_string()) && cycle.contains(&"c".to_string())),
            "expected a -> b -> c -> a, got {cycles:?}"
        );
        fs::remove_dir_all(dir).ok();
    }
}
