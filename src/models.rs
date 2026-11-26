use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct HostInfo {
    pub os_pretty: String,
    pub os_id: String,
    pub os_version: String,
    pub kernel: String,
    pub arch: String,
    pub libc: String,
    pub supported: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ToolStatus {
    pub name: String,
    pub found: bool,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct SystemMetrics {
    pub cpu_percent: f32,
    pub memory_total: u64,
    pub memory_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disks: Vec<DiskInfo>,
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    pub path: PathBuf,
    pub branch: String,
    pub remotes: Vec<String>,
    pub valid: bool,
    pub created_at: String,
}

#[derive(Clone, Debug, Default)]
pub struct TemplateInfo {
    pub path: PathBuf,
    pub raw: String,
    pub vars: HashMap<String, String>,
    pub lines: HashMap<String, usize>,
    pub functions: Vec<String>,
    pub line_count: usize,
}

impl TemplateInfo {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        self.get(key)
            .map(|value| value.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default()
    }

    pub fn name(&self) -> String {
        self.get("pkgname").unwrap_or("unknown").to_string()
    }

    pub fn version(&self) -> Option<&str> {
        self.get("version")
    }

    pub fn subpackages(&self) -> Vec<String> {
        self.functions
            .iter()
            .filter(|name| name.ends_with("_package"))
            .cloned()
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct PackageEntry {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub build_style: Option<String>,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub evidence: Option<String>,
    pub line: Option<usize>,
    pub verified: bool,
}

impl Finding {
    pub fn error(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            title: title.into(),
            detail: detail.into(),
            evidence: None,
            line: None,
            verified: true,
        }
    }

    pub fn warning(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            title: title.into(),
            detail: detail.into(),
            evidence: None,
            line: None,
            verified: true,
        }
    }

    pub fn info(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            title: title.into(),
            detail: detail.into(),
            evidence: None,
            line: None,
            verified: false,
        }
    }

    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence = Some(evidence.into());
        self
    }

    pub fn at_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DepKind {
    Depends,
    Makedepends,
    Hostmakedepends,
    Checkdepends,
}

impl DepKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DepKind::Depends => "depends",
            DepKind::Makedepends => "makedepends",
            DepKind::Hostmakedepends => "hostmakedepends",
            DepKind::Checkdepends => "checkdepends",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Dependency {
    pub name: String,
    pub kind: DepKind,
    pub in_repository: bool,
    pub installed: bool,
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildOp {
    Bootstrap,
    BinaryBootstrap,
    Fetch,
    Extract,
    Patch,
    Configure,
    Build,
    Check,
    Install,
    Package,
    Clean,
}

impl BuildOp {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "bootstrap" => Some(Self::Bootstrap),
            "binary-bootstrap" | "binarybootstrap" => Some(Self::BinaryBootstrap),
            "fetch" => Some(Self::Fetch),
            "extract" => Some(Self::Extract),
            "patch" => Some(Self::Patch),
            "configure" => Some(Self::Configure),
            "build" => Some(Self::Build),
            "check" => Some(Self::Check),
            "install" => Some(Self::Install),
            "pkg" | "package" => Some(Self::Package),
            "clean" => Some(Self::Clean),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::BinaryBootstrap => "binary-bootstrap",
            Self::Fetch => "fetch",
            Self::Extract => "extract",
            Self::Patch => "patch",
            Self::Configure => "configure",
            Self::Build => "build",
            Self::Check => "check",
            Self::Install => "install",
            Self::Package => "pkg",
            Self::Clean => "clean",
        }
    }

    pub fn all() -> &'static [BuildOp] {
        &[
            Self::Bootstrap,
            Self::BinaryBootstrap,
            Self::Fetch,
            Self::Extract,
            Self::Patch,
            Self::Configure,
            Self::Build,
            Self::Check,
            Self::Install,
            Self::Package,
            Self::Clean,
        ]
    }
}

#[derive(Clone, Debug)]
pub struct BuildRecord {
    pub id: i64,
    pub workspace: String,
    pub package: String,
    pub operation: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub log_path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct GitChange {
    pub status: String,
    pub path: String,
}

#[derive(Clone, Debug, Default)]
pub struct GitSnapshot {
    pub branch: String,
    pub remotes: Vec<String>,
    pub changes: Vec<GitChange>,
    pub log: Vec<String>,
    pub diff: String,
}

#[derive(Clone, Debug)]
pub struct OutputPackage {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub version: String,
}
