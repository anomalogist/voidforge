# Changelog

## 1.0.0 — 2026-11-26

First stable public release.

- Native Rust TUI with ten working screens; no web view, localhost server or mock data.
- Real `void-packages` workspace support: template scanning/validation, dependency and cycle
  analysis, patches, source verification, `xbps-src` phases with live logs and cancellation,
  Git operations, diagnosis from real logs, and local repository listing/indexing.
- SQLite settings, workspace index, build history and automatic in-place migration.
- Performance fix: navigation no longer re-runs Git status; untracked file walking removed.
- Built-package list now shows package names, versions and sizes instead of a count only.
- Path containment now resolves symlinks before granting access to files outside a workspace.
- Build mutexes recover from poisoning instead of panicking.
- 20 unit tests, 5 integration tests against a fixture `void-packages` tree, GitHub Actions CI,
  MIT license, portable README and Persian beginner/technical guides.

Supported target: Void Linux x86_64 with glibc. Builds must run as a normal user, never as root.
