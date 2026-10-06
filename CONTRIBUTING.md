# Contributing to J-Jump

Thanks for helping. Bug reports, ideas and pull requests are all welcome.

## Report a bug or suggest an idea

Open a [GitHub issue](https://github.com/mreasonyang/j-jump/issues). For bugs, include:

- `jjump --version`, your OS and CPU, and your shell and its version
- the command you ran, what you expected and what happened
- the output of `jjump doctor` (it's offline, and hides private paths and keys)

Please don't paste API keys, `jjump config show --json` output (it contains your excluded paths) or your visit history.

## Before a larger change

Open an issue first for new features, new providers or behavior changes, so we can agree on the approach before you
spend time on it. Small fixes and documentation improvements can go straight to a pull request.

## Build and test

You need Rust 1.88+ with `rustfmt` and `clippy` (for example via [rustup](https://rustup.rs)), a C toolchain and
Python 3.9+. Bash and Zsh are needed for the shell tests; Fish and fzf are optional, and their tests are skipped when
missing.

```sh
git clone https://github.com/<you>/j-jump.git && cd j-jump
./scripts/install-hooks.sh          # once per clone; the repository checks expect it
cargo build --locked --release      # binary at target/release/jjump
./scripts/test-product.sh           # formatting, clippy, Rust and Python tests, repository checks
```

To try your build without touching your real settings or history, give it its own state folder:

```sh
export J_JUMP_HOME="$(mktemp -d)"
eval "$(./target/release/jjump init bash)"
```

`J_JUMP_HOME` doesn't isolate the OS credential store, so don't run credential commands against a real key while testing.

## Pull requests

- Commit on your fork's `main` branch: the installed pre-push hook only allows pushing `main`. Then open a pull request.
- Keep each pull request to one change, with tests for new behavior.
- Run `./scripts/test-product.sh` and `git diff --check` before pushing.
- Update the [README](README.md), its [Chinese version](README.zh-CN.md) or the [configuration guide](docs/CONFIGURATION.md)
  when user-facing behavior changes.
- Leave `VERSION` alone; the maintainer sets it when preparing a release.

## Project rules

These keep J-Jump safe and predictable, so pull requests that break them can't be merged:

- Local navigation must work with no network and no key.
- A semantic provider only suggests; the user always picks, and J-Jump never falls back to another provider on its own.
- Model output and unquoted paths are never executed as shell code.
- File contents, Git remotes, environment variables, shell history and credentials are never sent to a provider.
- One current implementation: no migrations for old file formats.
- Tests use synthetic profiles and keys, never a real user's data or credentials.

The maintainer's full development workflow is in [AGENTS.md](AGENTS.md).

## License

By contributing, you agree that your contributions are licensed under the project's [MIT License](LICENSE.md).
