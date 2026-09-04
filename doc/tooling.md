# Development tooling and process

Conventions for working on this repository. Nothing here is specific to a
particular editor or assistant: it describes what the project expects of a
change, so any contributor — or any tool acting on someone's behalf — can
follow the same process.

## Worktrees for feature work

Start every new feature in its own git worktree rather than moving the main
checkout onto a branch. The checkout at the repository root stays on `main`
and stays buildable, so work in progress never blocks a package build, a
hotfix, or a look at released code.

```sh
git worktree add ../acr-<feature> -b feat/<feature> origin/main
cd ../acr-<feature>
```

Each worktree has its own working directory but shares the object store, so
this costs a checkout, not a clone. Two worktrees cannot have the same branch
checked out at once, which is the point: it makes the "which branch am I on"
question disappear.

Note that `target/` is per-worktree. A fresh worktree recompiles from scratch
unless you point `CARGO_TARGET_DIR` at a shared directory.

Clean up once the branch is merged:

```sh
git worktree remove ../acr-<feature>
git branch -d feat/<feature>
git push origin --delete feat/<feature>
```

`git worktree list` shows what is currently checked out where; `git worktree
prune` clears entries whose directory was deleted by hand.

Single-commit fixes on an existing branch do not need their own worktree.
Anything that will take more than one commit does.

## Branches and commits

- Branch names are prefixed by kind: `feat/`, `fix/`, `docs/`, `chore/`.
- Commit subjects follow the conventional-commit style already in the log:
  `fix(events): emit shuffle_changed instead of random_changed`,
  `docs(websocket): correct the event contract to match the implementation`.
  The scope is optional; the type is not.
- The body explains *why*, with evidence. The diff already says what changed.
- Changes land through a pull request against `main`, merged with a merge
  commit so contributor authorship survives.
- Public text — commit messages, PR titles and bodies, issue comments, release
  notes — is written in the project's own voice: what changed and why. Not
  which tool was used to write it.

## Building and testing

The crate links against ALSA and D-Bus, so a build needs their development
headers:

```sh
apt-get install libasound2-dev libdbus-1-dev pkg-config
cargo test --workspace
```

**This does not build on macOS.** `alsa-sys` fails at the `pkg-config --libs
--cflags alsa` step, because ALSA is a Linux sound API with no macOS
equivalent. There is no Homebrew formula that fixes this. On a Mac, run the
test suite in a container:

```sh
docker run --rm \
  -v "$PWD":/w -w /w \
  -v acr-cargo-registry:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/w/.docker-target \
  rust:1.86-bookworm \
  sh -c 'apt-get update -qq \
      && apt-get install -y -qq libasound2-dev libdbus-1-dev pkg-config \
      && cargo test --workspace'
```

The named volume keeps the crates.io registry between runs and
`CARGO_TARGET_DIR` keeps build artifacts out of the host `target/`, so only
the first run pays the full compile. `rust:1.86-bookworm` matches the
`rust-version` in `Cargo.toml`; keep the two in step.

Tests live in inline `#[cfg(test)] mod tests` blocks next to the code they
cover. Run one module with `cargo test --lib players::mpd::mpd::tests`.

### The workspace

The repository is a Cargo workspace, not a single package. The root
`Cargo.toml` is both the workspace manifest and the `audiocontrol` package
(the player daemon, `src/`); `crates/audiocontrol-metadata` is the metadata
code; the five `crates/acr-*` crates (`acr-types`, `acr-http`, `acr-images`,
`acr-store`, `acr-web`) are shared by both.

**`cargo build` and `cargo test` on their own operate on the root package
only.** The workspace declares no `default-members`, so without `--workspace`
a plain build produces the `audiocontrol` binary and the ten tools that are
still `[[bin]]` targets of the root package — not the four that moved to
`crates/audiocontrol-metadata/src/bin/`, and not the shared crates' own test
suites. Always pass `--workspace` for a build or test run that is meant to
cover everything:

```sh
cargo build --release --workspace   # all fifteen audiocontrol* binaries
cargo test --workspace              # every crate's tests, not just the root package's
```

`cargo test -p acr-store` runs one crate on its own. The player library must
not depend on the metadata crate: `scripts/check-crate-deps.sh` fails the
build if it does, and CI runs it on every push. `src/main.rs` is the one file
that links both, behind the default `metadata` feature — `cargo build
--no-default-features --bin audiocontrol` builds the player daemon alone,
which is what that script also checks.

## Writing testable code

Much of this daemon talks to a player over a socket and cannot be unit tested
without one. Where a change has a decision in it — a mapping, a diff, a
threshold — put that decision in a plain function or small struct that takes
values and returns values, and keep the I/O in the caller. The decision then
gets unit tests, and the wiring gets checked against a real device.

`doc/specs/2026-08-28-mpd-event-emission.md` shows the split in practice:
mapping MPD's flags onto a loop mode and diffing successive observations are
pure and tested; querying MPD and emitting the events is not.

## Packaging

Debian packaging lives in `debian/`. `build.sh` drives an sbuild package
build; `DIST` selects the distribution. Cross-compilation setup is supplied by
the surrounding hifiberry-os build, not by this repository.

Two things to remember when touching `debian/postinst`:

- Files under `/etc/audiocontrol` are dpkg conffiles, some shipped by other
  packages. They stay `root:root`; the daemon does not own its own
  configuration.
- `postinst` runs on every upgrade. Anything that must happen once needs to be
  gated on the previously-installed version (`$2`).

## Specs

Design work that is worth agreeing on before it is written goes in
`doc/specs/` as `YYYY-MM-DD-<topic>.md`, carrying a `Status:` line (`Proposed`,
`Accepted`, `Implemented`). A spec states the problem, the evidence for it,
what is in and out of scope, and what the tests should be — so the
implementation can be reviewed against something.

## What review looks for

`.codereview.toml` at the repository root states what this project cares
about, and applies whether the reviewer is a person or a tool: panics on paths
reachable from the API or a player event, blocking work inside async, lock
ordering against the event bus, credential handling, event-vocabulary changes
without a compatibility path, and new behaviour with no test where the crate
already tests that area.

Read it before starting, not after. It is short, and it is the standard the
change will be measured against.
