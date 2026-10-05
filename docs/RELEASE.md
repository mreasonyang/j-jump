# Release and Homebrew preparation

The release tooling is prepared. Manual hosted build acceptance is enabled. Binary release publication and remote Homebrew updates require explicit publication scope. No public installation URL is promised. The current VERSION is 0.0.31.

## Download installer

The root [install.sh](../install.sh) is a POSIX shell downloader. It is separate from the `install.sh`
inside an archive. The downloader chooses the archive with `uname`, checks the archive's SHA256 sidecar, rejects unsafe
archive paths/member types, then runs the bundled installer. It needs curl, tar, mktemp, awk, grep, wc, install, readlink,
and sha256sum or shasum, plus standard POSIX tools. It requires no Rust, Python or jq on the end user's machine.

| Detected system / CPU | Selected release target | Minimum |
| --- | --- | --- |
| macOS / arm64 or aarch64 | aarch64-apple-darwin | macOS15 |
| macOS / x86_64 or amd64 | x86_64-apple-darwin | macOS15 |
| Linux / x86_64 or amd64 | x86_64-unknown-linux-musl | Prepared musl archive |
| Linux / aarch64 or arm64 | aarch64-unknown-linux-musl | Prepared musl archive |

Unknown systems and CPUs, including Windows and 32-bit CPUs, are refused. macOS below15 is refused. Detection tests for
the four targets are fixtures; they do not establish native execution on all four platforms.

The default repository is `mreasonyang/j-jump`, which is public and has no release assets yet. The script
therefore reports unavailable releases without installing. A future separately approved public distribution repository
can be selected with `--repository OWNER/REPO`; this flag does not create or publish anything. From a checkout or a
separately obtained copy of the root script, the following commands apply **after that public release is available**
(replace `OWNER/REPO` with the actual approved destination):

```sh
# Latest stable release (requires a latest redirect to a vX.Y.Z tag)
sh ./install.sh --repository OWNER/REPO --prefix "$HOME/.local"
# Specific release; v0.0.31 is also accepted
sh ./install.sh --repository OWNER/REPO --version 0.0.31 --prefix "$HOME/.local"
# Explicitly replace an unchanged, receipt-owned installation
sh ./install.sh --repository OWNER/REPO --prefix "$HOME/.local" --replace
sh ./install.sh --help
```

Both `jjump` and `j-jump` are installed. The default prefix is `$HOME/.local`. The bundled installer refuses foreign or
modified executables, and an unchanged repeat succeeds without rewriting them. No sudo, Rust/Python installation,
credentials, shellrc edits or user-data changes occur. Add the prefix's bin directory to PATH and activate Bash, Zsh or
Fish as described in the [main README](../README.md#install-and-activate). Removal uses `install.sh --uninstall` inside an
extracted archive, as described in the [archive instructions](../packaging/README.md).

Downloads use HTTPS-only redirects with TLS verification, a 10-second connection timeout, a 120-second request timeout,
at most 5 redirects, a 1KiB sidecar limit and a 128MiB archive limit. A private temporary directory is removed on completion,
failure or interruption. SHA256 verifies integrity against the sidecar from the same release; it is not a signature and
does not independently authenticate a publisher. Public anonymous download acceptance remains deferred.

Offline downloader tests run with `python3 -m unittest discover -s tests -p test_download_installer.py -v`. They include
a local TLS server with real curl and refusal of an HTTP downgrade. The native lifecycle is opt-in via
`JJ_DOWNLOAD_TEST_ARCHIVE=/absolute/path/to/archive.tar.gz` and validates and exercises the exact native macOS/Linux ARM64/x86-64 archive supplied.
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
python3 scripts/release.py check-source --tag v0.0.31

# Run on the native target; this does not dispatch or publish anything:
./scripts/ci-release.sh aarch64-apple-darwin
```

`ci-release.sh` runs offline source tests, builds the fixture binary used by credential tests, builds the release binary, packages it with clean-source and architecture checks, installs into a temporary prefix, runs the product suite with JJ_TEST_BIN, then replaces/uninstalls the temporary installation. macOS release builds use deployment target15.0; the Formula requires Sequoia or newer. Compatibility with older releases is not claimed. Linux builds target musl on native ARM64/x86-64 runners and reject a dynamic interpreter in the archive executable.

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

The prepared hosted matrix uses macos-15, macos-15-intel, ubuntu-24.04 and ubuntu-24.04-arm. Rust is pinned to1.98.0; Cargo.lock is retained and builds use --locked. Actions are pinned to exact commit SHAs. Each claimed platform requires a successful native run for its exact source tag; an in-progress or failed run does not establish support.

## Bundle and Formula

The four-target bundle contains one tar.gz and checksum sidecar per target. Archives include jjump, j-jump -> jjump, installer, manifest, binary hash, MIT text and dependency/license inventory. Manifest signing status is explicitly unsigned.

```sh
python3 scripts/release.py verify --dist dist --output dist/release-manifest.json
python3 scripts/release.py formula --dist dist \
  --release-repository example/j-jump-releases --output dist/j-jump.rb
ruby -c dist/j-jump.rb
```

The repository in this example is a placeholder argument, not a published destination. The generator uses the archive's actual immutable filename/checksum and refuses incomplete, dirty, corrupt, mixed-source, unsafe or mismatched-architecture bundles. The inventory is the target-resolved Cargo graph, including build/test dependencies; it is not advertised as an SPDX SBOM.

The generated Formula downloads prebuilt archives, installs both current names directly into its keg and preserves license/provenance files. It does not invoke install.sh, install Rust/Python, edit startup files or access credentials. Shell activation uses eval of jjump init bash/zsh, or jjump init fish piped to source. Formula tests use an isolated profile with semantic requests disabled.

The existing public tap is `mreasonyang/homebrew-taps`, with root flomo.rb. Place j-jump.rb at that root too: introducing Formula/ alone would hide the existing root Formula under Homebrew's directory-discovery rules. The update helper writes only j-jump.rb, refuses dirty/non-main/foreign tap checkouts and reads back an ordinary main push.

## Deferred publication

The public source repository is `mreasonyang/j-jump`. Binary release assets and the Homebrew channel still need separate publication and verification. A public Formula requires anonymously accessible release downloads; making source public does not publish those assets.

For later authorized execution, the release destination comes from `J_JUMP_RELEASE_REPOSITORY`; otherwise it is the source repository. Publication refuses private destinations. Same-repository publication uses the job's contents-write GITHUB_TOKEN; a different release repository and the tap need a scoped `J_JUMP_DISTRIBUTION_TOKEN` stored through GitHub's secret UI. Do not put token values in chat or tracked files.

The publisher verifies the full source-bound bundle, refuses an existing release, uploads a draft, downloads and rechecks it, publishes, then verifies anonymous archive downloads. Failure stops before tap update. Interrupted draft/publication states require inspection; the tooling never overwrites or deletes existing release assets. Homebrew updates are opt-in in the same run and occur only after that publication step succeeds. Preflight refuses a tap update before publication when its scoped credential is missing.

After the public channel is actually published and verified, its intended user entry points are:

```sh
brew install mreasonyang/taps/j-jump
brew upgrade mreasonyang/taps/j-jump
brew uninstall mreasonyang/taps/j-jump
```

These commands are not currently an available release claim. Real Homebrew installation, upgrade and data-preserving uninstall must be measured against published artifacts before acceptance. Local tests, static Actions lint, remote source readback, hosted target tests and public installation are separate evidence states.

## Platform and distribution limits

The prepared target matrix is not a claim of native acceptance on every target. Public downloads and the Homebrew
channel are unavailable until their artifacts are published and independently verified. macOS packages are unsigned;
checksums bind their identities and do not establish notarization. Linux Secret Service prompts, terminal behavior,
live-provider effectiveness and genuine-user utility require their own acceptance.
