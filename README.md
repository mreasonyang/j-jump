# J-Jump

Independent Rust directory navigation for Bash, Zsh and Fish. Local navigation needs no zoxide, fzf, Python, network or background service. The interface is CLI and optional terminal pickers only.

Current source: **0.0.31**, defined by [VERSION](VERSION). Automatic semantic navigation remains disabled. Public release archives are not yet available; see [release tooling and platform limits](docs/RELEASE.md).

## Install and activate

No prebuilt archives are published yet, so build one from a source checkout. This needs Rust 1.88+ (for example via [rustup](https://rustup.rs)), a C toolchain and Python 3:

```sh
git clone https://github.com/mreasonyang/j-jump.git && cd j-jump
cargo build --locked --release
python3 scripts/package.py --binary target/release/jjump --target "$(rustc -vV | sed -n 's/^host: //p')" --dist dist
cd dist && sha256sum -c j-jump-*.tar.gz.sha256     # macOS: shasum -a 256 -c ...
tar -xzf j-jump-*.tar.gz && cd j-jump-*/
./install.sh --prefix "$HOME/.local"
```

With an archive from elsewhere, verify and extract it the same way and run its `install.sh`.

The root [download installer](install.sh) is also prepared: it detects macOS/Linux and Intel/ARM64, downloads the
latest or a specified release, verifies SHA256, and uses the archive installer. It needs standard system tools and curl,
without Rust, Python or jq. Its default repository has no public release yet; use it once a public release destination
is available. Options, replacement and platform limits are described in [download installation](docs/RELEASE.md#download-installer).

Choose the commands for your current shell:

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

`jjump` and `j-jump` are equivalent management commands; navigation uses `j` and `ji`. The installer does not edit shell startup files. To keep `j`/`ji` in new terminals, add your shell's init line to its startup file: `~/.bashrc` (Bash), `~/.zshrc` (Zsh) or `~/.config/fish/config.fish` (Fish). Existing `j`/`ji` commands cause initialization to refuse; `jjump init zsh --cmd jump` instead provides `jump`/`jumpi`. Repeated initialization does not duplicate hooks.

The first terminal `j` or `ji` opens setup if the selected profile has no configuration. Completing the wizard saves and continues navigation; cancellation discards the draft and retries next time. Local-only setup needs no key. Use `jjump setup` to edit settings later. Help, completion and redirected commands do not open first-use setup.

A source build needs Rust1.88+ and a C toolchain: `cargo build --locked --release`. SQLite and TLS are embedded. Linux OS credential storage additionally needs a Secret Service session; environment credentials and local navigation do not. [Package installation, replacement and removal](packaging/README.md).

## Navigate

```sh
j                         # HOME
j -                       # native shell previous directory
j ../project              # direct path
j -- 'directory with spaces'
j api server              # ordered keywords across visited paths
j api /                   # descendants of the current logical directory
ji                        # local numbered picker
ji '后端服务'              # optional Jev suggestion, then explicit selection
# Type `j api ` then Tab to select into the editable command line.
```

The initial visit inventory is empty. Ordinary `cd` builds it at interactive prompt boundaries; repeated prompts, failed cd and subshells do not add observations. Direct paths remain available if optional config/store lookup is broken. `j --help` and `ji --help` leave the directory unchanged.

Ordinary `j` uses a valid local keyword match immediately, with zero model calls. Only a genuine no-match may reach Jev. Queried `ji`/Space-Tab can request Jev when enabled; bare pickers and empty completion stay local. `--offline` explicitly selects local behavior. Semantic suggestions always require selection; cancellation/EOF never chooses the first result.

Local matches rank by directory-name exact match, prefix, substring, then ancestor match. Within each tier, visit weight decays with a seven-day half-life; lifetime counts are separate. Ties use last-visit time and path bytes. This is a fixed initial policy, not proven optimal for every workflow.

The default picker is numbered text, with n/p pages, v N full-path inspection and empty/q cancellation. Explicit `J_JUMP_PICKER=fzf` enables local fuzzy filtering with user-installed fzf; missing/failed fzf reports an error without switching pickers. fzf defaults/commands are ignored and preview/execute extensions are disabled. No extra provider requests occur while filtering.

Space-Tab only edits the line; Enter executes it. Bash requires the terminal's standard device-status reply. A terminal that does not reply leaves the original line unchanged; cancel it and use `ji`. UTF-8 completion is tested on Bash3.2 and5.2; this does not establish every terminal's compatibility.

## Optional Jev and privacy

Defaults are semantic **off**, consent **ask**, disclosure **strict**. There is no request-count setting or cumulative request cap. A credential alone never enables requests.

```sh
jjump setup                    # hidden key entry and settings
jjump config show              # readable status
jjump doctor                   # offline checks and next steps
jjump preview 'backend'        # exact outgoing JSON; no request
jjump explain api              # readable local ranking; no request
jjump explain --json api       # machine-readable ranking
jjump history prune            # preview confirmed missing visits
jjump history prune --apply    # remove previewed missing visits
jjump --offline query --interactive  # explicitly browse local history
```

Setup keeps a new key in memory until saving to the OS store. `TYPESAFE_API_KEY` takes priority over that entry; keys never belong in command arguments or config. `jjump credential status` does not unlock or display the key. `credential delete --apply` removes only J-Jump's OS entry, not environment values or the provider account key.

Strict disclosure sends query and candidate names; balanced adds limited context/usage buckets; full permits parent/cwd paths. Names and queries can themselves be sensitive. File contents, Git remotes, environment and shell history are never added to requests. Excluded folders are not recorded/searched. `no_send` folders remain locally usable but their directory information is not sent; while inside one, Jev is disabled.

Jev uses one private, lazy adapter and the default system/environment network path. There is no direct fallback or automatic retry. The interactive deadline is10 seconds with a3-second connection cap, excluding time answering consent. During a pending request, Enter opens local choices; it does not select a directory. W continues within the same deadline. Failure, timeout and abstention stop without selecting a path. A sent request may still finish remotely after cancellation.

`jjump adapter status|stop|restart` manages the local adapter; restart is lazy and idle exit is five minutes. It is not a login service. Forced semantic routing is opt-in (`semantic_route=force` or `--force-semantic`), still requires selection and fails closed without a terminal. Laya is research-only. [Full configuration and recovery guide](docs/CONFIGURATION-AND-HELP.md).

## State and recovery

Current formats only: config3, visits4, backup3, cache3. Old/unknown formats are refused unchanged, with no migration or downgrade path. `J_JUMP_HOME` redirects configuration/data/cache files; it **does not isolate the shared OS credential entry**. `--config` overrides `J_JUMP_CONFIG`, then the default configuration path. Only absolute private local state paths are supported.

```sh
jjump config set tracking off
jjump config set no_send '["/work/private-client"]'
jjump history list
jjump history clear --preview
jjump history backup /absolute/private-backup/visits.json
jjump history restore /absolute/private-backup/visits.json  # validation only
jjump cache clear --preview
jjump config reset --preview
jjump config recover           # preview corrupt-config recovery
```

Destructive commands require `--apply`; restore also requires it to replace visits. Reset preserves tracking/exclusions, visits and credentials while disabling semantic networking. Recovery retains visits/credentials but disables tracking and networking; re-enter exclusions before enabling tracking. Cache clearing preserves visits and credentials. No command erases shell/third-party history or guarantees secure erasure of backups/storage remnants. Human config output hides private roots; `config show --json` includes them and should be reviewed before sharing.

## Development

Use [the documentation index](docs/README.md) for installation, configuration and release tooling. Contributors should read [development instructions](AGENTS.md). Design drafts and internal delivery records are kept outside the repository.

`./scripts/test-product.sh` runs local checks. Target support requires tests against the exact installed package, not compilation alone. Unsigned archives carry source/binary hashes and a dependency/license inventory. A guarded manual-only release workflow, Homebrew Formula generator and platform-detecting download installer are prepared; hosted execution and public distribution are deferred. Ordinary pushes do not start Actions. See [release preparation](docs/RELEASE.md). Live service quality, genuine user utility and the full platform matrix remain open gates.

## License

J-Jump is distributed under the [MIT License](LICENSE.md). Dependency licenses and notices are included with generated archives.
