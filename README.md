# Directory Saver

A small, keyboard-driven desktop app for bookmarking the directories you use often and jumping back to them quickly. Built in Rust with [gpui](https://www.gpui.rs/), the GPU-accelerated UI framework behind the Zed editor.

## Features

- **Save directories** from one Add folder dialog. Paste a path or browse with the native folder picker. Only real folders get saved.
- **Paste-friendly paths.** Accepts absolute paths, `~` and `~/…`, quoted paths, and `file://` URIs (percent-encoded too).
- **Instant search** across directory names and paths. Every word in the query must match, and names that start with the query rank first.
- **Recent first.** Results are ordered by when you last opened them, then by when you added them.
- **Open in your file manager or a terminal.** Each row has Files and Terminal buttons. The terminal starts inside the folder (uses `$TERMINAL`, then gnome-terminal, konsole, kitty, alacritty, wezterm, xterm and others).
- **Stays local.** Everything is stored in a local SQLite database. No accounts, no network.

## Keyboard shortcuts

| Key              | Action                                          |
| ---------------- | ----------------------------------------------- |
| Type             | Search saved directories                        |
| `↑` / `↓`        | Move the selection                              |
| `Enter`          | Open the selected directory in the file manager |
| `Ctrl+Enter`     | Open a terminal in the selected directory       |
| `Esc`            | Clear the search, or close the Add folder dialog |
| `Ctrl+N`         | Open the Add folder dialog                      |
| `Ctrl+Backspace` | Remove the selected directory from the list     |
| `Ctrl+Q`         | Quit                                            |

Removing a directory only deletes the bookmark. Nothing on disk is touched.

## Install on Linux

Download or build `directory-saver-<version>-x86_64-linux.tar.gz`, then:

```sh
tar xzf directory-saver-*-linux.tar.gz
cd directory-saver-*-linux
./install.sh        # installs to ~/.local (set PREFIX=/usr/local with sudo for system-wide)
```

Directory Saver then shows up in your app launcher with its icon. `./uninstall.sh` removes it and keeps your saved directories.

## Data location

Saved directories live in `directories.db` inside your platform's data directory:

- Linux: `~/.local/share/directory-saver/directories.db`
- macOS: `~/Library/Application Support/directory-saver/directories.db`

Delete that file to start fresh.

## Building from source

### Requirements

- Rust (edition 2024, so Rust 1.85 or newer) via [rustup](https://rustup.rs/)
- Linux or macOS. The path-pasting code uses Unix APIs, so Windows isn't supported yet.
- On Linux, the system libraries gpui needs. On Debian/Ubuntu:

  ```sh
  sudo apt install build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-dev \
    libwayland-dev libx11-xcb-dev libvulkan-dev libfontconfig-dev libssl-dev
  ```

  You also need a GPU driver with Vulkan support.

### Run

```sh
git clone https://github.com/danirisdiandita/Directory-Saver.git
cd Directory-Saver
cargo run --release
```

### Build a Linux package

```sh
./scripts/build-linux.sh    # writes dist/directory-saver-<version>-<arch>-linux.tar.gz
```

The package holds the release binary, a `.desktop` entry, the icon set (`assets/icons`, rendered from `assets/icon.svg`) and the install scripts.

### Develop

```sh
cargo test                  # store and path-parsing tests
cargo clippy --all-targets
cargo fmt
```

SQLite is bundled through `rusqlite`, so you don't need a system SQLite.

## Project layout

```
src/
  main.rs        app window, list UI, actions and keybindings
  text_input.rs  single-line text input component for gpui
  store.rs       SQLite store: migrations, add, search, remove, last-opened tracking
  paste.rs       turns pasted text (~, quotes, file:// URIs) into absolute paths
  terminal.rs    finds a terminal emulator and opens it in a directory
assets/          app icon (icon.svg) and rendered hicolor PNGs
packaging/       .desktop entry, install.sh and uninstall.sh
scripts/         build-linux.sh release packager
patches/xattr    local patch for a libc incompatibility (see below)
```

### About `patches/xattr`

gpui 0.2.2 depends on `xattr` 0.2.3 (through `zed-async-tar`), which references `libc::ENOATTR`. Newer `libc` releases removed that constant on Linux. The local patch, wired up with `[patch.crates-io]` in `Cargo.toml`, swaps it for the equivalent `ENODATA`. It can go once upstream fixes this.

## Contributing

See [AGENTS.md](AGENTS.md) for the branch and worktree workflow. All work branches off `dev` and lands back in `dev`.
