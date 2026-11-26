use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Screen};
use crate::events::NoticeLevel;
use crate::models::{BuildOp, Severity};
use crate::tui::theme::Theme;

pub fn render(frame: &mut Frame, app: &App) {
    let theme = Theme::new(app.settings.theme);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.bg).fg(theme.text)),
        frame.area(),
    );

    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(frame.area());
    render_header(frame, chunks[0], app, theme);
    let body = Layout::horizontal([Constraint::Length(23), Constraint::Min(0)]).split(chunks[1]);
    render_sidebar(frame, body[0], app, theme);
    render_screen(frame, body[1], app, theme);
    render_footer(frame, chunks[2], app, theme);

    if app.help {
        render_help(frame, frame.area(), theme);
    }
    if let Some(confirm) = &app.confirm {
        render_confirm(frame, frame.area(), confirm, theme);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let workspace = app
        .active_workspace()
        .map(|item| truncate(&item.name, 24))
        .unwrap_or_else(|| "no workspace".into());
    let build = app
        .active_build_id
        .map(|id| format!("build #{id}"))
        .unwrap_or_else(|| "idle".into());
    let title = format!(
        " VoidForge — {} — {} — {} ",
        app.screen.label(),
        workspace,
        build
    );
    let widget = Paragraph::new(Line::from(Span::styled(
        title,
        Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD),
    )))
    .style(Style::default().bg(theme.surface));
    frame.render_widget(widget, area);
}

fn render_sidebar(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let mut items = Vec::new();
    for (index, screen) in Screen::ALL.iter().enumerate() {
        let key = if index == 9 {
            "0"
        } else {
            &(index + 1).to_string()
        };
        let selected = *screen == app.screen;
        let style = if selected {
            Style::default()
                .bg(theme.surface_alt)
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!(" {key} "), Style::default().fg(theme.accent)),
            Span::styled(screen.label(), style),
        ])));
    }
    let widget = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" VoidForge ")
            .style(Style::default().bg(theme.surface).fg(theme.text))
            .border_style(Style::default().fg(theme.border)),
    );
    frame.render_widget(widget, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let (color, message) = match &app.message {
        Some((NoticeLevel::Error, text)) => (theme.error, text.clone()),
        Some((NoticeLevel::Warning, text)) => (theme.warning, text.clone()),
        Some((NoticeLevel::Success, text)) => (theme.success, text.clone()),
        Some((NoticeLevel::Info, text)) => (theme.primary, text.clone()),
        None => (theme.muted, "Ready".into()),
    };
    let input = if app.command_mode {
        format!("{}_", app.command)
    } else {
        ": help   / filter   ? keys   q quit   Tab screens".into()
    };
    let widget = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(message, Style::default().fg(color))),
        Line::from(Span::styled(input, Style::default().fg(theme.muted))),
    ]))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .style(Style::default().bg(theme.surface))
            .border_style(Style::default().fg(theme.border)),
    );
    frame.render_widget(widget, area);
}

fn render_screen(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    match app.screen {
        Screen::Dashboard => render_dashboard(frame, area, app, theme),
        Screen::Workspaces => render_workspaces(frame, area, app, theme),
        Screen::Packages => render_packages(frame, area, app, theme),
        Screen::Template => render_template(frame, area, app, theme),
        Screen::Dependencies => render_dependencies(frame, area, app, theme),
        Screen::Patches => render_patches(frame, area, app, theme),
        Screen::BuildCenter => render_build_center(frame, area, app, theme),
        Screen::Diagnostics => render_diagnostics(frame, area, app, theme),
        Screen::Git => render_git(frame, area, app, theme),
        Screen::Settings => render_settings(frame, area, app, theme),
    }
}

fn render_dashboard(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let chunks = Layout::vertical([Constraint::Length(11), Constraint::Min(0)]).split(area);
    let cards = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    let host_lines = vec![
        row("OS", app.host.os_pretty.clone(), theme),
        row("Kernel", app.host.kernel.clone(), theme),
        row(
            "Arch / libc",
            format!("{} / {}", app.host.arch, app.host.libc),
            theme,
        ),
        row(
            "Support",
            if app.host.supported {
                "Void Linux native".into()
            } else {
                app.host.reason.clone().unwrap_or_else(|| "unknown".into())
            },
            theme,
        ),
        row(
            "Workspace",
            app.active_workspace()
                .map(|item| item.path.display().to_string())
                .unwrap_or_else(|| "none".into()),
            theme,
        ),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(host_lines)).block(panel(" Host ", theme)),
        cards[0],
    );

    let memory = ratio_u64(app.metrics.memory_used, app.metrics.memory_total);
    let swap = ratio_u64(app.metrics.swap_used, app.metrics.swap_total);
    let metrics = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
    ])
    .split(cards[1]);
    frame.render_widget(
        Gauge::default()
            .block(panel(" CPU ", theme))
            .gauge_style(Style::default().fg(theme.primary).bg(theme.surface_alt))
            .ratio(app.metrics.cpu_percent.clamp(0.0, 100.0) as f64 / 100.0)
            .label(format!("{:.1}%", app.metrics.cpu_percent)),
        metrics[0],
    );
    frame.render_widget(
        Gauge::default()
            .block(panel(" Memory ", theme))
            .gauge_style(Style::default().fg(theme.accent).bg(theme.surface_alt))
            .ratio(memory)
            .label(format!(
                "{} / {}",
                human_bytes(app.metrics.memory_used),
                human_bytes(app.metrics.memory_total)
            )),
        metrics[1],
    );
    frame.render_widget(
        Gauge::default()
            .block(panel(" Swap ", theme))
            .gauge_style(Style::default().fg(theme.warning).bg(theme.surface_alt))
            .ratio(swap)
            .label(format!(
                "{} / {}",
                human_bytes(app.metrics.swap_used),
                human_bytes(app.metrics.swap_total)
            )),
        metrics[2],
    );

    let lower = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);
    let mut tools = Vec::new();
    for tool in &app.tools {
        let mark = if tool.found { "ok" } else { "--" };
        let version = tool.version.clone().unwrap_or_default();
        tools.push(ListItem::new(Line::from(Span::styled(
            format!(" {mark} {:<16} {}", tool.name, truncate(&version, 34)),
            Style::default().fg(if tool.found {
                theme.success
            } else {
                theme.muted
            }),
        ))));
    }
    frame.render_widget(
        List::new(tools).block(panel(" Toolchain ", theme)),
        lower[0],
    );

    let mut activity = Vec::new();
    for build in app.builds.iter().take(20) {
        activity.push(ListItem::new(Line::from(Span::styled(
            format!(
                "#{} {:<16} {:<10} {:<10} {}",
                build.id,
                truncate(&build.package, 16),
                build.operation,
                build.status,
                build.started_at
            ),
            Style::default().fg(status_color(&build.status, theme)),
        ))));
    }
    if activity.is_empty() {
        activity.push(ListItem::new(Line::from(Span::styled(
            " No builds recorded yet. Press 7 then Enter to build the selected package.",
            Style::default().fg(theme.muted),
        ))));
    }
    frame.render_widget(
        List::new(activity).block(panel(" Recent Activity ", theme)),
        lower[1],
    );
}

fn render_workspaces(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let chunks = Layout::horizontal([Constraint::Percentage(45), Constraint::Min(0)]).split(area);
    let items: Vec<ListItem> = app
        .workspaces
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let selected = Some(index) == app.active_ws;
            let style = if selected {
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(
                format!(
                    " {} {} [{}] {}",
                    if item.valid { "ok" } else { "!!" },
                    item.name,
                    item.branch,
                    item.path.display()
                ),
                style,
            )))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(panel(" Registered void-packages ", theme)),
        chunks[0],
    );

    let detail = match app.active_workspace() {
        Some(ws) => vec![
            row("Name", ws.name.clone(), theme),
            row("Path", ws.path.display().to_string(), theme),
            row("Branch", ws.branch.clone(), theme),
            row("Remotes", ws.remotes.join(", "), theme),
            row("Valid", ws.valid.to_string(), theme),
            row("Created", ws.created_at.clone(), theme),
            row(
                "Commands",
                ":workspace add|clone|use|validate|refresh|remove",
                theme,
            ),
        ],
        None => vec![row(
            "Next",
            "Register a local void-packages tree with :workspace add <path>",
            theme,
        )],
    };
    frame.render_widget(
        Paragraph::new(Text::from(detail))
            .wrap(Wrap { trim: false })
            .block(panel(" Details ", theme)),
        chunks[1],
    );
}

fn render_packages(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let chunks = Layout::horizontal([Constraint::Percentage(45), Constraint::Min(0)]).split(area);
    let packages = app.filtered_packages();
    let items: Vec<ListItem> = packages
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let style = if index == app.package_index {
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            let version = item.version.clone().unwrap_or_else(|| "unknown".into());
            ListItem::new(Line::from(Span::styled(
                format!(
                    " {:<28} {:<14} {}",
                    truncate(&item.name, 28),
                    truncate(&version, 14),
                    item.description.as_deref().unwrap_or("")
                ),
                style,
            )))
        })
        .collect();
    let title = format!(" srcpkgs ({}) — filter: {} ", packages.len(), app.filter);
    frame.render_widget(List::new(items).block(panel(&title, theme)), chunks[0]);

    let detail = match app.selected_package() {
        Some(pkg) => vec![
            row("Name", pkg.name, theme),
            row(
                "Version",
                pkg.version.unwrap_or_else(|| "unknown".into()),
                theme,
            ),
            row(
                "Build style",
                pkg.build_style.unwrap_or_else(|| "unknown".into()),
                theme,
            ),
            row("Path", pkg.path.display().to_string(), theme),
            row(
                "Keys",
                "Enter template · e edit · v validate · b build",
                theme,
            ),
            row("Commands", ":pkg new|duplicate|rename|open", theme),
        ],
        None => vec![row(
            "Next",
            "Add a workspace or create a package with :pkg new <name>",
            theme,
        )],
    };
    frame.render_widget(
        Paragraph::new(Text::from(detail))
            .wrap(Wrap { trim: false })
            .block(panel(" Selected package ", theme)),
        chunks[1],
    );
}

fn render_template(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let chunks = Layout::horizontal([Constraint::Percentage(42), Constraint::Min(0)]).split(area);
    let mut vars: Vec<(String, String)> = app
        .template
        .as_ref()
        .map(|info| {
            info.vars
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default();
    vars.sort_by(|left, right| left.0.cmp(&right.0));
    let items: Vec<ListItem> = vars
        .into_iter()
        .map(|(key, value)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {key:<20}"), Style::default().fg(theme.accent)),
                Span::styled(truncate(&value, 60), Style::default().fg(theme.text)),
            ]))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(panel(" Parsed variables ", theme)),
        chunks[0],
    );

    let right = Layout::vertical([Constraint::Min(0), Constraint::Length(8)]).split(chunks[1]);
    let raw = app
        .template
        .as_ref()
        .map(|info| info.raw.clone())
        .unwrap_or_else(|| "No template selected.".into());
    frame.render_widget(
        Paragraph::new(raw)
            .wrap(Wrap { trim: false })
            .block(panel(" template (read-only) ", theme)),
        right[0],
    );
    let findings = app
        .findings
        .iter()
        .take(10)
        .map(|item| {
            ListItem::new(Line::from(Span::styled(
                format!(" {} {}", severity_mark(item.severity), item.title),
                Style::default().fg(severity_color(item.severity, theme)),
            )))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(findings).block(panel(" Validation ", theme)),
        right[1],
    );
}

fn render_dependencies(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let items: Vec<ListItem> = app
        .dependencies
        .iter()
        .map(|dep| {
            let repo = if dep.in_repository { "repo" } else { "----" };
            let installed = if dep.installed { "host" } else { "----" };
            ListItem::new(Line::from(Span::styled(
                format!(
                    " {:<14} {:<4} {:<4} {}",
                    dep.kind.as_str(),
                    repo,
                    installed,
                    dep.name
                ),
                Style::default().fg(if dep.in_repository {
                    theme.text
                } else {
                    theme.warning
                }),
            )))
        })
        .collect();
    frame.render_widget(List::new(items).block(panel(" Dependencies ", theme)), area);
}

fn render_patches(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let chunks = Layout::horizontal([Constraint::Percentage(42), Constraint::Min(0)]).split(area);
    let items: Vec<ListItem> = app
        .patches
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let style = if index == app.patch_index {
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(path.display().to_string(), style)))
        })
        .collect();
    frame.render_widget(List::new(items).block(panel(" Patches ", theme)), chunks[0]);

    let preview = app
        .patches
        .get(app.patch_index)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_else(|| "No patch selected.".into());
    frame.render_widget(
        Paragraph::new(preview)
            .wrap(Wrap { trim: false })
            .block(panel(" Patch preview ", theme)),
        chunks[1],
    );
}

fn render_build_center(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let chunks = Layout::horizontal([Constraint::Length(22), Constraint::Min(0)]).split(area);
    let ops: Vec<ListItem> = BuildOp::all()
        .iter()
        .enumerate()
        .map(|(index, op)| {
            let style = if index == app.build_op_index {
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.muted)
            };
            ListItem::new(Line::from(Span::styled(format!(" {}", op.as_str()), style)))
        })
        .collect();
    frame.render_widget(
        List::new(ops).block(panel(" xbps-src phases ", theme)),
        chunks[0],
    );

    let right = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(8),
        Constraint::Length(6),
    ])
    .split(chunks[1]);
    let lines = &app.build_logs;
    let take = lines.len().min(right[0].height.saturating_sub(2) as usize);
    let start = lines.len() - take;
    let logs = if take == 0 {
        "No live log. Select a package and phase, then press Enter or b.".into()
    } else {
        lines[start..].join("\n")
    };
    frame.render_widget(
        Paragraph::new(logs)
            .wrap(Wrap { trim: false })
            .block(panel(&format!(" Logs — {} lines ", lines.len()), theme)),
        right[0],
    );

    let history: Vec<ListItem> = app
        .builds
        .iter()
        .take(12)
        .map(|build| {
            ListItem::new(Line::from(Span::styled(
                format!(
                    "#{} {:<16} {:<10} {:<10} {:>5}s",
                    build.id,
                    truncate(&build.package, 16),
                    build.operation,
                    build.status,
                    build.duration_seconds.unwrap_or(0)
                ),
                Style::default().fg(status_color(&build.status, theme)),
            )))
        })
        .collect();
    frame.render_widget(
        List::new(history).block(panel(" History ", theme)),
        right[1],
    );

    let outputs: Vec<ListItem> = app
        .outputs
        .iter()
        .take(20)
        .map(|item| {
            ListItem::new(Line::from(Span::styled(
                format!(
                    " {:<24} {:<14} {}",
                    truncate(&item.name, 24),
                    truncate(&item.version, 14),
                    human_bytes(item.size)
                ),
                Style::default().fg(theme.success),
            )))
        })
        .collect();
    let outputs = if outputs.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            " No .xbps files in hostdir/binpkgs yet — run the pkg phase first.",
            Style::default().fg(theme.muted),
        )))]
    } else {
        outputs
    };
    frame.render_widget(
        List::new(outputs).block(panel(" Built packages (hostdir/binpkgs) ", theme)),
        right[2],
    );
}

fn render_diagnostics(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let chunks = Layout::horizontal([Constraint::Percentage(44), Constraint::Min(0)]).split(area);
    let items: Vec<ListItem> = app
        .findings
        .iter()
        .enumerate()
        .map(|(index, finding)| {
            let style = if index == app.diagnostic_index {
                Style::default()
                    .fg(severity_color(finding.severity, theme))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(severity_color(finding.severity, theme))
            };
            ListItem::new(Line::from(Span::styled(
                format!(" {} {}", severity_mark(finding.severity), finding.title),
                style,
            )))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(panel(" Evidence-based diagnostics ", theme)),
        chunks[0],
    );

    let detail = match app.findings.get(app.diagnostic_index) {
        Some(finding) => vec![
            row("Severity", format!("{:?}", finding.severity), theme),
            row("Detail", finding.detail.clone(), theme),
            row(
                "Evidence",
                finding.evidence.clone().unwrap_or_else(|| "none".into()),
                theme,
            ),
            row(
                "Line",
                finding
                    .line
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "n/a".into()),
                theme,
            ),
            row("Verified", finding.verified.to_string(), theme),
        ],
        None => vec![row(
            "Next",
            ":template validate, :workspace validate or :diagnose last",
            theme,
        )],
    };
    frame.render_widget(
        Paragraph::new(Text::from(detail))
            .wrap(Wrap { trim: false })
            .block(panel(" Finding ", theme)),
        chunks[1],
    );
}

fn render_git(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let chunks = Layout::horizontal([Constraint::Percentage(42), Constraint::Min(0)]).split(area);
    let items: Vec<ListItem> = app
        .git
        .changes
        .iter()
        .enumerate()
        .map(|(index, change)| {
            let style = if index == app.git_index {
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(
                format!(" {:<4} {}", change.status, change.path),
                style,
            )))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(panel(" Working tree ", theme)),
        chunks[0],
    );

    let right = Layout::vertical([Constraint::Length(8), Constraint::Min(0)]).split(chunks[1]);
    let summary = vec![
        row("Branch", app.git.branch.clone(), theme),
        row("Remotes", app.git.remotes.join(", "), theme),
        row("Changes", app.git.changes.len().to_string(), theme),
        row(
            "Recent",
            app.git
                .log
                .first()
                .cloned()
                .unwrap_or_else(|| "none".into()),
            theme,
        ),
        row(
            "Commands",
            ":git status|diff|stage|commit|fetch|pull|push",
            theme,
        ),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(summary))
            .wrap(Wrap { trim: false })
            .block(panel(" Repository ", theme)),
        right[0],
    );
    let diff = if app.git.diff.is_empty() {
        "No diff loaded. Run :git diff.".into()
    } else {
        app.git.diff.clone()
    };
    frame.render_widget(
        Paragraph::new(diff)
            .wrap(Wrap { trim: false })
            .block(panel(" Diff ", theme)),
        right[1],
    );
}

fn render_settings(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let rows = vec![
        row("Theme", app.settings.theme.as_str(), theme),
        row("Jobs", app.settings.jobs.to_string(), theme),
        row("Editor", app.settings.editor.clone(), theme),
        row(
            "Confirm danger",
            app.settings.confirm_dangerous.to_string(),
            theme,
        ),
        row(
            "Check installed",
            app.settings.check_installed.to_string(),
            theme,
        ),
        row(
            "Data directory",
            crate::utils::path::data_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "unknown".into()),
            theme,
        ),
        row(
            "Commands",
            ":theme dark|light · :jobs <n> · :editor <cmd>",
            theme,
        ),
        row("Theme key", "t toggles dark/light while on Settings", theme),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(rows))
            .wrap(Wrap { trim: false })
            .block(panel(" Settings ", theme)),
        area,
    );
}

fn render_help(frame: &mut Frame, area: Rect, theme: Theme) {
    let popup = centered(area, 70, 70);
    frame.render_widget(Clear, popup);
    let lines = vec![
        heading(" VoidForge keys ", theme),
        row(
            "Navigation",
            "1-0 screens · Tab/Shift+Tab next screen · j/k or arrows · Enter activate",
            theme,
        ),
        row(
            "Actions",
            "e edit template · v validate · b build selected · x cancel build · r refresh",
            theme,
        ),
        row(
            "Command",
            ": opens command line · / package filter · Esc closes overlay",
            theme,
        ),
        row(
            "Quit",
            "q or Ctrl+C; active builds require confirmation",
            theme,
        ),
        heading(" Real workflow ", theme),
        row(
            "2",
            ":workspace add <void-packages path> or :workspace clone <url>",
            theme,
        ),
        row("3", ":pkg new <name>, then e to edit template", theme),
        row(
            "8",
            ":template validate · :deps cycles · :diagnose last",
            theme,
        ),
        row(
            "7",
            "Build Center: choose xbps-src phase and press Enter",
            theme,
        ),
        row(
            "6",
            "Patches: :patch new <name> to edit a package patch",
            theme,
        ),
        heading(" Commands ", theme),
        row("Help", ":help shows this overlay", theme),
        row(
            "Workspace",
            ":workspace add|clone|use|remove|validate|refresh",
            theme,
        ),
        row("Packages", ":pkg list|new|duplicate|rename|open", theme),
        row(
            "Build",
            ":build <phase> [pkg] · :build cancel · :build logs <index>",
            theme,
        ),
        row(
            "Source",
            ":source verify|download|extract|github <url>",
            theme,
        ),
        row(
            "Git",
            ":git status|diff|stage|unstage|commit|checkout|fetch|pull|push",
            theme,
        ),
        row("Release", ":release list|index|info <name>", theme),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .style(Style::default().bg(theme.surface).fg(theme.text))
                    .border_style(Style::default().fg(theme.primary)),
            ),
        popup,
    );
}

fn render_confirm(frame: &mut Frame, area: Rect, app: &crate::app::Confirm, theme: Theme) {
    let popup = centered(area, 55, 30);
    frame.render_widget(Clear, popup);
    let lines = vec![
        heading(&app.title, theme),
        row("Detail", app.detail.clone(), theme),
        row("Confirm", "Enter", theme),
        row("Cancel", "Esc", theme),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Confirmation ")
                    .style(Style::default().bg(theme.surface).fg(theme.text))
                    .border_style(Style::default().fg(theme.warning)),
            ),
        popup,
    );
}

fn panel(title: &str, theme: Theme) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::default().bg(theme.surface).fg(theme.text))
        .border_style(Style::default().fg(theme.border))
}

fn row(key: &str, value: impl Into<String>, theme: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<16}"), Style::default().fg(theme.muted)),
        Span::styled(value.into(), Style::default().fg(theme.text)),
    ])
}

fn heading(title: &str, theme: Theme) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    ))
}

fn centered(area: Rect, width_pct: u16, height_pct: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - height_pct) / 2),
        Constraint::Percentage(height_pct),
        Constraint::Percentage((100 - height_pct) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - width_pct) / 2),
        Constraint::Percentage(width_pct),
        Constraint::Percentage((100 - width_pct) / 2),
    ])
    .split(vertical[1])[1]
}

fn status_color(status: &str, theme: Theme) -> ratatui::style::Color {
    match status {
        "success" | "ok" => theme.success,
        "failed" | "error" => theme.error,
        "running" | "queued" => theme.warning,
        _ => theme.muted,
    }
}

fn severity_color(severity: Severity, theme: Theme) -> ratatui::style::Color {
    match severity {
        Severity::Error => theme.error,
        Severity::Warning => theme.warning,
        Severity::Info => theme.primary,
    }
}

fn severity_mark(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "[E]",
        Severity::Warning => "[W]",
        Severity::Info => "[I]",
    }
}

fn ratio_u64(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64).clamp(0.0, 1.0)
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let text: String = value.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", text)
    }
}
