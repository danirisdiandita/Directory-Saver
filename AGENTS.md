# AGENTS.md

## Project

Directory Saver is a **desktop app written in Rust with [gpui](https://www.gpui.rs/)** (the UI framework from Zed).

- UI: gpui 0.2.x (`src/main.rs`, `src/text_input.rs`)
- Storage: SQLite via `rusqlite` (bundled) for saved directories
- `patches/xattr` is a local crate patch: gpui 0.2.2 pulls `xattr` 0.2.3, which uses `libc::ENOATTR` (removed on Linux in newer libc). Keep the `[patch.crates-io]` entry in `Cargo.toml` until upstream fixes it.

Common commands:

```sh
cargo build
cargo run
cargo test
cargo clippy --all-targets
cargo fmt
```

## Workflow: always use a git worktree

Every piece of work (feature, bug fix, or chore) happens in its **own git worktree on its own branch**. Never edit directly in the main checkout.

1. Make sure `dev` is current. If `dev` doesn't exist yet, create it from `main` and push it:
   ```sh
   git fetch origin
   git show-ref --verify --quiet refs/remotes/origin/dev \
     || (git branch dev origin/main && git push -u origin dev)
   ```
2. Create a worktree with a branch off `origin/dev`, using a prefix for the kind of work:
   - `feat/<short-name>` for features
   - `fix/<short-name>` for bug fixes
   - `chore/<short-name>` for chores (deps, config, docs, cleanup)
   ```sh
   git worktree add ../directory-saver-worktrees/<kind>-<short-name> -b <kind>/<short-name> origin/dev
   ```
3. Do the work inside that worktree. Before committing, run `cargo fmt`, `cargo clippy --all-targets`, and `cargo test`, and make sure they all pass.
4. Commit with a clear message (`feat: ...`, `fix: ...`, `chore: ...`).

## Integrate into `dev` automatically

Once the work is committed and checks pass, merge it into `dev` and push **without asking**, as long as there is no conflict:

```sh
git fetch origin
git rebase origin/dev            # in the worktree
cargo test                       # re-check after the rebase
git push origin HEAD:dev         # fast-forward dev
```

- If the rebase or the push hits a **conflict** (or the push is rejected as non-fast-forward and a re-rebase also conflicts), **stop**. Don't force-push and don't resolve non-trivial conflicts on your own. Report the conflicting files to the user and wait.
- Never force-push to `dev` or `main`.
- Never push to `main` directly. `main` is updated from `dev` only when the user asks.

After a successful push, remove the worktree and its local branch:

```sh
git worktree remove ../directory-saver-worktrees/<kind>-<short-name>
git branch -d <kind>/<short-name>
```
