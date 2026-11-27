<div align="center">
<p align="center">
  <img src="voidforge-icon.png" alt="VoidForge" width="96">
</p>
<h1>VoidForge</h1>

**The visual package workshop for Void Linux in your terminal.**



[![CI](https://github.com/anomalogist/voidforge/actions/workflows/ci.yml/badge.svg)](https://github.com/anomalogist/voidforge/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-478A62.svg)](LICENSE)
[![Rust 1.89+](https://img.shields.io/badge/Rust-1.89%2B-478A62.svg)](https://www.rust-lang.org/)
[![Void Linux](https://img.shields.io/badge/Target-Void%20Linux-185C3A.svg)](https://voidlinux.org/)



**Stable 1.0.0 · MIT license · No web server · No mock data**

</div>

---

VoidForge is a native Rust terminal application for working on **real Void Linux packages**. It opens your local `void-packages` tree, reads the templates on disk, runs the official `xbps-src` phases, streams the actual build log, and helps you turn failures into useful evidence.

It is a workshop, not a simulator. There is no dashboard server, no embedded browser, and no pretend build output. When VoidForge says a build succeeded, an `xbps-src` process really exited with status 0. When it says a template has a problem, that conclusion comes from the file you are editing.

```text
┌─ Dashboard ─────────────────────┬─ Build Center ────────────────────────────┐
│ HOST                            │ PHASE  build        PACKAGE  hello       │
│   Void Linux · x86_64 · glibc   │                                          │
│ TOOLS                           │ > xbps-src build hello                   │
│   xbps-src ✓   git ✓            │   => Building hello-1.0_1...            │
│   xbps-rindex ✓  curl ✓         │   => CC gcc ...                          │
│ RESOURCES                       │   => OK: hello-1.0_1 built               │
│   CPU 12%   RAM 1.4/8.0 GiB     │                                          │
│ ACTIVITY                        │STATUS  success (exit 0) · 47s            │
│   #12 build hello  success      │                                          │
└─────────────────────────────────┴──────────────────────────────────────────┘
: help   / filter   ? keys   q quit   Tab screens
```

> [!IMPORTANT]
> Run VoidForge as your normal user. **Never run it with `sudo`.** `xbps-src` builds must be unprivileged; root would leave root-owned files in your workspace and write settings into `/root`.

## Table of contents

- [Why VoidForge](#why-voidforge)
- [Requirements](#requirements)
- [Install and run](#install-and-run)
- [Your first session](#your-first-session)
- [A guided tour of the screens](#a-guided-tour-of-the-screens)
- [Everyday workflows](#everyday-workflows)
- [Keyboard and command reference](#keyboard-and-command-reference)
- [Where things are stored](#where-things-are-stored)
- [Safety and limitations](#safety-and-limitations)
- [Troubleshooting](#troubleshooting)
- [Project layout](#project-layout)
- [Development and CI](#development-and-ci)
- [License](#license)

## Why VoidForge

Packaging on Void usually means jumping between a template in an editor, a terminal running `xbps-src`, another terminal for `git`, and a separate log file when something breaks. VoidForge puts those steps in one keyboard-driven workspace while leaving the real files and commands in charge.

What it does well:

- **Reads real templates.** Multi-line `depends`, subpackage functions, shell variables, and archived-content checksums are parsed the way Void templates actually use them.
- **Runs official tooling.** Builds are `xbps-src <phase> <package>` with the configured job count. No reimplementation of the build system.
- **Keeps evidence.** Every build writes a timestamped log. Diagnosis highlights real log lines, not guesses detached from the output.
- **Respects your repository.** Git commands use argument arrays and `--` separators; patch names and patch paths are validated; workspace operations never delete your files behind your back.
- **Starts fast.** The initial screen does not scan thousands of templates. Package data is loaded when you actually open a package screen.
- **Stays out of the way.** Settings, indexes, and history live in XDG data. Your `void-packages` tree remains the source of truth.

## Requirements

Supported target: **Void Linux x86_64 with glibc**. Other distributions are not supported; CI on generic Linux only exercises the logic tests.

- Rust and Cargo **1.89 or newer** if you build from source
- A local `void-packages` checkout with `./xbps-src`
- The usual Void build tools: `xbps-src`, `xbps`, `xbps-query`, `xbps-rindex`, `git`, `patch`, `curl`, `tar`, `bash`, `ninja`

Install prerequisites on Void:

```sh
sudo xbps-install -S rust git curl tar bash patch ninja xbps xbps-src
```

If you already have a `void-packages` tree, note its absolute path — you will register it in the next section. If not, clone one first:

```sh
git clone https://github.com/void-linux/void-packages.git ~/void-packages
```

## Install and run

There is no binary release channel yet, so installation starts from source:

```sh
git clone https://github.com/anomalogist/voidforge.git
cd voidforge
cargo build --release
./target/release/voidforge
```

Optional system-wide install:

```sh
sudo install -Dm755 target/release/voidforge /usr/local/bin/voidforge
voidforge
```

For development, use `cargo run`. Quit with `q`, `Esc` in a dialog, or `Ctrl+C`; an active build is cancelled and its database row is marked `cancelled` before exit.

## Your first session

This walkthrough assumes `~/void-packages` exists and you started VoidForge with no previous configuration.

### 1. Register your workspace

Press `2` for **Workspaces**, then type:

```text
:workspace add ~/void-packages
```

`~` and absolute paths are both accepted. VoidForge expands the path, rejects traversal, and stores an absolute path in its database. The workspace appears in the list immediately.

If you prefer to let VoidForge clone it for you:

```text
:workspace clone https://github.com/void-linux/void-packages.git void-packages
```

Switch between multiple trees with `:workspace use <name>`. Removing one with `:workspace remove <name>` only deletes the database entry — the repository on disk is untouched.

### 2. Confirm the toolchain

Stay on the Dashboard (`1`) and look at **HOST** and **TOOLS**. Missing tools are listed rather than hidden. If something is absent, install it with `xbps-install` and press `r` to refresh.

For a deeper check run:

```text
:workspace validate
```

Press `8` to open **Diagnostics**. Each finding shows a title, a short explanation, and, where relevant, the exact file and line.

### 3. Find a template

Press `3` for **Packages**. The first visit loads `srcpkgs` from disk. Type `/hello` to filter by name or description, move with `j`/`k`, and press `Enter` to open the **Template** screen.

The Template screen shows parsed variables, subpackage functions, and static validation. Press `v` to re-run validation at any time, or `e` to open the file in your editor. On `VoidForge`, the editor is `$EDITOR` by default and can be changed with `:editor "..."`; after a successful edit the template is reloaded automatically.

### 4. Run your first build

Go to **Build Center** with `7`. Select a phase with `j`/`k` — the list runs from `bootstrap` through `pkg` and `clean`. For an existing package, `pkg` is usually the phase you want.

Press `b` (or `Enter`) to start. The log streams live, `x` requests cancellation, and the status line reports the real exit code and duration. Press `r` in a log view only when you want fresh workspace data; navigation itself never re-scans the tree.

### 5. Understand a failure

When a build fails, VoidForge analyzes the stored log and opens **Diagnostics** if the analysis succeeds. You can repeat it later:

```text
:diagnose last
:diagnose 12
:build logs 0
```

Findings are tied to log lines and marked as inferred hints, not as guaranteed root causes. Use them together with the log text and your knowledge of the package.

### 6. Ship the result

List built packages with `:release list`. If you maintain a local binary repository, index it with:

```text
:release index
```

That command enumerates the `.xbps` archives and runs `xbps-rindex -a -f` on them, so rebuilt packages with the same version are refreshed instead of silently left stale. Inspect metadata with `:release info <name>`.

Finally, review and commit your work from **Git & Repos**:

```text
:git status
:git diff
:git stage srcpkgs/hello/template
:git commit "hello: update to 1.1"
:git push origin master
```

Remote operations ask for confirmation. Branches and remotes are validated before being passed to git, and option-like values such as `--upload-pack=...` are rejected.

## A guided tour of the screens

Press `1`–`0` to jump directly to a screen, or `Tab` / `Shift+Tab` to cycle.

| # | Screen | What you can do there |
| --- | --- | --- |
| `1` | **Dashboard** | Host, glibc/musl detection, tool availability, CPU/memory/swap, recent history |
| `2` | **Workspaces** | Register, clone, validate, refresh, switch, and remove `void-packages` trees |
| `3` | **Packages** | Browse `srcpkgs`, filter, create, duplicate, rename, and open templates |
| `4` | **Template** | Variables, functions, static checks, external editor integration |
| `5` | **Dependencies** | `depends`, `makedepends`, `hostmakedepends`, `checkdepends` with repository/host evidence |
| `6` | **Patches** | List, preview, create, generate from git diffs, apply and revert with backups |
| `7` | **Build Center** | Official `xbps-src` phases, live logs, cancellation, build history, output packages |
| `8` | **Diagnostics** | Log-backed findings for builds, templates, workspaces, and dependency cycles |
| `9` | **Git & Repos** | Status, branches, remotes, diffs, stage/unstage, commit, fetch, pull, push, tags |
| `0` | **Settings** | Theme (dark/light), parallel jobs (1–64), editor command, safety preferences |

A few details worth knowing:

- The status bar always shows the current message and a short hint line.
- `?` toggles the full in-app key/command help; `Esc` dismisses help, dialogs, or the current message.
- Filter text is matched against both package names and descriptions.
- Selecting a different package clears stale diagnostics, so findings always belong to the template you are looking at.
- Untracked build trees (`masterdir`, `hostdir`, `.git`) are skipped when expanding git status, which keeps a large workspace responsive.

## Everyday workflows

### Create and duplicate a package

```text
:pkg new mypackage
:pkg duplicate mypackage mypackage-git
:pkg rename mypackage old-mypackage
```

`pkg new` writes a minimal template with all required fields present but empty. Fill in `version`, `short_desc`, `maintainer`, `license`, `homepage`, `distfiles`, and `checksum` before building. `pkg rename` moves the directory on disk and rewrites the top-level `pkgname`, even for subpackages that were originally symlinks.

### Keep source files honest

```text
:source verify
:source download 0
:source extract hello-1.0.tar.gz
:source github https://github.com/owner/project
```

`verify` compares declared checksums with files in the source cache. `download` fetches a distfile with timeouts and rejects unsafe names and symlink destinations. `extract` unpacks into a contained directory. `source github` asks the GitHub API for the latest release tag.

### Work with patches

```text
:patch new fix-build.patch
:patch from-git fix-build.patch src/configure.ac
:patch apply fix-build.patch
:patch revert fix-build.patch
:patch remove fix-build.patch
```

A new patch starts empty; edit it in your own editor. `from-git` creates a unified diff against `HEAD`, including staged changes. `apply` and `revert` validate the patch, snapshot every affected file with a unique backup name, and restore those snapshots if `patch` fails partway through. Patch names cannot contain directories or `..`, and diff paths are checked before any file is touched.

### Analyze dependency cycles

```text
:dependencies     (or view 5)
:deps cycles      -> opens Diagnostics
```

Cycle analysis parses all four dependency fields, skips symlinked subpackages to avoid phantom nodes, canonicalizes rotations, and reports each distinct cycle once.

### Tune the environment

```text
:theme light
:jobs 8
:editor "vim"
:view build
```

Invalid theme names and out-of-range job counts are rejected with a clear message instead of being silently replaced.

## Keyboard and command reference

### Keys

| Key | Action |
| --- | --- |
| `1`–`0` | Switch screen |
| `Tab` / `Shift+Tab` | Next / previous screen |
| `j`/`k` or arrow keys | Move selection |
| `Home` / `End` | First / last item |
| `Enter` | Activate selected item or confirm a dialog |
| `Esc` | Close help, dialog, command line, or message |
| `q` | Quit; closes a confirmation dialog when one is open |
| `Ctrl+C` | Quit safely from any mode |
| `e` | Edit the selected template |
| `v` | Validate the selected template |
| `b` | Build the selected package with the selected phase |
| `x` | Cancel the active build |
| `r` | Refresh workspace data |
| `:` | Open the command line |
| `/` | Start a package filter |
| `?` | Toggle help |
| `t` | Toggle theme on the Settings screen |

### Commands

Everything typed after `:` is tokenized with quote support, so messages such as `:git commit "fix broken build"` work as expected.

**Workspaces**

```text
:workspace add <path>
:workspace clone <url> [name]
:workspace use <name-or-path>
:workspace validate
:workspace refresh
:workspace remove <name>
```

**Packages and templates**

```text
:pkg list
:pkg new <name>
:pkg duplicate <source> <target>
:pkg rename <old> <new>
:pkg open <name>
:template validate
:template edit
```

**Dependencies, patches, and sources**

```text
:deps cycles
:patch list
:patch new <name>.patch
:patch from-git <name>.patch <file>
:patch apply <name>.patch
:patch revert <name>.patch
:patch remove <name>.patch
:source verify
:source download [index]
:source extract [filename]
:source github <url>
```

**Builds, diagnosis, and releases**

```text
:build <phase> [package]
:build cancel
:build logs [index]
:diagnose last|<build-id>
:release list
:release index
:release info <name>
```

Build phases: `bootstrap`, `binary-bootstrap`, `fetch`, `extract`, `patch`, `configure`, `build`, `check`, `install`, `pkg`, `clean`.

**Git**

```text
:git status
:git diff
:git stage <path>
:git unstage <path>
:git commit <message>
:git branch
:git branch-new <name>
:git checkout <branch>
:git fetch [remote]
:git pull [remote] [branch]
:git push [remote] [branch]
:git tag
```

**Utilities**

```text
:filter <words>
:theme dark|light
:jobs <1-64>
:editor <command>
:view dashboard|workspaces|packages|template|deps|patches|build|diagnostics|git|settings
:help
:clear
:quit
```

## Where things are stored

VoidForge deliberately keeps very little state of its own.

| Path | Contents |
| --- | --- |
| `~/.local/share/voidforge/voidforge.sqlite3` | Settings, workspace index, build and git history, notices |
| `~/.local/share/voidforge/logs/` | Timestamped `xbps-src` logs used by `:diagnose` |
| `$XDG_DATA_HOME/voidforge/...` | Same layout when `XDG_DATA_HOME` is set |
| Your `void-packages` tree | Templates, patches, sources, build artifacts, git repository |

Migrations are embedded into the binary and applied in place at startup. Removing a workspace entry never removes repository files, and no command in VoidForge deletes your `void-packages` tree.

## Safety and limitations

- Only Void Linux x86_64 with glibc is supported and tested.
- Static template validation catches common mistakes; it cannot prove that `xbps-src` will succeed.
- Builds use the real toolchain on your machine. A missing dependency or broken masterdir is reported, not hidden.
- Network operations are bounded by timeouts and never prompt for credentials interactively.
- Patches are checked before they touch files; failed applications are rolled back from unique backups. Reject files (`.rej`) are left for inspection.
- VoidForge does not execute shell strings built from user input. Commands use argument arrays, and paths from templates or diffs are containment-checked.
- Very old databases are upgraded in place by embedded migrations. Keep a copy if you want to roll back an experiment.

## Troubleshooting

**`not a terminal` when starting.** VoidForge needs a real TTY. Run it in a terminal emulator rather than piping its output or launching it from a CI script.

**The terminal looks odd after a crash.** VoidForge installs a panic hook that leaves raw mode and the alternate screen, so this should be rare. If it still happens, run `reset` in the terminal, or `stty sane` followed by `clear`.

**No packages appear.** Check the active workspace on screen `2`, confirm `srcpkgs` exists, then run `:workspace validate` and `r`. Individual unreadable templates are skipped instead of hiding the whole tree.

**Tools show as missing.** Install them with `sudo xbps-install -S ...`, then press `r`. Tool checks require a successful command with a version-like output line.

**A build fails with no diagnostics text.** Open `:build logs <index>` and read the raw log. Diagnosis only replaces findings when it can read and analyze the log; a failed analysis never overwrites previous evidence silently.

**`:git pull` says to check out a branch.** Detached HEAD has no branch to pull into. Run `:git checkout <branch>` first.

**Patch apply reports a rejection.** Inspect `.rej` files next to the affected source files. VoidForge has already restored the originals from its backups; fix the patch and apply again.

**A package name starting with a capital letter is rejected.** It should not be — names such as `R-cran-foo` and `AppStream` are valid. If you see this, please report it as a bug.

## Project layout

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
├── patches.rs          Patch validation, apply, revert, and backups
├── release.rs          binpkg listing, metadata, xbps-rindex
├── source.rs           distfiles, checksums, download/extract, GitHub tags
├── system.rs           Host/tool detection and metric sampling
├── template.rs         Static template validation
├── tui/
│   ├── mod.rs          Terminal lifecycle and event loop
│   ├── render.rs       Ratatui rendering for every screen
│   └── theme.rs        Dark and light palettes
├── utils/path.rs       Path expansion and workspace containment checks
└── workspace.rs        Workspace registration and validation
```

Database schema changes live in `migrations/` and are embedded at compile time.

## Development and CI

The full quality gate is intentionally short enough to run before every commit:

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
```

The same commands run in `.github/workflows/ci.yml` on every push and pull request. Tests include unit tests and integration tests against a fixture `void-packages` tree: template scanning, validation, dependencies, patches, git metadata, release listing, and `xbps-src` invocation with a stub script. No network access or real build is required by the test suite.

Bug reports and pull requests are welcome. Please include your Void version, the exact command you ran, and the relevant log excerpt; "it didn't work" is very hard to act on, while five lines of real output usually are enough.

## License

MIT — see [LICENSE](LICENSE).
