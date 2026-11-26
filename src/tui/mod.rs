pub mod render;
pub mod theme;

use std::io::{self, Stdout};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use sysinfo::{Disks, System};

use crate::app::{self, App, KeyOutcome};
use crate::events::{NoticeLevel, UiEvent};
use crate::system;

type Backend = CrosstermBackend<Stdout>;

/// Always put the terminal back the way we found it, even if the event loop
/// returns early or a key handler panics. A TUI that leaves raw mode enabled
/// after a crash is infuriating to recover from.
struct TerminalGuard {
    armed: bool,
}

impl TerminalGuard {
    fn new() -> Self {
        Self { armed: true }
    }

    fn restore(&mut self) -> io::Result<()> {
        if !self.armed {
            return Ok(());
        }
        self.armed = false;
        let mut stdout = io::stdout();
        let raw = disable_raw_mode();
        let screen = execute!(stdout, LeaveAlternateScreen, Show);
        raw.and(screen)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub fn run() -> io::Result<()> {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
        previous_hook(info);
    }));

    enable_raw_mode()?;
    let mut guard = TerminalGuard::new();
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_app(&mut terminal);
    result.and(guard.restore())
}

fn run_app(terminal: &mut Terminal<Backend>) -> io::Result<()> {
    let mut app = App::new().map_err(other)?;
    spawn_metrics(app.builds_manager.event_sender());

    while app.running {
        terminal.draw(|frame| render::render(frame, &app))?;

        while event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match app.handle_key(key) {
                    KeyOutcome::Continue => {}
                    KeyOutcome::Edit(path) => edit_template(terminal, &mut app, &path)?,
                },
                Event::Key(_) => {}
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        while let Ok(event) = app.events_rx.try_recv() {
            app.handle_event(event);
        }

        if let Some(path) = app.edit_request.take() {
            edit_template(terminal, &mut app, &path)?;
        }
    }
    Ok(())
}

fn spawn_metrics(sender: mpsc::Sender<UiEvent>) {
    thread::spawn(move || {
        let mut system = System::new_all();
        let mut disks = Disks::new_with_refreshed_list();
        loop {
            thread::sleep(Duration::from_secs(2));
            let metrics = system::sample_metrics(&mut system, &mut disks);
            if sender.send(UiEvent::Metrics(metrics)).is_err() {
                break;
            }
        }
    });
}

fn edit_template(
    terminal: &mut Terminal<Backend>,
    app: &mut App,
    path: &std::path::Path,
) -> io::Result<()> {
    let editor = app.settings.editor.clone();
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, Show)?;

    let result = app::open_editor(&editor, &path.to_path_buf());

    // Restore the TUI before doing anything else, including on editor errors.
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen, Hide)?;
    if result.is_ok() {
        app.refresh_all();
    }
    match result {
        Ok(()) => app.notify(NoticeLevel::Success, "editor closed; template reloaded"),
        Err(error) => app.notify(NoticeLevel::Error, error.to_string()),
    }
    Ok(())
}

fn other(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
