# Archive installation

J-Jump provides independent Rust navigation for Bash, Zsh and Fish. `jjump` and `j-jump` are equivalent management entries; j/ji are shell navigation functions. Local operation needs no zoxide, fzf, Python or network. Archives are unsigned; target acceptance is separate from compilation.

## Verify, extract and install

Choose an archive matching your OS/architecture. In the download directory, verify its companion checksum with `sha256sum -c ARCHIVE.tar.gz.sha256` on Linux or `shasum -a 256 -c ARCHIVE.tar.gz.sha256` on macOS. Substitute the actual archive filename. Extract with `tar -xzf ARCHIVE.tar.gz`, enter its extracted directory and run:

```sh
./install.sh --prefix "$HOME/.local"
```

From version 0.0.34, the archive installer connects your login shell automatically, including the binary directory on
PATH. Open a new terminal to use `j` and `ji`. It identifies the shell from `SHELL`; use `--shell bash`, `--shell zsh` or
`--shell fish` to override it. To install only the binaries, pass `--no-shell`.

With an installed 0.0.34+ binary, including one installed through Homebrew, you can connect or undo the integration:

```sh
jjump shell install
jjump shell install --shell zsh --cmd jump
jjump shell install --shell zsh --rc /absolute/startup-file
jjump shell uninstall
```

For manual integration, add the binary directory to PATH and activate your shell:

```sh
# Bash
export PATH="$HOME/.local/bin:$PATH"
eval "$(jjump init bash)"
# Zsh
export PATH="$HOME/.local/bin:$PATH"
eval "$(jjump init zsh)"
# Fish
set -gx PATH "$HOME/.local/bin" $PATH
jjump init fish | source
```

Automatic integration backs up existing files and adds a marked block without replacing your other settings. Repeating
the same installation leaves the startup files unchanged. Existing manual `jjump init` lines are preserved. Known j/ji
aliases, functions or executable conflicts require another prefix such as `--cmd jump`; initialization also protects
commands defined dynamically by other startup scripts. Startup files are never executed by the installer.

Zsh uses `$ZDOTDIR/.zshrc` or `~/.zshrc`; Fish uses `$XDG_CONFIG_HOME/fish/config.fish` or
`~/.config/fish/config.fish`. Bash uses `~/.bashrc` and connects the first existing login profile (`.bash_profile`,
`.bash_login`, `.profile`), creating `.bash_profile` only if none exists. Use `--rc` for a custom startup file; that
updates only the selected file. Dotfile symlinks are retained while the real file is backed up and edited. Backups
appear beside the edited file as `FILE.j-jump-backup-*`, with access limited to the current user. Edited managed blocks
are preserved and reported for manual inspection. `jjump init` itself still only prints code and never edits files.

If shell integration fails, the binary remains installed and the installer returns an error with a remedy. Fix the
reported shell, command prefix or startup-file issue and run `jjump shell install` again. Shell integration needs no
network, product profile or credentials and does not enable Jev.

First terminal j/ji opens setup when configuration is absent. Completing first setup saves and resumes navigation; cancellation saves nothing and retries later. Reopen `jjump setup` for changes. Local-only mode needs no key. Ordinary cd builds visits at prompt boundaries; j goes HOME, j - uses native previous and j words ranks visited directories. Bare ji opens a local picker. Space-Tab only edits the command line. Bash needs a terminal device-status reply for insertion; an unsupported terminal leaves the line unchanged.

## Settings and diagnostics

Jev defaults off; consent defaults ask and fields strict. A key does not enable networking. Hidden setup input is saved to the OS store on completion; TYPESAFE_API_KEY takes precedence. Linux OS storage needs a Secret Service session. There is no plaintext fallback or cumulative request quota. J_JUMP_HOME isolates file state, not the shared OS credential entry.

Use `jjump config show`, `jjump doctor`, `jjump preview QUERY` and subcommand `--help`. Preview/doctor are offline. `--json` supports automation, but config JSON includes private root paths; review it before sharing. Display language is auto/en/zh through `config set language` or J_JUMP_LANG. Technical tokens and Jev wait text stay unchanged.

The numbered picker is default; n/p change pages and v N shows a full path. Explicit J_JUMP_PICKER=fzf enables local filtering with installed fzf; failures do not switch pickers. fzf defaults/commands are ignored; preview/execute are disabled. Filtering makes no extra provider requests.

Only the lazy private adapter sends Jev requests. It follows system/environment proxy settings, has a10-second interactive deadline/3-second connection cap, and never retries through direct transport. Enter during a pending request opens a local picker and still requires selection; W keeps waiting within the same deadline. Errors/timeouts/abstention stop without choosing a path. A sent request may finish remotely after cancellation. `jjump adapter status`, stop and restart manage the local process; restart is lazy. There is no login service or automatic semantic navigation.

## Replace, recover and remove

From the new extracted archive, run `./install.sh --prefix "$HOME/.local" --replace`. Replacement requires receipt-owned unchanged commands. Foreign/retargeted aliases or unowned executables are refused; a missing managed alias is repairable. An interrupted-install marker supports recovery across atomic renames. Only the current binary is retained, with j-jump as its equivalent entry; jj is not installed.

Run `jjump shell uninstall` before removing the binary to remove unchanged managed blocks, then run
`./install.sh --prefix "$HOME/.local" --uninstall` to remove owned commands, receipt and marker. Configuration, visits,
credentials, backups and your other shell settings remain. Use the same `--shell` and `--rc` used for connection if
needed. Manual init lines remain yours to remove. If the binary is removed first, its managed startup block checks for
the executable and stays inactive. Start a fresh shell afterwards.

Only current config3/visits4/backup3/cache3 are accepted; old/unknown formats remain unchanged and are rejected. No migration or downgrade binary is supplied. State clear/reset/recover/restore commands preview unless explicitly applied, except backup creation. Config reset retains tracking/exclusion policy and data while disabling semantic networking. Corrupt-config recovery also disables tracking; restore exclusions before re-enabling it. No action erases shell/third-party history or guarantees secure erasure of storage remnants.

## Provenance and acceptance

The archive contains manifest.json with source SHA/target/binary hash, binary.sha256, dependencies.json schema2, licenses/ and LICENSE.md. The dependency inventory records the resolved target graph and collected licence texts; missing texts are named explicitly in the inventory and licenses/MISSING.txt. Hashes establish identity, not signing/notarization or correctness.

A source README update does not change an already-built archive's bytes. The public source is [mreasonyang/j-jump](https://github.com/mreasonyang/j-jump). [Release 0.0.34](https://github.com/mreasonyang/j-jump/releases/tag/v0.0.34) provides all four accepted native archives. The public Homebrew channel is `brew install mreasonyang/taps/j-jump`; see [release acceptance and platform limits](../docs/RELEASE.md).

Installer receipts live in `share/j-jump-install`, separate from private data. Installing the same unchanged archive succeeds without rewriting binaries. Uninstall stops verified same-profile helpers, keeps unrelated profiles and user state, and removes only receipt-owned current names. Unknown previous executables are preserved because current-only installations do not create or adopt them.

The source repository also provides a platform-detecting download entry at its root `install.sh`. It downloads and
verifies a matching public release before calling this archive installer. All four native Actions builds exercise latest and explicit-version installation through a local TLS downloader fixture, with the same state-preserving removal contract. Public archive downloads are separately verified against the accepted bundle.
