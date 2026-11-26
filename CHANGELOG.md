# Changelog

## 1.0.0 — 2026-11-27

First stable public release.

- Native Rust TUI with ten working screens; no web view, localhost server, or mock data.
- Real `void-packages` workspace support: template scanning and validation, dependency and cycle analysis, patches, source verification, official `xbps-src` phases with live logs and cancellation, Git operations, diagnosis from stored logs, and local repository listing/indexing.
- SQLite settings, workspace index, build history, and automatic in-place migration.

Hardening completed for the stable release:

- Terminal state is restored on panics and editor failures; raw mode and the alternate screen never leak.
- Build logs stream with bounded flush waits and UTF-8-safe lines; noisy logs are capped and de-duplicated.
- Distfile and workspace paths are containment-checked, including symlinks; downloads reject symlink destinations and use timeouts.
- Template parsing handles multi-line quoted values, subpackage-local variables, shell parameter expansions, uppercase package names, `@` checksums, and versioned/virtual dependencies.
- Dependency detection compares exact installed package names and detects cycles across all four dependency fields without phantom subpackage nodes.
- Patch names and diff paths are validated; affected files are backed up with unique names and restored if `patch` fails.
- Git arguments are validated and passed after `--`; status parsing uses NUL-delimited porcelain output and understands renames.
- Repository indexing passes actual `.xbps` archives with `-f` and detects an empty index instead of reporting false success.
- Tool detection is architecture-aware and requires real successful version output.
- Navigation uses cached template data; the first screen no longer scans thousands of templates.

Quality gate: 28 unit tests, 5 integration tests against a fixture `void-packages` tree, GitHub Actions CI for fmt/check/test/clippy/release build, and the MIT license.

Supported target: Void Linux x86_64 with glibc. Builds must run as a normal user, never as root.
