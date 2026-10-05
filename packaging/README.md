# Archive installation

J-Jump provides independent Rust navigation for Bash, Zsh and Fish. `jjump` and `j-jump` are equivalent management entries; j/ji are shell navigation functions. Local operation needs no zoxide, fzf, Python or network. Archives are unsigned; target acceptance is separate from compilation.

## Verify, extract and install

Choose an archive matching your OS/architecture. In the download directory, verify its companion checksum with `sha256sum -c ARCHIVE.tar.gz.sha256` on Linux or `shasum -a 256 -c ARCHIVE.tar.gz.sha256` on macOS. Substitute the actual archive filename. Extract with `tar -xzf ARCHIVE.tar.gz`, enter its extracted directory and run:

```sh
./install.sh --prefix "$HOME/.local"
```

Put the prefix's bin directory on PATH and activate only your current shell:

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

The installer never edits shellrc. Initialization refuses existing j/ji conflicts; `--cmd jump` generates jump/jumpi instead. Add the matching init line to your startup file yourself if desired.

First terminal j/ji opens setup when configuration is absent. Completing first setup saves and resumes navigation; cancellation saves nothing and retries later. Reopen `jjump setup` for changes. Local-only mode needs no key. Ordinary cd builds visits at prompt boundaries; j goes HOME, j - uses native previous and j words ranks visited directories. Bare ji opens a local picker. Space-Tab only edits the command line. Bash needs a terminal device-status reply for insertion; an unsupported terminal leaves the line unchanged.

## Settings and diagnostics

Jev defaults off; consent defaults ask and fields strict. A key does not enable networking. Hidden setup input is saved to the OS store on completion; TYPESAFE_API_KEY takes precedence. Linux OS storage needs a Secret Service session. There is no plaintext fallback or cumulative request quota. J_JUMP_HOME isolates file state, not the shared OS credential entry.

Use `jjump config show`, `jjump doctor`, `jjump preview QUERY` and subcommand `--help`. Preview/doctor are offline. `--json` supports automation, but config JSON includes private root paths; review it before sharing. Display language is auto/en/zh through `config set language` or J_JUMP_LANG. Technical tokens and Jev wait text stay unchanged.

The numbered picker is default; n/p change pages and v N shows a full path. Explicit J_JUMP_PICKER=fzf enables local filtering with installed fzf; failures do not switch pickers. fzf defaults/commands are ignored; preview/execute are disabled. Filtering makes no extra provider requests.

Only the lazy private adapter sends Jev requests. It follows system/environment proxy settings, has a10-second interactive deadline/3-second connection cap, and never retries through direct transport. Enter during a pending request opens a local picker and still requires selection; W keeps waiting within the same deadline. Errors/timeouts/abstention stop without choosing a path. A sent request may finish remotely after cancellation. `jjump adapter status`, stop and restart manage the local process; restart is lazy. There is no login service or automatic semantic navigation.

## Replace, recover and remove

From the new extracted archive, run `./install.sh --prefix "$HOME/.local" --replace`. Replacement requires receipt-owned unchanged commands. Foreign/retargeted aliases or unowned executables are refused; a missing managed alias is repairable. An interrupted-install marker supports recovery across atomic renames. Only the current binary is retained, with j-jump as its equivalent entry; jj is not installed.

Run `./install.sh --prefix "$HOME/.local" --uninstall` to remove owned commands, receipt and marker. Configuration, visits, credentials, backups and shellrc remain. Remove your init line yourself and start a fresh shell.

Only current config3/visits4/backup3/cache3 are accepted; old/unknown formats remain unchanged and are rejected. No migration or downgrade binary is supplied. State clear/reset/recover/restore commands preview unless explicitly applied, except backup creation. Config reset retains tracking/exclusion policy and data while disabling semantic networking. Corrupt-config recovery also disables tracking; restore exclusions before re-enabling it. No action erases shell/third-party history or guarantees secure erasure of storage remnants.

## Provenance and acceptance

The archive contains manifest.json with source SHA/target/binary hash, binary.sha256, dependencies.json schema2, licenses/ and LICENSE.md. The dependency inventory records the resolved target graph and collected licence texts; missing texts are named explicitly in the inventory and licenses/MISSING.txt. Hashes establish identity, not signing/notarization or correctness.

A source README update does not change an already-built archive's bytes. The public source is [mreasonyang/j-jump](https://github.com/mreasonyang/j-jump). The current available installation path is this archive installer. A new Formula generator is prepared in the source repository, but its public Homebrew channel and hosted release execution are deferred.

Installer receipts live in `share/j-jump-install`, separate from private data. Installing the same unchanged archive succeeds without rewriting binaries. Uninstall stops verified same-profile helpers, keeps unrelated profiles and user state, and removes only receipt-owned current names. Unknown previous executables are preserved because current-only installations do not create or adopt them.

The source repository also prepares a platform-detecting download entry at its root `install.sh`. It downloads and
verifies a matching public release before calling this archive installer. Its public release destination is still
deferred; it does not change the local archive workflow or provide a working public download URL yet.
