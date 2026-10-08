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

> [!NOTE]
> J-Jump is young (version 0.0.x). Expect rough edges, and expect upgrades to occasionally need a one-time fix to your
> settings or history; `jjump doctor` tells you exactly what to do. Feedback is very welcome in
> [Issues](https://github.com/mreasonyang/j-jump/issues).

## Why J-Jump

- **By name, instantly.** `j pay` jumps to the best match among folders you've actually visited. Ranking happens on
  your machine and uses no network.
- **By meaning, with a model you choose.** Can't remember the name? `ji my cv` asks a semantic model which folder you
  mean: Jev, Cloudflare Clef-Flash or OpenAI in the cloud, or Tev1 running entirely on your own computer.
- **In any language.** `ji 税务` finds `taxes`; `ji machine learning experiments` finds `机器学习实验`.
- **You stay in control.** Semantic help is off until you turn it on. It only suggests: nothing moves until you pick.
  By default the model sees only your query and folder names.
- **Small and native.** One Rust binary for Bash, Zsh and Fish on macOS and Linux. No runtime, no plugin manager and no
  background service for local navigation.

## Quick start

**1. Install.** Either command also connects your shell.

```sh
# Homebrew
brew install mreasonyang/taps/j-jump && jjump shell install

# Or the install script (macOS 15+ or Linux, x86-64 or ARM64)
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh
```

The script installs `jjump` into `~/.local/bin` after checking its SHA-256 checksum, with no sudo. Shell setup backs up
your startup file and adds one marked block; pass `--no-shell` to skip it. [More options](docs/CONFIGURATION.md#shell-integration).

**2. Open a new terminal** and keep using `cd` as usual. J-Jump starts empty and learns each folder you visit.

**3. Run `j` or `ji`.** The first run opens a short setup wizard. Choose `off` to stay local-only, which needs no key,
or pick a semantic provider. Change anything later with `jjump setup`.

> Already use `j` for something else? `jjump shell install --cmd jump` gives you `jump` and `jumpi` instead.

## Everyday use

| Command | What it does |
| --- | --- |
| `j pay` | Jump to the best visited folder matching `pay` |
| `j api server` | Match several words in order along the path |
| `j api /` | Only search inside the current folder |
| `j`, `j -` | Go home, or back to the previous folder |
| `j ../dir`, `j -- 'my folder'` | Plain paths work too |
| `ji` | Browse your history in a numbered list |
| `ji pay` | Choose from matches; asks your semantic provider first when it's on |
| `j pay ` then <kbd>Tab</kbd> | Put a match on the command line; <kbd>Enter</kbd> goes there |

Exact folder names rank first, then prefixes, substrings and parent-folder matches. Within each group, folders you visit
often and recently win. `jjump explain pay` shows why.

## Find folders by meaning

Semantic help is **off by default**. Choose one provider in `jjump setup`, which also asks for its key or local model:

| Provider | Runs | You need |
| --- | --- | --- |
| [Jev](https://docs.typesafe.ai/introduction) by TypeSafe AI (default) | Cloud | An API key from the [TypeSafe console](https://console.typesafe.ai/keys) |
| [Clef-Flash](https://developers.cloudflare.com/workers-ai/models/clef-flash/) on Cloudflare Workers AI | Cloud | A Cloudflare Account ID and Workers AI API token |
| [OpenAI Decisions](https://developers.openai.com/api/docs/guides/decisions) (`gpt-6-luna`) | Cloud | An OpenAI API key and J-Jump 0.0.42+ |
| Tev1 4B via [Ollama](https://ollama.com/download) | Your computer | Ollama 0.35+, a ~4.5 GB model and J-Jump 0.0.40+ |

- `j` only asks the provider when nothing matches locally. `ji QUERY` asks it whenever semantic help is on.
- The provider's pick is listed first and labelled. **You always choose**; nothing is selected for you.
- By default J-Jump asks before each request. Cloud providers may charge per request.
- In the default `strict` privacy mode, only your query and candidate folder names are sent. File contents, Git
  remotes, environment variables, shell history and keys are never sent. `jjump preview "my cv"` shows the exact request.
- Keep sensitive folders out with `no_send` (searchable locally, never sent) or `exclude` (never recorded).

Setup without the wizard, privacy levels and every setting: [configuration guide](docs/CONFIGURATION.md).

## Troubleshooting

- **`j: command not found`:** open a new terminal. If it persists, run `jjump shell install`.
- **`j foo` finds nothing:** J-Jump only knows folders you've visited since installing. `cd` there once.
- **Anything else:** run `jjump doctor`. It works offline and tells you what to do next.

More fixes and exit codes: [troubleshooting](docs/CONFIGURATION.md#troubleshooting).

## Update and uninstall

```sh
# Update
brew upgrade mreasonyang/taps/j-jump
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh -s -- --replace

# Uninstall: remove the shell block first, then the program
jjump shell uninstall
brew uninstall mreasonyang/taps/j-jump
```

If you used the install script, finish with `install.sh --uninstall` from the
[release archive](packaging/README.md#replace-recover-and-remove). Your settings, history and keys stay until you
[clear them](docs/CONFIGURATION.md#history-and-data).

## Platforms

Release builds are tested natively on macOS 15+ (Apple silicon and Intel) and Linux (x86-64 and ARM64), through both
Homebrew and the install script, with Bash, Zsh and Fish. Binaries are unsigned, and the installers verify SHA-256
checksums instead. Windows, 32-bit systems and older macOS aren't supported.
[Release status and platform limits](docs/RELEASE.md).

## Documentation

- [Configuration guide](docs/CONFIGURATION.md): settings, providers, privacy, data and troubleshooting
- [配置、帮助与恢复](docs/CONFIGURATION-AND-HELP.md): the same topics in Chinese
- [Install script and releases](docs/RELEASE.md)
- [Archive installation, shell integration and removal](packaging/README.md)

## Contributing

Bug reports, ideas and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) to build from source and run
the tests.

## License

[MIT](LICENSE.md). Release archives include the licenses and notices of all dependencies.
