# Release and Homebrew

The current source and latest published release is [J-Jump 0.0.38](https://github.com/mreasonyang/j-jump/releases/tag/v0.0.38). It adds optional Cloudflare Workers AI Clef-Flash alongside official Jev, repairs provider-switch credential drafts and invalid environment readiness, and improves English/Chinese setup, consent and error guidance. Three native targets passed in [run 37461168018](https://github.com/mreasonyang/j-jump/actions/runs/37461168018), and the Intel macOS target passed its isolated repair build in [run 37467122364](https://github.com/mreasonyang/j-jump/actions/runs/37467122364). Each passes source and exact installed-package tests using immutable tag v0.0.38 at source 2fd2ce746f6b67093270c16bed4d1ddf2370fed2. All four public archives, checksum sidecars and the release manifest match the accepted Actions bundle after anonymous downloads. The bundle passes source identity, checksums, archive safety, neutral ownership, license inventory and payload disclosure checks.

| Target | Native build, installed product and TLS downloader lifecycle |
| --- | --- |
| aarch64-apple-darwin | Passed on macos-15 |
| x86_64-apple-darwin | Passed on macos-15-intel |
| aarch64-unknown-linux-musl | Passed on ubuntu-24.04-arm |
| x86_64-unknown-linux-musl | Passed on ubuntu-24.04 |

Each native job tests automatic Bash/Zsh/Fish startup configuration, backups, exact undo, command conflicts and repeated installation in synthetic homes, both from source and from the exact installed archive. The real-curl TLS downloader suite tests the exact archive's installation, repeat, replacement and removal lifecycle. Temporary acceptance installations outside those shell fixtures pass `--no-shell` and leave runner startup files untouched. Ubuntu jobs suppress the distribution's global `compinit` initialization so its terminal prompt does not contaminate isolated fixture stderr.

The [public Homebrew Formula](https://github.com/mreasonyang/homebrew-taps/blob/main/j-jump.rb) matches the unmodified Formula generated from the accepted four-target 0.0.38 bundle. After Homebrew installation, run `jjump shell install`; the complete command below does both. Formula generation, Ruby syntax, public downloads and the exact tap commit are verified. The current 0.0.38 Homebrew lifecycle has not been executed natively; archive lifecycle tests are separate evidence.

The public root downloader passes latest 0.0.38 installation at the default prefix, unchanged repeat, explicit replacement, Bash/Zsh/Fish startup and navigation, managed shell removal and configuration-preserving binary uninstall on native Linux x86-64. A real public 0.0.35-to-0.0.38 upgrade preserves configuration, visits and unrelated startup content. Installed binary hashes match the accepted public archive.

Earlier [0.0.35 acceptance](https://github.com/mreasonyang/j-jump/actions/runs/37405268855) includes a native macOS ARM64 public Homebrew lifecycle in an independent, non-default prefix, a public downloader lifecycle and an actual 0.0.34-to-0.0.35 upgrade. Those results apply to 0.0.35. Earlier [0.0.34 acceptance](https://github.com/mreasonyang/j-jump/actions/runs/37341841880) covered four native archive jobs and verified public downloads. Current 0.0.38 results use the exact newly published assets.

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

The default repository is `mreasonyang/j-jump`, with public 0.0.38 assets. Retrieve the maintained installer from `main/install.sh`; without `--version`, it automatically chooses the latest stable release through GitHub's latest redirect. No version number is needed in the default command. `--repository OWNER/REPO` selects another public asset destination; this flag does not create or publish anything.

```sh
# Install the latest stable release into "$HOME/.local".
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh
```

For a custom prefix or release version, pass options to `sh -s --`:

```sh
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh -s -- --prefix "$HOME/.local" --version 0.0.38
```

If you want to inspect the script before running it, download it first, review it and then run it with the desired options:

```sh
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh -o j-jump-install.sh
# Review j-jump-install.sh before running it.
sh ./j-jump-install.sh --prefix "$HOME/.local"
# Optional: choose a specific release; v0.0.38 is also accepted
sh ./j-jump-install.sh --version 0.0.38 --prefix "$HOME/.local"
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
does not independently authenticate a publisher. Anonymous downloads of all four 0.0.38 targets pass verification against the accepted bundle.

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

The local validator rejects automatic events, alternate workflows, unpinned actions, missing guards and bypasses around the job dependency chain. The pre-push hook remains main-only and fast-forward. A future automatic tag trigger requires a new approved change to that policy.

## Local preparation

Release tools need Python3.9+ and the Rust/C build environment; end users do not. Use a clean complete checkout, including the tracked scripts and license. Untracked build configuration causes release preflight to refuse; build in an isolated clean checkout instead of deleting unrelated configuration.

```sh
python3 scripts/release.py check-source
# For a previously authorized, existing version tag:
python3 scripts/release.py check-source --tag v0.0.38

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

The public source and release repository is `mreasonyang/j-jump`; the Homebrew channel is `mreasonyang/taps/j-jump`. Release 0.0.38 contains four native archives, their four SHA256 sidecars and a source-bound release manifest. Publication used the guarded release and tap helpers with the accepted Actions bundle and existing local gh/git authorization. Three original native build jobs and the isolated Intel repair build passed. The complete source-bound bundle and Formula were verified locally from their accepted archives. Hosted bundle/publisher/tap jobs were skipped; guarded local helpers performed publication and tap update.

For future authorized hosted publication, the release destination comes from `J_JUMP_RELEASE_REPOSITORY`; otherwise it is the source repository. Publication refuses private destinations. Same-repository publication uses the job's contents-write GITHUB_TOKEN; a different release repository and the tap need a scoped `J_JUMP_DISTRIBUTION_TOKEN` stored through GitHub's secret UI. Do not put token values in chat or tracked files.

The publisher verifies the full source-bound bundle, refuses an existing release, uploads a draft, downloads and rechecks it, publishes, then verifies anonymous archive downloads. Failure stops before tap update. Interrupted draft/publication states require inspection; the tooling never overwrites or deletes existing release assets. Homebrew updates are opt-in in the same run and occur only after that publication step succeeds. Preflight refuses a tap update before publication when its scoped credential is missing.

The published user entry points are:

```sh
brew install mreasonyang/taps/j-jump && jjump shell install
brew upgrade mreasonyang/taps/j-jump
brew uninstall mreasonyang/taps/j-jump
```

The current native archive and Linux public-downloader lifecycle results provide the separately described evidence above; the current Homebrew lifecycle remains unmeasured. The tap update verifies public assets and reads back the exact remote commit.

## Platform and distribution limits

The four 0.0.38 archive targets have native Actions acceptance and verified public downloads. Current 0.0.38 Homebrew lifecycle execution is unmeasured on all targets. Historical macOS ARM64 Homebrew acceptance applies to 0.0.35 only. macOS packages are unsigned;
checksums bind their identities and do not establish notarization. Linux Secret Service prompts, terminal behavior,
live-provider effectiveness and genuine-user utility require their own acceptance.
