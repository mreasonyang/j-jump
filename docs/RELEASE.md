# Release and Homebrew

The current source version and latest published release is [J-Jump 0.0.42](https://github.com/mreasonyang/j-jump/releases/tag/v0.0.42). It adds OpenAI Decisions as an optional semantic provider alongside Jev, Cloudflare Clef-Flash and local Ollama Tev1. Select `openai` in setup or configuration; its API key has a separate OS credential entry, and semantic requests remain opt-in. Suggestions still require explicit user selection.

All four native targets pass source and exact installed-package tests at immutable tag v0.0.42, source b88013126ca692d6f0bb1396b86b09c952ee424e, in [build run 38036285009](https://github.com/mreasonyang/j-jump/actions/runs/38036285009). ARM64 targets pass on the first attempt; both x86-64 targets pass on the unchanged second attempt. The original Intel macOS source test used a temporary parent containing `bl`, which also matched its query and invalidated the fixture's expected no-match result. The original Linux x86-64 installed Fish rapid-prompt test missed visits. Original failures are retained; neither source nor assertions changed for the repeats. Rapid prompt observation remains best effort: the hook waits at most four 5ms sleeps for an earlier live supervisor and leaves the current directory eligible for a later prompt when that guard remains busy.

| Target | Native build, installed product and TLS downloader lifecycle |
| --- | --- |
| macOS ARM64 | PASS |
| macOS x86-64 | PASS |
| Linux x86-64 musl | PASS |
| Linux ARM64 musl | PASS |

Each native job runs the source product suite and repeats it against the exact installed optimized archive, including the strict no-match performance regression. Synthetic homes cover Bash/Zsh/Fish startup, backups, exact undo, conflicts and repeated installation. Real curl and a local TLS fixture exercise the exact archive's downloader lifecycle. The complete source-bound bundle passes architecture, checksum, archive safety, neutral ownership, license inventory and payload disclosure validation. All nine public assets match after draft roundtrip; anonymous downloads of all four archives match the accepted bundle.

The [public Homebrew Formula](https://github.com/mreasonyang/homebrew-taps/blob/c8c7a93e1c07c7bba929d81068e717ea4aa670c7/j-jump.rb) is generated from this four-target bundle. [Native Homebrew acceptance 38057686928](https://github.com/mreasonyang/j-jump/actions/runs/38057686928) passes on all four targets using Homebrew 7.0.8, the exact Formula commit and real public assets in independent prefixes. It covers installation, repeat, Formula test, reinstall, actual public 0.0.35-to-0.0.42 upgrade, old-keg cleanup, Bash/Zsh/Fish startup/navigation/undo, and configuration/visit preservation. Installed binary hashes match the public manifest. After installation, run `jjump shell install` and open a new terminal.

The anonymous public root downloader passes latest and pinned 0.0.42 installation at the default prefix, unchanged repeat, explicit replacement, Bash/Zsh/Fish startup/navigation, exact shell undo and data-preserving uninstall on native Linux x86-64. An actual public 0.0.41-to-0.0.42 upgrade preserves configuration, visits and unrelated startup content. Installed binary hashes match the public manifest.

Provider protocol and setup tests use synthetic services. This release does not establish live OpenAI, Jev or Cloudflare account access, API latency or semantic quality; live API requests were not performed for 0.0.42. Earlier local macOS ARM64 real-model validation on 0.0.40 covered Ollama 0.35.1 with Tev1 4B Q8_0, English/Chinese setup and model editing in Bash/Zsh/Fish, representative selection/cancellation, and upgrades from 0.0.38/0.0.39. Those historical functional examples do not establish general semantic accuracy or current live-model acceptance across platforms. Ollama and model installation remain user-managed.

Historical [0.0.41 build results](https://github.com/mreasonyang/j-jump/actions/runs/37500544279), [Linux x86-64 repeat](https://github.com/mreasonyang/j-jump/actions/runs/37503301737), [Homebrew acceptance](https://github.com/mreasonyang/j-jump/actions/runs/37508931264) and [Homebrew Linux repeat](https://github.com/mreasonyang/j-jump/actions/runs/37509458807) apply to 0.0.41. The current evidence above applies to the public 0.0.42 assets.

## Download installer

The root [install.sh](../install.sh) is a POSIX shell downloader. It is separate from the `install.sh`
inside an archive. The downloader chooses the archive with `uname`, checks the archive's SHA256 sidecar, rejects unsafe
archive paths/member types, then runs the bundled installer. It needs curl, tar, mktemp, awk, grep, wc, install, readlink,
and sha256sum or shasum, plus standard POSIX tools. It requires no Rust, Python or jq on the end user's machine.

| Detected system / CPU | Selected release target | Minimum |
| --- | --- | --- |
| macOS / arm64 or aarch64 | aarch64-apple-darwin | macOS15 |
| macOS / x86_64 or amd64 | x86_64-apple-darwin | macOS15 |
| Linux / x86_64 or amd64 | x86_64-unknown-linux-musl | Native tested musl archive |
| Linux / aarch64 or arm64 | aarch64-unknown-linux-musl | Native tested musl archive |

Unknown systems and CPUs, including Windows and 32-bit CPUs, are refused. macOS below15 is refused. Detection fixtures cover target selection; the native Actions runs above additionally exercise each target's real archive through the TLS downloader and installed lifecycle.

The default repository is `mreasonyang/j-jump`, with public 0.0.42 assets. Retrieve the maintained installer from `main/install.sh`; without `--version`, it automatically chooses the latest stable release through GitHub's latest redirect. No version number is needed in the default command. `--repository OWNER/REPO` selects another public asset destination; this flag does not create or publish anything.

```sh
# Install the latest stable release into "$HOME/.local".
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh
```

For a custom prefix or release version, pass options to `sh -s --`:

```sh
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh -s -- --prefix "$HOME/.local" --version 0.0.42
```

If you want to inspect the script before running it, download it first, review it and then run it with the desired options:

```sh
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh -o j-jump-install.sh
# Review j-jump-install.sh before running it.
sh ./j-jump-install.sh --prefix "$HOME/.local"
# Optional: choose a specific release; v0.0.42 is also accepted
sh ./j-jump-install.sh --version 0.0.42 --prefix "$HOME/.local"
# Explicitly replace an unchanged, receipt-owned installation
sh ./j-jump-install.sh --prefix "$HOME/.local" --replace
sh ./j-jump-install.sh --help
```

Both `jjump` and `j-jump` are installed. The default prefix is `$HOME/.local`. The bundled installer refuses foreign or
modified executables, and an unchanged repeat succeeds without rewriting them. No sudo, Rust/Python installation,
credentials or product-data changes occur. The installer connects Bash, Zsh or Fish automatically and adds its binary
directory to PATH. Open a new terminal after installation, or pass `--no-shell` to skip startup-file changes.
Removal uses `install.sh --uninstall` inside an extracted archive, as described in the [archive instructions](../packaging/README.md).

Downloads use HTTPS-only redirects with TLS verification, a 10-second connection timeout, a 120-second request timeout,
at most 5 redirects, a 1KiB sidecar limit and a 128MiB archive limit. A private temporary directory is removed on completion,
failure or interruption. SHA256 verifies integrity against the sidecar from the same release; it is not a signature and
does not independently authenticate a publisher. Anonymous downloads of all four 0.0.42 targets pass verification against the accepted bundle.

Offline downloader tests run with `python3 -m unittest discover -s tests -p test_download_installer.py -v`. They include
a local TLS server with real curl and refusal of an HTTP downgrade. The native lifecycle is opt-in via
`JJ_DOWNLOAD_TEST_ARCHIVE=/absolute/path/to/archive.tar.gz` and validates and exercises the exact native macOS/Linux ARM64/x86-64 archive supplied.

### Automatic shell setup

Release 0.0.34 adds `jjump shell install` and `jjump shell uninstall`. Its archive installer connects the login
shell and PATH by default; `--no-shell` skips shell changes and `--shell bash|zsh|fish` overrides shell detection.
The download entry forwards these options; they require an archive of version 0.0.34 or newer. Existing public 0.0.33
downloads retain their existing behavior. See [automatic connection, backup and removal](../packaging/README.md).

The generated Homebrew Formula provides `jjump shell install` in its installation instructions. It does not modify
user startup files from Homebrew's installation hooks. The complete installation command is:

```sh
brew install mreasonyang/taps/j-jump && jjump shell install
```

When supplied, the real-curl TLS test also downloads and installs that exact archive from the local fixture server. Every native Actions build runs this opt-in after packaging.

## Execution boundary

The only workflow is `.github/workflows/release.yml`. It uses workflow_dispatch only: main/tag pushes, pull requests, repository events and schedules cannot start it. Its JSON representation is valid YAML and lets the Python stdlib validator check duplicate keys and exact event names without a parser dependency.

`J_JUMP_ACTIONS_ENABLED` must equal `true` before build jobs can run. Publication additionally requires `J_JUMP_PUBLISH_ENABLED=true` and explicit publish input; publish and update_homebrew default false. Manual builds have been explicitly authorized and the build variable is enabled. Publication remains separately gated; do not enable it or dispatch publication without explicit authority. A guard is not a guarantee that a manual dispatch has no billing cost.

For an already published release, the same guarded workflow has an opt-in `homebrew_acceptance_only` mode.
Supply the source `tag` and exact matching `homebrew_tap_sha`; `build_target=all` runs four native systems,
or choose one target for a focused check. This mode skips compilation and bundle generation and refuses
publication or tap updates. It runs the public lifecycle test from the exact workflow revision against
the immutable release source, without changing the release assets.

The same test can be run locally on a supported native target with Python 3, Git, curl, Bash, Zsh, Fish,
a C compiler and make (for Homebrew's Formula test dependencies):

```sh
results=$(mktemp -d)
python3 scripts/test-homebrew-release.py \
  --version 0.0.42 --source-sha b88013126ca692d6f0bb1396b86b09c952ee424e \
  --target x86_64-unknown-linux-musl \
  --tap-sha c8c7a93e1c07c7bba929d81068e717ea4aa670c7 \
  --report "$results/homebrew.json"
```

Change `--target` to the host's native archive target. The test creates an independent Homebrew 7.0.8
prefix, synthetic home and cache, then removes them on exit. It installs the real public Formula,
checks the installed binary against the public release manifest, tests repeat installation and
reinstallation, and upgrades the public 0.0.35 Formula to 0.0.42. It checks `brew test`, Bash/Zsh/Fish
startup and offline history navigation (including Bash login shells), removal of the old upgraded keg,
exact shell integration undo, and configuration/visit preservation
through upgrade, reinstall and uninstall. Inherited provider credentials and shell hooks are excluded.
On Linux the prefix is outside `/tmp` and `/var/tmp` so Homebrew's sandbox can protect its repository.

The local validator rejects automatic events, alternate workflows, unpinned actions, missing guards and bypasses around the job dependency chain. The pre-push hook remains main-only and fast-forward. A future automatic tag trigger requires a new approved change to that policy.

## Local preparation

Release tools need Python3.9+ and the Rust/C build environment; end users do not. Use a clean complete checkout, including the tracked scripts and license. Untracked build configuration causes release preflight to refuse; build in an isolated clean checkout instead of deleting unrelated configuration.

```sh
python3 scripts/release.py check-source
# In a clean checkout at the previously authorized, existing version tag:
python3 scripts/release.py check-source --tag v0.0.42

# Run on the native target; this does not dispatch or publish anything:
./scripts/ci-release.sh aarch64-apple-darwin
```

`ci-release.sh` runs offline source tests, builds the fixture binary used by credential tests, builds the release binary, packages it with clean-source and architecture checks, installs into a temporary prefix, runs the product suite with JJ_TEST_BIN, runs the no-match performance regression against the exact optimized installed binary, then replaces/uninstalls the temporary installation. macOS release builds use deployment target15.0; the Formula requires Sequoia or newer. Compatibility with older releases is not claimed. Linux builds target musl on native ARM64/x86-64 runners and reject a dynamic interpreter in the archive executable.

### Release package disclosure boundary

`python3 scripts/build-release.py --target TARGET` is the release compiler entry point used by
ci-release.sh. Rust and bundled native source paths are remapped to generic /build prefixes, including the checkout,
Cargo/Rustup homes and build-user home. Caller options and paths with spaces are retained. This uses Rust's
[source path remapping](https://doc.rust-lang.org/rustc/remap-source-paths.html) and the native compiler flags consumed by
[cc-rs](https://docs.rs/cc/latest/cc/#external-configuration-via-environment-variables).

The --release packager and release verifier inspect payload bytes and reject personal/build path prefixes. Tar members
use uid/gid0 with empty owner/group names, fixed portable modes, source-commit timestamps and no host-specific extended
metadata. Gzip headers have a zero timestamp and no original filename. Identical staged content is packed identically;
this is not a claim that different toolchains or machines compile identical executables. Copyright and license notice
names are retained. Development fixture packaging may accept debug source paths and is not a release privacy claim.

The exact artifact must still pass the installed-product and downloader lifecycle before acceptance. This package
boundary does not clean Git history, change source visibility, sign an archive or publish it.

Native jobs disable automatic Git maintenance, fsmonitor and untracked caching through job-local Git configuration to keep disposable repository cleanup free from these background Git writers. Before full product builds they repeat the repository cleanup regression three times. These settings apply only to ephemeral Actions jobs.

The manual `build_target` input defaults to `all`. Select one target for an isolated repair build; that mode skips the hosted bundle and refuses publication inputs. Full hosted publication still requires all four targets and the complete verified bundle. Previously accepted archives at the same immutable source tag can be combined with a repaired target through the local verifier and guarded publisher; source identities and all checksums must match.

The hosted matrix uses macos-15, macos-15-intel, ubuntu-24.04 and ubuntu-24.04-arm. Rust is pinned to1.98.0; Cargo.lock is retained and builds use --locked. Actions are pinned to exact commit SHAs. Each claimed platform requires a successful native run for its exact source tag; an in-progress or failed run does not establish support. The no-match performance regression runs against the exact optimized installed binary with its original timing bounds; rapid prompt tracking is also checked in the native installed suite.

## Bundle and Formula

The four-target bundle contains one tar.gz and checksum sidecar per target. Archives include jjump, j-jump -> jjump, installer, manifest, binary hash, MIT text and dependency/license inventory. Manifest signing status is explicitly unsigned.

```sh
python3 scripts/release.py verify --dist dist --output dist/release-manifest.json
python3 scripts/release.py formula --dist dist \
  --release-repository example/j-jump-releases --output dist/j-jump.rb
ruby -c dist/j-jump.rb
```

The repository in this example is a placeholder argument, not a published destination. The generator uses the archive's actual immutable filename/checksum and refuses incomplete, dirty, corrupt, mixed-source, unsafe or mismatched-architecture bundles. The inventory is the target-resolved Cargo graph, including build/test dependencies; it is not advertised as an SPDX SBOM.

The generated Formula downloads prebuilt archives, installs both current names directly into its keg and preserves license/provenance files. It does not invoke install.sh, install Rust/Python, edit startup files or access credentials. Shell activation uses eval of jjump init bash/zsh, or jjump init fish piped to source. Formula tests use an isolated profile with semantic requests disabled, record from inside the target directory and execute real offline Bash navigation. The Formula infers its version from the immutable archive URL. Distribution metadata and Formula test corrections do not rebuild or change the source-bound archives.

The existing public tap is `mreasonyang/homebrew-taps`, with root flomo.rb. Place j-jump.rb at that root too: introducing Formula/ alone would hide the existing root Formula under Homebrew's directory-discovery rules. The update helper writes only j-jump.rb, refuses dirty/non-main/foreign tap checkouts and reads back an ordinary main push.

## Publication

The public source and release repository is `mreasonyang/j-jump`; the Homebrew channel is `mreasonyang/taps/j-jump`. Release 0.0.42 contains four native archives, their four SHA256 sidecars and a source-bound release manifest. All four native targets pass at immutable source b88013126ca692d6f0bb1396b86b09c952ee424e in the build runs recorded above. The complete bundle and Formula are verified locally from their accepted source-bound archives. The guarded local release and tap helpers publish only that accepted bundle using existing local gh/git authorization, then verify draft roundtrip, anonymous downloads and exact tap SHA readback. The manual build inputs keep hosted publication disabled; native public Homebrew acceptance is a separate read-only workflow run.

For future authorized hosted publication, the release destination comes from `J_JUMP_RELEASE_REPOSITORY`; otherwise it is the source repository. Publication refuses private destinations. Same-repository publication uses the job's contents-write GITHUB_TOKEN; a different release repository and the tap need a scoped `J_JUMP_DISTRIBUTION_TOKEN` stored through GitHub's secret UI. Do not put token values in chat or tracked files.

The publisher verifies the full source-bound bundle, refuses an existing release, uploads a draft, downloads and rechecks it, publishes, then verifies anonymous archive downloads. Failure stops before tap update. Interrupted draft/publication states require inspection; the tooling never overwrites or deletes existing release assets. Homebrew updates are opt-in in the same run and occur only after that publication step succeeds. Preflight refuses a tap update before publication when its scoped credential is missing.

The published user entry points are:

```sh
brew install mreasonyang/taps/j-jump && jjump shell install
brew upgrade mreasonyang/taps/j-jump
brew uninstall mreasonyang/taps/j-jump
```

The current native archive, four-target public Homebrew and Linux x86-64 public-downloader lifecycle results provide the separately described evidence above. The tap update verifies public assets and reads back the exact remote commit.

## Platform and distribution limits

The four 0.0.42 archive targets have native Actions acceptance and verified public downloads. The current Homebrew 7.0.8 lifecycle also passes natively on all four targets in independent, non-default prefixes. These checks exercise the current Formula and public assets; existing-user Homebrew installations and provider credential prompts remain separate environment-dependent behavior. macOS packages are unsigned;
checksums bind their identities and do not establish notarization. Linux Secret Service prompts, terminal behavior,
live-provider effectiveness and genuine-user utility require their own acceptance.
