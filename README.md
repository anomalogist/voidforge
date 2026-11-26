# VoidForge

**Version 1.0.0 — stable release. License: MIT.**

The Visual Package Workshop for Void Linux — terminal edition.

VoidForge is a native Rust TUI for creating, inspecting, building, diagnosing, and releasing Void
Linux packages from a real `void-packages` workspace. It uses the official `xbps-src` workflow and
treats the filesystem as the source of truth; SQLite stores only settings, indexes, and history.

No web view, no localhost server, no mock data, and no simulated builds.

## Requirements

Supported target: **Void Linux x86_64 with glibc** (other distributions are not supported).

- Rust and Cargo 1.89 or newer (only for building from source)
- A local `void-packages` tree containing `xbps-src`
- Usual Void build tools: `xbps-src`, `xbps`, `xbps-query`, `xbps-rindex`, `git`, `patch`, `curl`,
  `tar`, `bash`, `ninja`

Install prerequisites on Void:

```sh
sudo xbps-install -S rust git curl tar bash patch ninja xbps xbps-src
```

VoidForge itself **must never be run as root**: `xbps-src` builds are unprivileged, and running
under `sudo` would create root-owned files in your workspace and write settings to `/root` instead
of your home directory. Use `sudo` only for installing prerequisites or copying the binary into a
system path.

## Install and run

From source:

```sh
git clone <repository-url> voidforge
cd voidforge
cargo build --release
./target/release/voidforge
```

Optional system-wide install of the resulting binary:

```sh
sudo install -Dm755 target/release/voidforge /usr/local/bin/voidforge
voidforge
```

For development:

```sh
cd voidforge
cargo run
```

The first start scans the registered `void-packages` tree and usually takes a few seconds.
Quit with `q` (`Ctrl+C` also exits safely and cancels an active build).

## First session

1. Press `2` (Workspaces) and register your tree with `:workspace add ~/void-packages` if it is not
   already listed.
2. Check it with `:workspace validate`, then press `8` (Diagnostics) to see the real evidence.
3. Press `3` (Packages), filter with `/hello`, and open a template with `Enter`.
4. Press `7` (Build Center), choose a phase such as `pkg`, and press `b` or `Enter`.
5. If a build fails, run `:diagnose last` to inspect evidence from the stored log.
6. See produced packages with `:release list`, and commit changes with `:git status` /
   `:git commit "<message>"`.

The full beginner tutorial is in `docs/آموزش-مبتدی.md` (or `VoidForge-آموزش-مبتدی.md` next to the
repository when distributed together), and the technical guide is in `docs/راهنما.md`.

## Keys

| Key | Action |
| --- | --- |
| `1`–`0` | Switch screen |
| `Tab` / `Shift+Tab` | Next / previous screen |
| `j`/`k` or arrow keys | Move selection |
| `Enter` | Activate selected item |
| `e` | Open selected template in `$EDITOR` |
| `v` | Validate selected template |
| `b` | Build selected package with selected phase |
| `x` | Request cancellation of active build |
| `r` | Refresh workspace data |
| `:` | Command line |
| `/` | Package filter |
| `?` | Help |
| `q` / `Ctrl+C` | Quit |

## Command line

Workspace:

```text
:workspace add <path>
:workspace clone <url> [name]
:workspace use <name>
:workspace validate
:workspace refresh
:workspace remove <name>
```

Packages and templates:

```text
:pkg list|new <name>|duplicate <src> <dst>|rename <old> <new>|open <name>
:template validate
:template edit
```

Dependencies, patches, and sources:

```text
:deps cycles
:patch list|new <name>.patch|from-git <name>.patch <file>|apply <name>|revert <name>|remove <name>
:source verify|download [index]|extract [filename]|github <url>
```

Build, diagnosis, and release:

```text
:build <phase> [package]
:build cancel
:build logs [index]
:diagnose last|<build-id>
:release list|index|info <name>
```

Git:

```text
:git status|diff|stage <path>|unstage <path>|commit <message>|branch|checkout <branch>
:git branch-new <name>|fetch [remote]|pull [remote] [branch]|push [remote] [branch]|tag
```

Settings:

```text
:theme dark|light
:jobs <1-64>
:editor <command>
:clear
:view <screen>
```

Build phases are `bootstrap`, `binary-bootstrap`, `fetch`, `extract`, `patch`, `configure`,
`build`, `check`, `install`, `pkg`, and `clean`.

## Screens

- **Dashboard** — real host, toolchain, CPU, memory, swap, and build history
- **Workspaces** — register, validate, clone, and switch `void-packages` trees
- **Packages** — scan `srcpkgs`, filter templates, create/duplicate/rename packages
- **Template** — parsed variables, functions, static validation, external editor
- **Dependencies** — parsed dependency kinds, repo/host evidence, cycle analysis
- **Patches** — list, preview, create drafts, generate from git diffs, apply/revert with backups
- **Build Center** — official `xbps-src` phases, live streamed logs, cancellation, history
- **Diagnostics** — evidence extracted from real stored build logs
- **Git & Repos** — working tree, branches, remotes, diffs, and remote operations
- **Settings** — theme, jobs, editor, and validation preferences

## Architecture

```text
src/
├── app.rs              State, command parser, key handling, confirmations
├── build.rs            xbps-src process management and streamed events
├── db.rs               SQLite indexes, settings, build/git history
├── deps.rs             Dependency parsing and cycle detection
├── diagnostics.rs      Real build-log evidence rules
├── events.rs           TUI event channel
├── git.rs              Safe git wrapper without shell interpretation
├── models.rs           Shared domain models
├── package.rs          Template parsing and srcpkgs operations
├── patches.rs          Patch creation, validation, apply/revert
├── release.rs          binpkg listing, metadata, xbps-rindex
├── source.rs           distfiles, checksums, download/extract, GitHub tags
├── system.rs           Host/tool detection and metric sampling
├── template.rs         Static template validation
├── tui/
│   ├── mod.rs          Terminal lifecycle and event loop
│   ├── render.rs       Ratatui rendering for all screens
│   └── theme.rs        Dark/light palettes
├── utils/path.rs       Path expansion and workspace containment checks
└── workspace.rs        Workspace registration and validation
```

Migrations live in `migrations/` and are embedded at compile time.

## Storage

Application data is stored in `~/.local/share/voidforge/voidforge.sqlite3`, or
`$XDG_DATA_HOME/voidforge/voidforge.sqlite3` when set. Build logs are stored under
`~/.local/share/voidforge/logs/`.

Package templates, patches, sources, and build artifacts stay inside the selected
`void-packages` workspace. Removing a workspace from VoidForge only removes its database entry;
files on disk are not deleted.

## Quality checks and CI

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
```

The same commands run in `.github/workflows/ci.yml` on every push and pull request. The test suite
contains unit tests plus integration tests that exercise a fixture `void-packages` tree (template
scanning, validation, dependencies, patches, git metadata, release listing, and `xbps-src`
invocation/log capture using a stub script). No network access is required by the tests.

## Known limitations

- Only Void Linux x86_64 with glibc is supported and tested; CI runs on a generic Linux runner for
  logic tests, not for real package builds.
- Static template validation catches common mistakes but cannot guarantee that a package builds.
- Real `xbps-src` builds depend on your local toolchain and a valid `void-packages` tree; VoidForge
  reports the tool's real exit status and log rather than emulating success.
- Database schema changes are handled by embedded migrations; very old databases are upgraded
  in place.

## License

MIT — see `LICENSE`.
