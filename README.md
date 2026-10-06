<div align="center">

# J-Jump

**Jump to any folder by name, by meaning or in any language, straight from your shell.**

[![Latest release](https://img.shields.io/github/v/release/mreasonyang/j-jump?sort=semver)](https://github.com/mreasonyang/j-jump/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)
![Platforms: macOS | Linux](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey)
![Shells: Bash | Zsh | Fish](https://img.shields.io/badge/shell-bash%20%7C%20zsh%20%7C%20fish-4EAA25)

**English** · [简体中文](README.zh-CN.md)

</div>

<p align="center"><img src=".github/assets/demo.gif" alt="J-Jump demo: j pay jumps by name; ji my cv finds resumes with Jev; ji 税务 finds taxes" width="760"></p>

You never typed "resumes" or "taxes". J-Jump remembers the folders you visit, takes you to the right one by a few
letters of its name and, with an optional semantic model, finds it by what it means, even in another language.

## Why J-Jump

- **By name, instantly.** `j pay` jumps to the best match among folders you've actually visited. Ranking happens on
  your machine and uses no network.
- **By meaning, with a model you choose.** Can't remember the name? `ji my cv` asks a semantic model which folder you
  mean: Jev or Cloudflare Clef-Flash in the cloud, or Tev1 running entirely on your own computer.
- **In any language.** `ji 税务` finds `taxes`; `ji machine learning experiments` finds `机器学习实验`.
- **You stay in control.** Semantic help is off until you turn it on. It only suggests: nothing moves until you pick.
  In the default strict mode, the model receives only your query and folder names.
- **Small and native.** One Rust binary for Bash, Zsh and Fish on macOS and Linux. No runtime or plugin manager, and
  no background service for local navigation.

## Quick start

**1. Install.** Either command also connects your shell.

```sh
# Homebrew
brew install mreasonyang/taps/j-jump && jjump shell install

# Or the install script (macOS 15+ or Linux, x86-64 or ARM64)
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh
```

The install script picks the right build, verifies its SHA-256 checksum and installs `jjump` (plus the equivalent
`j-jump`) into `~/.local/bin`, without sudo. Shell setup detects Bash, Zsh or Fish, backs up your startup file and adds
one clearly marked block, including the `PATH` entry. Pass `--no-shell` to the script to skip it.
[Script options](docs/RELEASE.md#download-installer) · [Shell options](packaging/README.md)

**2. Open a new terminal.** Keep using `cd` as usual: J-Jump starts empty and learns each folder you visit.

**3. Run `j` or `ji`.** The first run opens a short setup wizard. Choose `off` to stay local-only (no key needed),
or pick a semantic provider now. Change anything later with `jjump setup`.

> Already use `j` for something else? `jjump shell install --cmd jump` gives you `jump` and `jumpi` instead.
> J-Jump never overwrites existing `j`/`ji` commands.

## Everyday use

| Command | What it does |
| --- | --- |
| `j pay` | Jump to the best visited folder matching `pay` |
| `j api server` | Match several words in order along the path |
| `j api /` | Only search inside the current folder |
| `j` | Go to your home folder |
| `j -` | Go back to the previous folder |
| `j ../dir`, `j -- 'my folder'` | Plain paths work too |
| `ji` | Browse your history in a numbered list |
| `ji pay` | Choose from matches; asks your semantic provider first when it's on |
| `j pay ` then <kbd>Tab</kbd> | Choose a match into the command line, then press <kbd>Enter</kbd> to go |

**How matches are ranked:** exact folder name, then prefix, then substring, then a match on a parent folder. Within each
tier, folders you visit often and recently come first; visit weight halves every seven days. `jjump explain pay` shows
the ranking.

**In the picker:** type a number to jump, `n`/`p` to change page, `v 3` to see a full path, and Enter or `q` to cancel.
For fuzzy filtering, install [fzf](https://github.com/junegunn/fzf) and set `export J_JUMP_PICKER=fzf`.

## Find folders by meaning

Semantic help is **off by default**, and local navigation never needs it. Pick one provider:

| Provider | Runs | You need | Available in |
| --- | --- | --- | --- |
| `jev` (default) | Cloud | A Jev API key | Release 0.0.38 |
| `clef-flash` | Cloud ([Cloudflare Workers AI](https://developers.cloudflare.com/workers-ai/models/clef-flash/)) | A Cloudflare Account ID and Workers AI API token | Release 0.0.38 |
| `tev1` | Your computer, via [Ollama](https://ollama.com/download) | Ollama 0.35+ and a ~4.5 GB model; no key | Source 0.0.40, not yet released |

The easiest way to set one up is `jjump setup`. It walks you through choosing the provider and entering its key or
local model, then turns semantic help on. Keys are typed hidden and saved to your OS credential store, never to a file;
each provider has its own entry.

**How it behaves:**

- `j pay` with a local match jumps immediately and never contacts a provider. Only when nothing matches may it ask.
- `ji QUERY` and <kbd>Tab</kbd> completion with a query ask the provider. A bare `ji`, `--offline` and
  `J_JUMP_OFFLINE=1` stay local.
- The suggestion appears first in the list, labelled with the provider's name. You still choose; cancelling never
  picks anything.
- By default (`consent ask`) J-Jump asks before each request; `jjump config set consent always` stops asking. Cloud
  services may charge per request.
- A request waits up to 10 seconds. Press Enter to switch to local choices or `W` to keep waiting. Errors, timeouts
  and "not sure" answers leave you where you are. J-Jump never falls back to another provider on its own.

<details>
<summary><b>Jev without the wizard</b></summary>

```sh
jjump config set provider jev
export TYPESAFE_API_KEY="your-jev-key"     # or store it: jjump credential set
jjump config set semantic on
```

The environment variable takes priority over the stored key. A key alone never turns requests on.

</details>

<details>
<summary><b>Cloudflare Clef-Flash without the wizard</b></summary>

```sh
jjump config set provider clef-flash
export CLOUDFLARE_ACCOUNT_ID="your-32-character-account-id"
export CLOUDFLARE_AUTH_TOKEN="your-workers-ai-api-token"
jjump config set semantic on
```

- `CLOUDFLARE_API_TOKEN` works as an alias when `CLOUDFLARE_AUTH_TOKEN` is unset or empty.
- `CLOUDFLARE_ACCOUNT_ID` overrides the saved `cloudflare_account_id`. An invalid value blocks requests until you fix
  or unset it.
- `jjump doctor` checks the format of these values locally. It doesn't test API permissions or send a request.

</details>

<details>
<summary><b>Local Tev1 4B (source 0.0.40)</b></summary>

Tev1 runs on your computer through Ollama. J-Jump never installs or starts Ollama and never downloads models.

```sh
# 1. Install Ollama 0.35+ and open the app (or keep `ollama serve` running), then:
ollama pull tev1:4b-q8_0
# 2. Choose tev1 in the wizard; type `check` at the connection step to test it.
jjump setup
```

Or configure it directly:

```sh
jjump config set provider tev1
jjump config set ollama_url http://127.0.0.1:11434
jjump config set ollama_model tev1:4b-q8_0
jjump provider-check --models      # list installed compatible models without loading one
jjump provider-check               # load the model and send one synthetic test request
jjump config set semantic on
```

- Supported [GGUF tags](https://ollama.com/library/tev1/tags): `tev1:4b-q8_0` (recommended), `tev1:4b-q4_K_M`,
  `tev1:4b-bf16` and existing `tev1:4b` installs. MLX/Safetensors builds don't work.
- Only loopback addresses are used, with no proxy. For another port, start Ollama with
  `env OLLAMA_HOST=127.0.0.1:11439 ollama serve` and set the same URL in J-Jump.
- The 10-second deadline includes loading the model, so the first request after a cold start may need a retry. Ollama
  keeps the model in memory for five minutes after each request.
- Tev1 sees at most 23 folder groups per question, so the folder you want may not be among them. Its answers aren't
  cached between runs.
- Setup only contacts Ollama when you type `list` or `check`; `doctor`, `preview` and status stay offline.

</details>

### What the provider can see

| `privacy` setting | Sent to the provider |
| --- | --- |
| `strict` (default) | Your query and candidate folder names |
| `balanced` | Plus each folder's parent name, the current folder's name and a low/medium/high visit level |
| `full` | Like `balanced`, but with full parent and current folder paths |

File contents, Git remotes, environment variables, shell history and credentials are **never** sent, and the same
rules apply to local Tev1. Folder names and queries can themselves be sensitive, so you have more controls:

- `jjump preview "my cv"` prints the exact request without sending anything.
- `no_send` folders still work locally but are never sent, and semantic requests are off while you're inside them.
- `exclude` folders are never recorded or searched at all.

```sh
jjump config set no_send '["/work/private-client"]'
jjump config set exclude '["/work/scratch"]'
```

## Configuration

Use the interactive `jjump setup`, or `jjump config set KEY VALUE`. `jjump config show` prints current values, and
`jjump doctor` runs offline checks with next steps.

| Key | Values | Default |
| --- | --- | --- |
| `semantic` | `on`, `off` | `off` |
| `provider` | `jev`, `clef-flash`, `tev1` | `jev` |
| `consent` | `ask`, `always` | `ask` |
| `privacy` | `strict`, `balanced`, `full` | `strict` |
| `tracking` | `on`, `off` | `on` |
| `exclude`, `no_send` | JSON array of absolute paths | `[]` |
| `language` | `auto`, `en`, `zh` | `auto` |
| `semantic_route` | `local_first`, `force` (always ask the provider; you still choose) | `local_first` |
| `candidate_limit` | `1`–`254` folder groups offered to a cloud provider | `254` |
| `cloudflare_account_id` | 32 hexadecimal characters | empty |
| `ollama_url` | Loopback HTTP address | `http://127.0.0.1:11434` |
| `ollama_model` | A supported Tev1 4B tag | `tev1:4b-q8_0` |

| Environment variable | Effect |
| --- | --- |
| `TYPESAFE_API_KEY` | Jev key; overrides the stored Jev key |
| `CLOUDFLARE_AUTH_TOKEN`, `CLOUDFLARE_API_TOKEN` | Workers AI token; overrides the stored Cloudflare token |
| `CLOUDFLARE_ACCOUNT_ID` | Overrides the saved Cloudflare Account ID |
| `J_JUMP_PICKER` | `fzf` or `numbered` (default) |
| `J_JUMP_OFFLINE=1` | Keep this command local |
| `J_JUMP_LANG` | `en` or `zh` messages |
| `J_JUMP_CONFIG` | Use another config file (`--config` takes priority) |
| `J_JUMP_HOME` | Keep config, history and cache under another absolute folder (OS credential entries are still shared) |

Files live in `~/Library/Application Support/j-jump` and `~/Library/Caches/j-jump` on macOS, and in the XDG folders
`~/.config/j-jump`, `~/.local/share/j-jump` and `~/.cache/j-jump` on Linux.

## Your history and data

```sh
jjump history list
jjump history forget --preview -- /work/old-project
jjump history prune                 # preview visits to folders that no longer exist
jjump config set tracking off       # pause recording
mkdir -m 700 ~/jjump-backup && jjump history backup ~/jjump-backup/visits.json
jjump history restore ~/jjump-backup/visits.json    # validates; add --apply to replace
jjump history clear --preview
jjump data clear --preview          # history and cached semantic answers
jjump credential delete             # remove the current provider's stored key (preview)
```

Anything that deletes or replaces data shows a preview first; add `--apply` to do it. Backups must sit in a private
(`chmod 700`) folder. Deleting is not secure erasure.

## Troubleshooting

| Problem | Fix |
| --- | --- |
| `j: command not found` after installing | Open a new terminal. If it persists, run `jjump shell install` and follow what it reports. |
| `j foo` finds nothing | J-Jump only knows folders visited since you installed it. `cd` there once, or check with `jjump explain foo`. |
| `j` or `ji` is already taken | `jjump shell install --cmd jump` gives you `jump` and `jumpi`. |
| <kbd>Tab</kbd> doesn't insert a choice in Bash | Your terminal didn't answer Bash's cursor query; the line is left as it was. Use `ji foo` instead. |
| Linux can't save a key | The OS credential store needs a Secret Service session, such as GNOME Keyring. Or use the provider's environment variable. |
| Tev1 doesn't answer | Make sure Ollama is running and the model is pulled, then run `jjump provider-check`. A cold start may need a retry. |
| An error mentions an old or unknown format | J-Jump is pre-1.0 and reads only its current state formats. Your files are left untouched; `jjump doctor` shows the remedy. |
| Anything else | Run `jjump doctor`. It's offline and suggests next steps. |

Exit codes for scripts: `2` input, `3` no match, `4` selection required, `5` provider, `6` path, `7` state, `130` cancelled.

## Update and uninstall

```sh
# Update
brew upgrade mreasonyang/taps/j-jump                                                         # Homebrew
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh -s -- --replace   # install script

# Uninstall: remove the shell block first, then the binary
jjump shell uninstall
brew uninstall mreasonyang/taps/j-jump                                                       # Homebrew
```

For an install-script installation, run `install.sh --uninstall` from the
[release archive](packaging/README.md#replace-recover-and-remove) after `jjump shell uninstall`. Your settings, history
and stored keys stay; clear them first with the commands above if you want them gone. `jjump shell uninstall` removes
only unchanged J-Jump blocks; remove any `jjump init` lines you added by hand yourself.

## Platforms

| System | Install script | Homebrew |
| --- | --- | --- |
| macOS 15+ on Apple silicon | ✅ | ✅ |
| macOS 15+ on Intel | ✅ | ✅ |
| Linux x86-64 (static musl build) | ✅ | ✅ |
| Linux ARM64 (static musl build) | ✅ | ✅ |

Release 0.0.38 passed native tests on all four systems through both install routes, including Bash, Zsh and Fish
setup. Binaries are unsigned (no macOS notarization); installers check SHA-256 checksums instead. Windows, 32-bit
systems and macOS before 15 aren't supported. Details: [release status and platform limits](docs/RELEASE.md).

<details>
<summary><b>Build from source</b></summary>

You need Rust 1.88+ (for example via [rustup](https://rustup.rs)), a C toolchain and Python 3. SQLite and TLS are built
in. Until the next release, building from source is how to try Tev1.

```sh
git clone https://github.com/mreasonyang/j-jump.git && cd j-jump
cargo build --locked --release
python3 scripts/package.py --binary target/release/jjump --target "$(rustc -vV | sed -n 's/^host: //p')" --dist dist
cd dist && sha256sum -c j-jump-*.tar.gz.sha256     # macOS: shasum -a 256 -c ...
tar -xzf j-jump-*.tar.gz && cd j-jump-*/
./install.sh --prefix "$HOME/.local"
```

The archive installer connects your shell too (pass `--no-shell` to skip). See
[archive installation](packaging/README.md) for replacing and removing an installation.

</details>

## Documentation

- [Configuration, privacy and recovery guide](docs/CONFIGURATION-AND-HELP.md) (in Chinese)
- [Install script, releases and platform limits](docs/RELEASE.md)
- [Archive installation, shell integration and removal](packaging/README.md)
- [Documentation index](docs/README.md)

## Contributing

Bug reports and ideas are welcome in [GitHub Issues](https://github.com/mreasonyang/j-jump/issues). Before changing
code, read the [development instructions](AGENTS.md) and run `./scripts/test-product.sh`.

## License

[MIT](LICENSE.md). Release archives include the licenses and notices of all dependencies.
