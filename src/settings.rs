use sysinfo::System;

use crate::db;
use crate::error::Result;
use crate::utils::path::data_dir;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeName {
    Dark,
    Light,
}

impl ThemeName {
    pub fn parse_checked(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("light") {
            Some(Self::Light)
        } else if value.eq_ignore_ascii_case("dark") {
            Some(Self::Dark)
        } else {
            None
        }
    }

    /// Persisted values are trusted; unknown values fall back to the default.
    /// User input goes through `parse_checked` so typos are reported instead.
    pub fn parse(value: &str) -> Self {
        Self::parse_checked(value).unwrap_or(Self::Dark)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub theme: ThemeName,
    pub jobs: usize,
    pub editor: String,
    pub confirm_dangerous: bool,
    pub check_installed: bool,
}

impl Settings {
    pub fn load(conn: &rusqlite::Connection) -> Result<Self> {
        let theme = db::get_setting(conn, "theme")?
            .map(|value| ThemeName::parse(&value))
            .unwrap_or(ThemeName::Dark);
        let jobs = db::get_setting(conn, "jobs")?
            .and_then(|value| value.parse::<usize>().ok())
            .map(|value| value.clamp(1, 64))
            .unwrap_or_else(default_jobs);
        let editor = db::get_setting(conn, "editor")?
            .filter(|value| !value.is_empty())
            .or_else(|| std::env::var("EDITOR").ok())
            .unwrap_or_else(|| "vi".to_string());
        let confirm_dangerous = db::get_setting(conn, "confirm_dangerous")?
            .map(|value| value == "true")
            .unwrap_or(true);
        let check_installed = db::get_setting(conn, "check_installed")?
            .map(|value| value == "true")
            .unwrap_or(false);
        Ok(Self {
            theme,
            jobs,
            editor,
            confirm_dangerous,
            check_installed,
        })
    }

    pub fn save(&self, conn: &rusqlite::Connection) -> Result<()> {
        db::set_setting(conn, "theme", self.theme.as_str())?;
        db::set_setting(conn, "jobs", &self.jobs.to_string())?;
        db::set_setting(conn, "editor", &self.editor)?;
        db::set_setting(
            conn,
            "confirm_dangerous",
            &self.confirm_dangerous.to_string(),
        )?;
        db::set_setting(conn, "check_installed", &self.check_installed.to_string())?;
        Ok(())
    }

    pub fn log_dir(&self) -> Result<std::path::PathBuf> {
        let path = data_dir()?.join("logs");
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }
}

pub fn default_jobs() -> usize {
    let mut system = System::new_all();
    system.refresh_memory();
    let memory_gb = system.total_memory() / (1024 * 1024 * 1024);
    if memory_gb <= 6 {
        1
    } else {
        let cpus = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(1);
        cpus.clamp(1, 4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_theme() {
        assert_eq!(ThemeName::parse("light"), ThemeName::Light);
        assert_eq!(ThemeName::parse("dark"), ThemeName::Dark);
    }

    #[test]
    fn jobs_never_zero() {
        assert!(default_jobs() >= 1);
    }
}
