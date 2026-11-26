use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sysinfo::{Disks, System};

use crate::build::BuildManager;
use crate::db::{self, Db};
use crate::error::{invalid, Result};
use crate::events::{NoticeLevel, UiEvent};
use crate::models::{
    BuildOp, BuildRecord, Dependency, Finding, GitSnapshot, HostInfo, OutputPackage, PackageEntry,
    Severity, SystemMetrics, TemplateInfo, ToolStatus, Workspace,
};
use crate::settings::{Settings, ThemeName};
use crate::system::{self, ToolScope};
use crate::{deps, diagnostics, git, package, patches, release, source, template, workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Workspaces,
    Packages,
    Template,
    Dependencies,
    Patches,
    BuildCenter,
    Diagnostics,
    Git,
    Settings,
}

impl Screen {
    pub const ALL: [Screen; 10] = [
        Screen::Dashboard,
        Screen::Workspaces,
        Screen::Packages,
        Screen::Template,
        Screen::Dependencies,
        Screen::Patches,
        Screen::BuildCenter,
        Screen::Diagnostics,
        Screen::Git,
        Screen::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Workspaces => "Workspaces",
            Self::Packages => "Packages",
            Self::Template => "Template",
            Self::Dependencies => "Dependencies",
            Self::Patches => "Patches",
            Self::BuildCenter => "Build Center",
            Self::Diagnostics => "Diagnostics",
            Self::Git => "Git & Repos",
            Self::Settings => "Settings",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Dashboard => "Host, toolchain, resources and recent activity",
            Self::Workspaces => "Register and validate void-packages repositories",
            Self::Packages => "Browse srcpkgs templates from disk",
            Self::Template => "Static validation and external editor integration",
            Self::Dependencies => "Dependency evidence, duplicates and cycles",
            Self::Patches => "Inspect, generate, apply and revert patches",
            Self::BuildCenter => "Official xbps-src phases with streamed logs",
            Self::Diagnostics => "Real log evidence with inferred causes",
            Self::Git => "Status, branches, diffs and remote operations",
            Self::Settings => "Theme, jobs, editor and safety preferences",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    RemoveWorkspace(i64),
    RenamePackage(String, String),
    RemovePatch(PathBuf),
    ApplyPatch { patch: PathBuf, reverse: bool },
    GitPull { remote: String, branch: String },
    GitPush { remote: String, branch: String },
    IndexRepository,
    CleanBuild(String),
    Quit,
}

#[derive(Clone, Debug)]
pub struct Confirm {
    pub title: String,
    pub detail: String,
    pub action: Action,
}

pub enum KeyOutcome {
    Continue,
    Edit(PathBuf),
}

pub struct App {
    pub database: Db,
    pub settings: Settings,
    pub host: HostInfo,
    pub metrics: SystemMetrics,
    pub tools: Vec<ToolStatus>,
    pub workspaces: Vec<Workspace>,
    pub active_ws: Option<usize>,
    pub packages: Vec<PackageEntry>,
    pub package_index: usize,
    pub filter: String,
    pub template: Option<TemplateInfo>,
    pub findings: Vec<Finding>,
    pub diagnostic_index: usize,
    pub dependencies: Vec<Dependency>,
    pub patches: Vec<PathBuf>,
    pub patch_index: usize,
    pub builds: Vec<BuildRecord>,
    pub build_index: usize,
    pub build_op_index: usize,
    pub build_logs: Vec<String>,
    pub git: GitSnapshot,
    pub git_index: usize,
    pub outputs: Vec<OutputPackage>,
    pub screen: Screen,
    pub command: String,
    pub command_mode: bool,
    pub message: Option<(NoticeLevel, String)>,
    pub confirm: Option<Confirm>,
    pub help: bool,
    pub running: bool,
    pub events_rx: Receiver<UiEvent>,
    pub builds_manager: BuildManager,
    pub active_build_id: Option<i64>,
    pub edit_request: Option<PathBuf>,
}

impl App {
    pub fn new() -> Result<Self> {
        let database = Db::open()?;
        let settings = Settings::load(database.conn())?;
        let host = system::detect_host();
        let mut sys = System::new_all();
        let mut disks = Disks::new_with_refreshed_list();
        let metrics = system::sample_metrics(&mut sys, &mut disks);
        let workspaces = db::workspaces(database.conn())?;
        let (tx, rx) = channel();
        let mut app = Self {
            database,
            settings,
            host,
            metrics,
            tools: Vec::new(),
            workspaces,
            active_ws: None,
            packages: Vec::new(),
            package_index: 0,
            filter: String::new(),
            template: None,
            findings: Vec::new(),
            diagnostic_index: 0,
            dependencies: Vec::new(),
            patches: Vec::new(),
            patch_index: 0,
            builds: Vec::new(),
            build_index: 0,
            build_op_index: 0,
            build_logs: Vec::new(),
            git: GitSnapshot::default(),
            git_index: 0,
            outputs: Vec::new(),
            screen: Screen::Dashboard,
            command: String::new(),
            command_mode: false,
            message: None,
            confirm: None,
            help: false,
            running: true,
            events_rx: rx,
            builds_manager: BuildManager::new(tx),
            active_build_id: None,
            edit_request: None,
        };
        if !app.workspaces.is_empty() {
            app.active_ws = Some(0);
        }
        app.refresh_all();
        Ok(app)
    }

    pub fn active_workspace(&self) -> Option<&Workspace> {
        self.active_ws.and_then(|index| self.workspaces.get(index))
    }

    pub fn filtered_packages(&self) -> Vec<PackageEntry> {
        let query = self.filter.trim().to_ascii_lowercase();
        self.packages
            .iter()
            .filter(|item| {
                query.is_empty()
                    || item.name.to_ascii_lowercase().contains(&query)
                    || item
                        .description
                        .as_deref()
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .contains(&query)
            })
            .cloned()
            .collect()
    }

    pub fn selected_package(&self) -> Option<PackageEntry> {
        self.filtered_packages().get(self.package_index).cloned()
    }

    pub fn selected_op(&self) -> BuildOp {
        BuildOp::all()[self.build_op_index % BuildOp::all().len()]
    }

    /// Full reload: settings, workspaces, package index, git state and the
    /// selected package. Used on startup and after commands that change data.
    pub fn refresh_all(&mut self) {
        self.workspaces = db::workspaces(self.database.conn()).unwrap_or_default();
        if self.workspaces.is_empty() {
            self.active_ws = None;
        } else {
            self.active_ws = Some(self.active_ws.unwrap_or(0).min(self.workspaces.len() - 1));
        }
        let scopes: Vec<ToolScope> = self
            .active_workspace()
            .map(|ws| vec![ToolScope::Workspace(ws.path.clone())])
            .unwrap_or_default();
        self.tools = system::detect_tools(&scopes);
        self.builds = db::builds(self.database.conn(), 100).unwrap_or_default();
        self.rescan_workspace_data();
        self.refresh_selection();
    }

    /// Re-read everything that lives inside the selected `void-packages`
    /// tree. This is the expensive part (thousands of templates plus git),
    /// so navigation keys must never call it.
    fn rescan_workspace_data(&mut self) {
        if let Some(ws) = self.active_workspace().cloned() {
            self.packages = package::scan(&ws).unwrap_or_default();
            self.git = git::snapshot(&ws.path).unwrap_or_default();
            self.outputs = release::list(&ws).unwrap_or_default();
        } else {
            self.packages.clear();
            self.outputs.clear();
            self.git = GitSnapshot::default();
        }
    }

    /// Cheap refresh of the currently selected package only. Safe to call on
    /// every selection change or screen switch.
    fn refresh_selection(&mut self) {
        self.package_index = self
            .package_index
            .min(self.filtered_packages().len().saturating_sub(1));
        if let Some(ws) = self.active_workspace().cloned() {
            if let Some(pkg) = self.selected_package() {
                self.template = package::parse_template(&pkg.path.join("template")).ok();
                if let Some(info) = &self.template {
                    self.dependencies = deps::for_package(&ws, info, self.settings.check_installed)
                        .unwrap_or_default();
                } else {
                    self.dependencies.clear();
                }
                self.patches = patches::list(&ws, &pkg.name).unwrap_or_default();
            } else {
                self.template = None;
                self.dependencies.clear();
                self.patches.clear();
            }
        } else {
            self.template = None;
            self.dependencies.clear();
            self.patches.clear();
        }
        self.patch_index = self.patch_index.min(self.patches.len().saturating_sub(1));
        self.diagnostic_index = self
            .diagnostic_index
            .min(self.findings.len().saturating_sub(1));
        self.git_index = self.git_index.min(self.git.changes.len().saturating_sub(1));
    }

    fn switch_screen(&mut self, screen: Screen) {
        self.screen = screen;
        self.refresh_selection();
    }

    pub fn notify(&mut self, level: NoticeLevel, message: impl Into<String>) {
        let text = message.into();
        let _ = db::record_activity(self.database.conn(), "notice", &text);
        self.message = Some((level, text));
    }

    pub fn confirm(&mut self, title: impl Into<String>, detail: impl Into<String>, action: Action) {
        self.confirm = Some(Confirm {
            title: title.into(),
            detail: detail.into(),
            action,
        });
    }

    /// Single exit path shared by `q`, `:quit` and `Ctrl+C`.
    pub fn request_quit(&mut self) {
        if self.builds_manager.active() > 0 {
            self.confirm(
                "Quit with active build?",
                "The running xbps-src process will be terminated.",
                Action::Quit,
            );
        } else {
            self.running = false;
        }
    }

    fn workspace_mut(&mut self) -> Result<Workspace> {
        self.active_workspace()
            .cloned()
            .ok_or_else(|| invalid("no active workspace"))
    }

    fn selected_template(&self) -> Result<(Workspace, PackageEntry, TemplateInfo)> {
        let ws = self
            .active_workspace()
            .cloned()
            .ok_or_else(|| invalid("no active workspace"))?;
        let pkg = self
            .selected_package()
            .ok_or_else(|| invalid("no package selected"))?;
        let info = package::parse_template(&pkg.path.join("template"))?;
        Ok((ws, pkg, info))
    }

    pub fn handle_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::Metrics(metrics) => {
                self.set_metrics(metrics);
            }
            UiEvent::BuildLog { build_id, line } => {
                if self.active_build_id == Some(build_id) {
                    self.build_logs.push(line);
                    if self.build_logs.len() > 600 {
                        self.build_logs.drain(..self.build_logs.len() - 600);
                    }
                }
            }
            UiEvent::BuildFinished {
                build_id,
                status,
                code,
                duration_seconds,
                log_path,
            } => {
                let _ = db::record_build_finish(
                    self.database.conn(),
                    build_id,
                    &status,
                    &log_path.to_string_lossy(),
                    duration_seconds,
                );
                if self.active_build_id == Some(build_id) {
                    self.active_build_id = None;
                }
                self.builds = db::builds(self.database.conn(), 100).unwrap_or_default();
                if status == "failed" {
                    if let Ok(found) = diagnostics::analyze(&log_path) {
                        self.findings = found;
                        self.diagnostic_index = 0;
                    }
                    self.screen = Screen::Diagnostics;
                    self.notify(
                        NoticeLevel::Error,
                        format!("build failed (exit {:?}), diagnostics opened", code),
                    );
                } else {
                    self.notify(
                        NoticeLevel::Success,
                        format!("build finished successfully in {}s", duration_seconds),
                    );
                }
            }
            UiEvent::Notice { level, message } => self.notify(level, message),
        }
    }

    fn set_metrics(&mut self, metrics: SystemMetrics) {
        self.metrics = metrics;
    }

    pub fn start_build(&mut self, package_name: &str, op: BuildOp) -> Result<()> {
        if self.active_build_id.is_some() {
            return Err(invalid(
                "a build is already running; cancel it before starting another",
            ));
        }
        let ws = self.workspace_mut()?;
        if !ws.path.join("srcpkgs").join(package_name).is_dir() {
            return Err(invalid(format!("package not found: {package_name}")));
        }
        let log_dir = self.settings.log_dir()?;
        let id =
            db::record_build_start(self.database.conn(), Some(ws.id), package_name, op.as_str())?;
        let path =
            match self
                .builds_manager
                .start(id, &ws, package_name, op, self.settings.jobs, &log_dir)
            {
                Ok(path) => path,
                Err(error) => {
                    let _ = db::delete_build(self.database.conn(), id);
                    self.builds = db::builds(self.database.conn(), 100).unwrap_or_default();
                    return Err(error);
                }
            };
        self.active_build_id = Some(id);
        self.build_logs = vec![format!(
            "Started {} {} with -j{} — log: {}",
            op.as_str(),
            package_name,
            self.settings.jobs,
            path.display()
        )];
        self.screen = Screen::BuildCenter;
        self.notify(
            NoticeLevel::Info,
            format!("{} {} started", op.as_str(), package_name),
        );
        Ok(())
    }

    fn apply_action(&mut self, action: Action) -> Result<String> {
        match action {
            Action::RemoveWorkspace(id) => {
                workspace::remove(self.database.conn(), id)?;
                self.active_ws = None;
                self.refresh_all();
                Ok("workspace removed from database (files were not touched)".into())
            }
            Action::RenamePackage(old, new) => {
                let ws = self.workspace_mut()?;
                let path = package::rename(&ws, &old, &new)?;
                self.refresh_all();
                Ok(format!("renamed {old} to {new} at {}", path.display()))
            }
            Action::RemovePatch(path) => {
                patches::remove(&path)?;
                self.refresh_all();
                Ok("patch removed".into())
            }
            Action::ApplyPatch { patch, reverse } => {
                let (ws, _, info) = self.selected_template()?;
                let backups = patches::apply(&ws, &info, &patch, reverse)?;
                Ok(format!(
                    "patch {} applied; {} backup(s) created",
                    patch.display(),
                    backups.len()
                ))
            }
            Action::GitPull { remote, branch } => {
                let ws = self.workspace_mut()?;
                let output = git::pull(&ws.path, &remote, &branch)?;
                db::record_git(
                    self.database.conn(),
                    Some(ws.id),
                    "pull",
                    &format!("{remote}/{branch}"),
                    "success",
                )?;
                self.refresh_all();
                Ok(format!(
                    "pulled {remote}/{branch}: {}",
                    output.lines().next().unwrap_or("ok")
                ))
            }
            Action::GitPush { remote, branch } => {
                let ws = self.workspace_mut()?;
                let output = git::push(&ws.path, &remote, &branch)?;
                db::record_git(
                    self.database.conn(),
                    Some(ws.id),
                    "push",
                    &format!("{remote}/{branch}"),
                    "success",
                )?;
                Ok(format!(
                    "pushed {remote}/{branch}: {}",
                    output.lines().next().unwrap_or("ok")
                ))
            }
            Action::IndexRepository => {
                let ws = self.workspace_mut()?;
                let output = release::index(&ws)?;
                Ok(format!("repository indexed: {output}"))
            }
            Action::CleanBuild(name) => {
                self.start_build(&name, BuildOp::Clean)?;
                Ok("clean started".into())
            }
            Action::Quit => {
                // Make the confirmation honest: terminate the process group
                // before leaving so no xbps-src child is left behind.
                self.builds_manager.cancel_all();
                self.running = false;
                Ok("exiting".into())
            }
        }
    }

    pub fn execute_command(&mut self, raw: &str) -> Result<String> {
        let input = raw.trim().trim_start_matches(':').trim();
        let tokens = tokenize(input);
        if tokens.is_empty() {
            return Ok(String::new());
        }
        match tokens[0].as_str() {
            "help" | "?" => {
                self.help = true;
                Ok("help opened".into())
            }
            "quit" | "q" => {
                self.request_quit();
                if self.confirm.is_some() {
                    Ok("confirmation required".into())
                } else {
                    Ok("exiting".into())
                }
            }
            "clear" => {
                self.message = None;
                Ok(String::new())
            }
            "filter" => {
                self.filter = tokens.get(1).cloned().unwrap_or_default();
                self.package_index = 0;
                self.refresh_all();
                Ok(format!("filter: {}", self.filter))
            }
            "theme" => {
                let theme = ThemeName::parse(tokens.get(1).map(String::as_str).unwrap_or("dark"));
                self.settings.theme = theme;
                self.settings.save(self.database.conn())?;
                Ok(format!("theme set to {}", theme.as_str()))
            }
            "jobs" => {
                let jobs: usize = tokens
                    .get(1)
                    .ok_or_else(|| invalid("usage: jobs <1-64>"))?
                    .parse()
                    .map_err(|_| invalid("jobs must be a number"))?;
                if !(1..=64).contains(&jobs) {
                    return Err(invalid("jobs must be between 1 and 64"));
                }
                self.settings.jobs = jobs;
                self.settings.save(self.database.conn())?;
                Ok(format!("parallel jobs set to {jobs}"))
            }
            "editor" => {
                let editor = tokens[1..].join(" ");
                if editor.trim().is_empty() {
                    return Err(invalid("usage: editor <command>"));
                }
                self.settings.editor = editor.clone();
                self.settings.save(self.database.conn())?;
                Ok(format!("editor set to {editor}"))
            }
            "workspace" => self.command_workspace(&tokens),
            "pkg" => self.command_package(&tokens),
            "template" => self.command_template(&tokens),
            "deps" => {
                let ws = self.workspace_mut()?;
                if tokens.get(1).map(String::as_str) == Some("cycles") {
                    self.findings = deps::possible_cycles(&ws)
                        .into_iter()
                        .map(|cycle| {
                            Finding::warning(
                                "possible dependency cycle",
                                format!("cycle: {}", cycle.join(" -> ")),
                            )
                        })
                        .collect();
                    if self.findings.is_empty() {
                        self.findings.push(Finding::info(
                            "no cycles",
                            "No runtime dependency cycle was detected",
                        ));
                    }
                    self.screen = Screen::Diagnostics;
                    Ok("cycle analysis complete".into())
                } else {
                    self.screen = Screen::Dependencies;
                    Ok(format!("{} dependencies loaded", self.dependencies.len()))
                }
            }
            "patch" => self.command_patch(&tokens),
            "build" => self.command_build(&tokens),
            "source" => self.command_source(&tokens),
            "diagnose" => self.command_diagnose(&tokens),
            "git" => self.command_git(&tokens),
            "release" => self.command_release(&tokens),
            "view" => {
                let screen = tokens
                    .get(1)
                    .and_then(|value| screen_from_name(value))
                    .ok_or_else(|| invalid("unknown screen"))?;
                self.screen = screen;
                Ok(format!("viewing {}", screen.label()))
            }
            other => Err(invalid(format!("unknown command `{other}`; run :help"))),
        }
    }

    fn command_workspace(&mut self, tokens: &[String]) -> Result<String> {
        match tokens.get(1).map(String::as_str) {
            Some("add") => {
                let path = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: workspace add <path>"))?;
                let ws = workspace::add(self.database.conn(), path)?;
                self.refresh_all();
                self.active_ws = self.workspaces.iter().position(|item| item.id == ws.id);
                self.refresh_all();
                Ok(format!("workspace {} added (valid={})", ws.name, ws.valid))
            }
            Some("clone") => {
                let url = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: workspace clone <url> [name]"))?;
                let ws =
                    workspace::clone(self.database.conn(), url, tokens.get(3).map(String::as_str))?;
                self.refresh_all();
                self.active_ws = self.workspaces.iter().position(|item| item.id == ws.id);
                self.refresh_all();
                Ok(format!("cloned into {}", ws.path.display()))
            }
            Some("use") => {
                let name = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: workspace use <name>"))?;
                let index = self
                    .workspaces
                    .iter()
                    .position(|item| item.name == *name || item.path.to_string_lossy() == *name)
                    .ok_or_else(|| invalid("workspace not found"))?;
                self.active_ws = Some(index);
                self.package_index = 0;
                self.refresh_all();
                Ok(format!("active workspace: {}", self.workspaces[index].name))
            }
            Some("remove") => {
                let name = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: workspace remove <name>"))?;
                let ws = self
                    .workspaces
                    .iter()
                    .find(|item| item.name == *name)
                    .cloned()
                    .ok_or_else(|| invalid("workspace not found"))?;
                self.confirm(
                    "Remove workspace?",
                    "Only the database entry is removed; repository files stay on disk.",
                    Action::RemoveWorkspace(ws.id),
                );
                Ok("confirmation required".into())
            }
            Some("validate") => {
                let ws = self.workspace_mut()?;
                let findings = workspace::validate(&ws.path);
                let errors = findings
                    .iter()
                    .filter(|item| item.severity == Severity::Error)
                    .count();
                self.findings = findings;
                self.screen = Screen::Diagnostics;
                Ok(format!("workspace validation: {errors} error(s)"))
            }
            Some("refresh") | None => {
                let ws = self.workspace_mut()?;
                let updated = workspace::refresh(self.database.conn(), &ws)?;
                if let Some(index) = self.active_ws {
                    self.workspaces[index] = updated;
                }
                self.refresh_all();
                Ok("workspace refreshed".into())
            }
            Some(other) => Err(invalid(format!("unknown workspace command: {other}"))),
        }
    }

    fn command_package(&mut self, tokens: &[String]) -> Result<String> {
        match tokens.get(1).map(String::as_str) {
            Some("list") => {
                self.screen = Screen::Packages;
                Ok(format!("{} packages", self.packages.len()))
            }
            Some("new") => {
                let name = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: pkg new <name>"))?;
                let ws = self.workspace_mut()?;
                let path = package::create(&ws, name)?;
                self.refresh_all();
                Ok(format!("created {}", path.display()))
            }
            Some("duplicate") => {
                let source = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: pkg duplicate <src> <dst>"))?;
                let target = tokens
                    .get(3)
                    .ok_or_else(|| invalid("usage: pkg duplicate <src> <dst>"))?;
                let ws = self.workspace_mut()?;
                let path = package::duplicate(&ws, source, target)?;
                self.refresh_all();
                Ok(format!("duplicated into {}", path.display()))
            }
            Some("rename") => {
                let source = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: pkg rename <old> <new>"))?;
                let target = tokens
                    .get(3)
                    .ok_or_else(|| invalid("usage: pkg rename <old> <new>"))?;
                self.confirm(
                    "Rename package?",
                    format!("{source} -> {target}; the directory will be moved on disk."),
                    Action::RenamePackage(source.clone(), target.clone()),
                );
                Ok("confirmation required".into())
            }
            Some("open") => {
                let name = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: pkg open <name>"))?;
                let index = self
                    .filtered_packages()
                    .iter()
                    .position(|item| item.name == *name)
                    .ok_or_else(|| invalid("package not found"))?;
                self.package_index = index;
                self.screen = Screen::Template;
                self.refresh_all();
                Ok(format!("opened {name}"))
            }
            _ => Err(invalid("usage: pkg list|new|duplicate|rename|open")),
        }
    }

    fn command_template(&mut self, tokens: &[String]) -> Result<String> {
        match tokens.get(1).map(String::as_str) {
            Some("edit") => {
                let (_, pkg, _) = self.selected_template()?;
                let path = pkg.path.join("template");
                self.edit_request = Some(path.clone());
                Ok(format!(
                    "opening {} in {}",
                    path.display(),
                    self.settings.editor
                ))
            }
            Some("validate") | None => {
                let (ws, _, info) = self.selected_template()?;
                self.findings = template::validate(&info, &ws);
                self.diagnostic_index = 0;
                self.screen = Screen::Template;
                let errors = self
                    .findings
                    .iter()
                    .filter(|item| item.severity == Severity::Error)
                    .count();
                Ok(format!("static validation: {errors} error(s)"))
            }
            Some(other) => Err(invalid(format!("unknown template command: {other}"))),
        }
    }

    fn command_patch(&mut self, tokens: &[String]) -> Result<String> {
        match tokens.get(1).map(String::as_str) {
            Some("list") => {
                self.screen = Screen::Patches;
                Ok(format!("{} patches", self.patches.len()))
            }
            Some("new") => {
                let (ws, pkg, _) = self.selected_template()?;
                let name = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: patch new <name>.patch"))?;
                let path = patches::create_draft(&ws, &pkg.name, name)?;
                self.refresh_all();
                Ok(format!("draft created: {}", path.display()))
            }
            Some("from-git") => {
                let (ws, pkg, _) = self.selected_template()?;
                let name = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: patch from-git <name>.patch <file>"))?;
                let file = tokens
                    .get(3)
                    .ok_or_else(|| invalid("usage: patch from-git <name>.patch <file>"))?;
                let path = patches::from_git_diff(&ws, &pkg.name, name, file)?;
                self.refresh_all();
                Ok(format!("patch written: {}", path.display()))
            }
            Some("apply") | Some("revert") => {
                let reverse = tokens[1] == "revert";
                let patch_name = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: patch apply <name>"))?;
                let ws = self.workspace_mut()?;
                let patch = patches::patch_dir(
                    &ws,
                    &self
                        .selected_package()
                        .ok_or_else(|| invalid("no package"))?
                        .name,
                )
                .join(patch_name);
                if !patch.is_file() {
                    return Err(invalid("patch not found"));
                }
                self.confirm(
                    if reverse {
                        "Revert patch?"
                    } else {
                        "Apply patch?"
                    },
                    "Backups of affected files will be created before modification.",
                    Action::ApplyPatch { patch, reverse },
                );
                Ok("confirmation required".into())
            }
            Some("remove") => {
                let patch_name = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: patch remove <name>"))?;
                let ws = self.workspace_mut()?;
                let patch = patches::patch_dir(
                    &ws,
                    &self
                        .selected_package()
                        .ok_or_else(|| invalid("no package"))?
                        .name,
                )
                .join(patch_name);
                self.confirm(
                    "Remove patch?",
                    "The patch file will be deleted from srcpkgs.",
                    Action::RemovePatch(patch),
                );
                Ok("confirmation required".into())
            }
            _ => Err(invalid(
                "usage: patch list|new|from-git|apply|revert|remove",
            )),
        }
    }

    fn command_build(&mut self, tokens: &[String]) -> Result<String> {
        match tokens.get(1).map(String::as_str) {
            Some("cancel") => {
                let id = self
                    .active_build_id
                    .ok_or_else(|| invalid("no active build"))?;
                self.builds_manager.cancel(id)?;
                Ok("termination signal sent to active build".into())
            }
            Some("logs") => {
                let index: usize = tokens
                    .get(2)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let build = self
                    .builds
                    .get(index)
                    .cloned()
                    .ok_or_else(|| invalid("build record not found"))?;
                let path = build
                    .log_path
                    .ok_or_else(|| invalid("this build has no log"))?;
                self.build_logs = std::fs::read_to_string(path)?
                    .lines()
                    .rev()
                    .take(400)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                self.screen = Screen::BuildCenter;
                Ok(format!("loaded {} log lines", self.build_logs.len()))
            }
            Some(op) => {
                let build_op = BuildOp::parse(op).ok_or_else(|| invalid("unknown build phase"))?;
                let package_name = tokens
                    .get(2)
                    .map(String::as_str)
                    .map(str::to_string)
                    .or_else(|| self.selected_package().map(|item| item.name))
                    .ok_or_else(|| invalid("no package selected"))?;
                if build_op == BuildOp::Clean {
                    self.confirm(
                        "Clean package build directory?",
                        format!("xbps-src clean {package_name} removes build artifacts."),
                        Action::CleanBuild(package_name),
                    );
                    return Ok("confirmation required".into());
                }
                self.start_build(&package_name, build_op)?;
                Ok("build started".into())
            }
            None => {
                self.screen = Screen::BuildCenter;
                Ok("build center opened".into())
            }
        }
    }

    fn command_source(&mut self, tokens: &[String]) -> Result<String> {
        match tokens.get(1).map(String::as_str) {
            Some("verify") => {
                let ws = self.workspace_mut()?;
                let (_, _, info) = self.selected_template()?;
                let results = source::verify(&ws.path, &info);
                let summary = results
                    .iter()
                    .map(|(item, actual, matches)| {
                        format!(
                            "{}: {} ({})",
                            item.filename,
                            if *matches { "verified" } else { "mismatch" },
                            actual.as_deref().unwrap_or("missing")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                if summary.is_empty() {
                    Ok("no distfiles declared".into())
                } else {
                    Ok(summary)
                }
            }
            Some("download") => {
                let ws = self.workspace_mut()?;
                let (_, _, info) = self.selected_template()?;
                let index: usize = tokens
                    .get(2)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let path = source::download(&ws.path, &info, index)?;
                Ok(format!("downloaded {}", path.display()))
            }
            Some("extract") => {
                let ws = self.workspace_mut()?;
                let (_, _, info) = self.selected_template()?;
                let filename = tokens
                    .get(2)
                    .map(String::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        source::distfiles(&info)
                            .first()
                            .map(|item| item.filename.clone())
                    })
                    .ok_or_else(|| invalid("no archive specified"))?;
                let path = source::extract(&ws.path, &info, &filename)?;
                Ok(format!("extracted into {}", path.display()))
            }
            Some("github") => {
                let url = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: source github <url>"))?;
                let tag = source::github_latest(url)?;
                Ok(format!("latest upstream tag: {tag}"))
            }
            _ => Err(invalid("usage: source verify|download|extract|github")),
        }
    }

    fn command_diagnose(&mut self, tokens: &[String]) -> Result<String> {
        let record = match tokens.get(1).map(String::as_str) {
            Some("last") | None => self
                .builds
                .first()
                .cloned()
                .ok_or_else(|| invalid("no build history"))?,
            Some(id) => {
                let id: i64 = id
                    .parse()
                    .map_err(|_| invalid("build id must be numeric"))?;
                self.builds
                    .iter()
                    .find(|item| item.id == id)
                    .cloned()
                    .ok_or_else(|| invalid("build not found"))?
            }
        };
        let path = record
            .log_path
            .map(PathBuf::from)
            .ok_or_else(|| invalid("this build has no stored log"))?;
        self.findings = diagnostics::analyze(&path)?;
        self.diagnostic_index = 0;
        self.screen = Screen::Diagnostics;
        Ok(format!(
            "analyzed {} finding(s) from #{}",
            self.findings.len(),
            record.id
        ))
    }

    fn command_git(&mut self, tokens: &[String]) -> Result<String> {
        let ws = self.workspace_mut()?;
        let remote = tokens.get(2).cloned().unwrap_or_else(|| {
            ws.remotes
                .first()
                .cloned()
                .unwrap_or_else(|| "origin".into())
        });
        match tokens.get(1).map(String::as_str) {
            Some("status") => {
                self.git = git::snapshot(&ws.path)?;
                self.screen = Screen::Git;
                Ok(format!("{} changed path(s)", self.git.changes.len()))
            }
            Some("diff") => {
                self.git.diff = git::diff(&ws.path, None)?;
                self.screen = Screen::Git;
                Ok(format!("diff: {} bytes", self.git.diff.len()))
            }
            Some("stage") => {
                let path = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: git stage <path>"))?;
                git::stage(&ws.path, path)?;
                self.refresh_all();
                Ok(format!("staged {path}"))
            }
            Some("unstage") => {
                let path = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: git unstage <path>"))?;
                git::unstage(&ws.path, path)?;
                self.refresh_all();
                Ok(format!("unstaged {path}"))
            }
            Some("commit") => {
                let message = tokens[2..].join(" ");
                let output = git::commit(&ws.path, &message)?;
                db::record_git(
                    self.database.conn(),
                    Some(ws.id),
                    "commit",
                    &message,
                    "success",
                )?;
                self.refresh_all();
                Ok(output
                    .lines()
                    .next()
                    .unwrap_or("commit created")
                    .to_string())
            }
            Some("branch") => Ok(git::branches(&ws.path)?.join(" | ")),
            Some("checkout") => {
                let branch = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: git checkout <branch>"))?;
                git::checkout(&ws.path, branch)?;
                self.refresh_all();
                Ok(format!("switched to {branch}"))
            }
            Some("branch-new") => {
                let branch = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: git branch-new <name>"))?;
                git::create_branch(&ws.path, branch)?;
                Ok(format!("created branch {branch}"))
            }
            Some("fetch") => {
                let output = git::fetch(&ws.path, &remote)?;
                Ok(format!(
                    "fetched {remote}: {}",
                    output.lines().next().unwrap_or("ok")
                ))
            }
            Some("pull") => {
                let branch = tokens
                    .get(3)
                    .cloned()
                    .unwrap_or_else(|| self.git.branch.clone());
                self.confirm(
                    "Pull from remote?",
                    format!("Fast-forward pull from {remote}/{branch}"),
                    Action::GitPull { remote, branch },
                );
                Ok("confirmation required".into())
            }
            Some("push") => {
                let branch = tokens
                    .get(3)
                    .cloned()
                    .unwrap_or_else(|| self.git.branch.clone());
                self.confirm(
                    "Push to remote?",
                    format!("This will update {remote}/{branch} externally."),
                    Action::GitPush { remote, branch },
                );
                Ok("confirmation required".into())
            }
            Some("tag") => Ok(git::tags(&ws.path)?.join(" | ")),
            _ => Err(invalid(
                "usage: git status|diff|stage|unstage|commit|branch|checkout|fetch|pull|push|tag",
            )),
        }
    }

    fn command_release(&mut self, tokens: &[String]) -> Result<String> {
        match tokens.get(1).map(String::as_str) {
            Some("list") => {
                self.screen = Screen::BuildCenter;
                Ok(format!(
                    "{} package(s) in hostdir/binpkgs",
                    self.outputs.len()
                ))
            }
            Some("index") => {
                self.confirm(
                    "Index local repository?",
                    "xbps-rindex -a will update the repository index in hostdir/binpkgs.",
                    Action::IndexRepository,
                );
                Ok("confirmation required".into())
            }
            Some("info") => {
                let ws = self.workspace_mut()?;
                let name = tokens
                    .get(2)
                    .ok_or_else(|| invalid("usage: release info <name>"))?;
                let info = release::metadata(&release::output_dir(&ws), name)?;
                Ok(info)
            }
            _ => Err(invalid("usage: release list|index|info")),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> KeyOutcome {
        if self.confirm.is_some() {
            match key.code {
                KeyCode::Enter => {
                    let confirm = self.confirm.take().unwrap();
                    match self.apply_action(confirm.action) {
                        Ok(message) => self.notify(NoticeLevel::Success, message),
                        Err(error) => self.notify(NoticeLevel::Error, error.to_string()),
                    }
                }
                KeyCode::Esc => self.confirm = None,
                _ => {}
            }
            return KeyOutcome::Continue;
        }

        if self.command_mode {
            match key.code {
                KeyCode::Char(c) => self.command.push(c),
                KeyCode::Backspace => {
                    self.command.pop();
                }
                KeyCode::Enter => {
                    let command = self.command.clone();
                    self.command_mode = false;
                    self.command.clear();
                    match self.execute_command(&command) {
                        Ok(message) if !message.is_empty() => {
                            self.notify(NoticeLevel::Info, message)
                        }
                        Ok(_) => {}
                        Err(error) => self.notify(NoticeLevel::Error, error.to_string()),
                    }
                }
                KeyCode::Esc => {
                    self.command_mode = false;
                    self.command.clear();
                }
                _ => {}
            }
            return KeyOutcome::Continue;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if key.code == KeyCode::Char('c') {
                self.request_quit();
            }
            return KeyOutcome::Continue;
        }

        match key.code {
            KeyCode::Tab => {
                let index = self.screen as usize;
                let next = Screen::ALL[(index + 1) % Screen::ALL.len()];
                self.switch_screen(next);
            }
            KeyCode::BackTab => {
                let index = self.screen as usize;
                let next = Screen::ALL[(index + Screen::ALL.len() - 1) % Screen::ALL.len()];
                self.switch_screen(next);
            }
            KeyCode::Char('1') => self.switch_screen(Screen::Dashboard),
            KeyCode::Char('2') => self.switch_screen(Screen::Workspaces),
            KeyCode::Char('3') => self.switch_screen(Screen::Packages),
            KeyCode::Char('4') => self.switch_screen(Screen::Template),
            KeyCode::Char('5') => self.switch_screen(Screen::Dependencies),
            KeyCode::Char('6') => self.switch_screen(Screen::Patches),
            KeyCode::Char('7') => self.switch_screen(Screen::BuildCenter),
            KeyCode::Char('8') => self.switch_screen(Screen::Diagnostics),
            KeyCode::Char('9') => self.switch_screen(Screen::Git),
            KeyCode::Char('0') => self.switch_screen(Screen::Settings),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Home => self.move_selection(isize::MIN / 4),
            KeyCode::End => self.move_selection(isize::MAX / 4),
            KeyCode::Enter => self.activate(),
            KeyCode::Char(':') => {
                self.command_mode = true;
                self.command = ":".into();
            }
            KeyCode::Char('/') => {
                self.command_mode = true;
                self.command = ":filter ".into();
            }
            KeyCode::Char('?') => self.help = !self.help,
            KeyCode::Esc => {
                self.help = false;
                self.message = None;
            }
            KeyCode::Char('r') => {
                self.refresh_all();
                self.notify(NoticeLevel::Info, "workspace data refreshed");
            }
            KeyCode::Char('e') => {
                if self.screen == Screen::Template || self.screen == Screen::Packages {
                    match self.selected_template() {
                        Ok((_, pkg, _)) => {
                            let path = pkg.path.join("template");
                            return KeyOutcome::Edit(path);
                        }
                        Err(error) => self.notify(NoticeLevel::Error, error.to_string()),
                    }
                }
            }
            KeyCode::Char('v') => match self.execute_command("template validate") {
                Ok(result) => self.notify(NoticeLevel::Info, result),
                Err(error) => self.notify(NoticeLevel::Error, error.to_string()),
            },
            KeyCode::Char('b') => {
                let name = self.selected_package().map(|item| item.name);
                if let Some(name) = name {
                    let op = self.selected_op();
                    match self.start_build(&name, op) {
                        Ok(()) => {}
                        Err(error) => self.notify(NoticeLevel::Error, error.to_string()),
                    }
                }
            }
            KeyCode::Char('x') => {
                if let Some(id) = self.active_build_id {
                    match self.builds_manager.cancel(id) {
                        Ok(()) => self.notify(NoticeLevel::Warning, "build termination requested"),
                        Err(error) => self.notify(NoticeLevel::Error, error.to_string()),
                    }
                }
            }
            KeyCode::Char('t') => {
                if self.screen == Screen::Settings {
                    let next = match self.settings.theme {
                        ThemeName::Dark => ThemeName::Light,
                        ThemeName::Light => ThemeName::Dark,
                    };
                    self.settings.theme = next;
                    let _ = self.settings.save(self.database.conn());
                }
            }
            KeyCode::Char('q') => self.request_quit(),
            _ => {}
        }
        KeyOutcome::Continue
    }

    fn move_selection(&mut self, delta: isize) {
        let len = match self.screen {
            Screen::Workspaces => self.workspaces.len(),
            Screen::Packages => self.filtered_packages().len(),
            Screen::Patches => self.patches.len(),
            Screen::BuildCenter => BuildOp::all().len(),
            Screen::Diagnostics => self.findings.len(),
            Screen::Git => self.git.changes.len(),
            _ => 0,
        };
        if len == 0 {
            return;
        }
        let current = match self.screen {
            Screen::Workspaces => self.active_ws.unwrap_or(0),
            Screen::Packages => self.package_index,
            Screen::Patches => self.patch_index,
            Screen::BuildCenter => self.build_op_index,
            Screen::Diagnostics => self.diagnostic_index,
            Screen::Git => self.git_index,
            _ => 0,
        } as isize;
        let next = (current + delta).clamp(0, len as isize - 1) as usize;
        if next == current as usize {
            return;
        }
        match self.screen {
            Screen::Workspaces => {
                self.active_ws = Some(next);
                self.package_index = 0;
                // Switching repository is the only case that needs a deep
                // rescan of templates, git state and built packages.
                self.rescan_workspace_data();
                self.refresh_selection();
            }
            Screen::Packages => {
                self.package_index = next;
                self.refresh_selection();
            }
            Screen::Patches => self.patch_index = next,
            Screen::BuildCenter => self.build_op_index = next,
            Screen::Diagnostics => self.diagnostic_index = next,
            Screen::Git => self.git_index = next,
            _ => {}
        }
    }

    fn activate(&mut self) {
        match self.screen {
            Screen::Packages => self.switch_screen(Screen::Template),
            Screen::BuildCenter => {
                let op = self.selected_op();
                if let Some(name) = self.selected_package().map(|item| item.name) {
                    if op == BuildOp::Clean {
                        self.confirm(
                            "Clean package build directory?",
                            format!("xbps-src clean {name} removes build artifacts."),
                            Action::CleanBuild(name),
                        );
                    } else if let Err(error) = self.start_build(&name, op) {
                        self.notify(NoticeLevel::Error, error.to_string());
                    }
                }
            }
            Screen::Diagnostics => {
                if let Some(finding) = self.findings.get(self.diagnostic_index) {
                    self.notify(
                        NoticeLevel::Info,
                        finding
                            .evidence
                            .clone()
                            .unwrap_or_else(|| finding.detail.clone()),
                    );
                }
            }
            _ => {}
        }
    }
}

fn screen_from_name(name: &str) -> Option<Screen> {
    match name.to_ascii_lowercase().as_str() {
        "dashboard" | "dash" => Some(Screen::Dashboard),
        "workspaces" | "workspace" => Some(Screen::Workspaces),
        "packages" | "pkgs" => Some(Screen::Packages),
        "template" => Some(Screen::Template),
        "dependencies" | "deps" => Some(Screen::Dependencies),
        "patches" | "patch" => Some(Screen::Patches),
        "build" | "build-center" => Some(Screen::BuildCenter),
        "diagnostics" | "diag" => Some(Screen::Diagnostics),
        "git" => Some(Screen::Git),
        "settings" => Some(Screen::Settings),
        _ => None,
    }
}

fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in input.chars() {
        match quote {
            Some(expected) => {
                if ch == expected {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            None => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

pub fn open_editor(editor: &str, path: &PathBuf) -> Result<()> {
    let tokens = tokenize(editor);
    let program = tokens
        .first()
        .ok_or_else(|| invalid("empty editor command"))?;
    let status = Command::new(program)
        .args(&tokens[1..])
        .arg(path)
        .status()?;
    if !status.success() {
        return Err(invalid("editor exited with an error"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn tokenizes_quoted_commands() {
        let tokens = tokenize("git commit \"fix broken build\"");
        assert_eq!(tokens, vec!["git", "commit", "fix broken build"]);
    }

    #[test]
    fn screens_have_labels() {
        assert_eq!(Screen::BuildCenter.label(), "Build Center");
        assert_eq!(Screen::ALL.len(), 10);
    }

    #[test]
    fn error_type_formats() {
        let error = Error::Invalid("bad".into());
        assert!(error.to_string().contains("bad"));
    }
}
