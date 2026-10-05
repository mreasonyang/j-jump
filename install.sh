#!/bin/sh
# Download a matching current release, then use its owner-checked installer.
set -eu
LC_ALL=C
export LC_ALL

usage() {
    cat <<'EOF'
Usage: sh install.sh [--version X.Y.Z] [--repository OWNER/REPO]
                     [--prefix ABSOLUTE_DIR] [--replace]
  --version     install this version (optional v prefix); default: latest stable
  --repository  public GitHub release repository; default: mreasonyang/j-jump
  --prefix      installation prefix; default: $HOME/.local
  --replace     replace an unchanged installation owned by the archive installer
  --help        show this help

Requires macOS 15+ or Linux, x86-64 or ARM64, curl, tar and SHA256 tools.
No sudo or shell startup-file edits. Public release availability is required.
EOF
}
fail() { printf '%s\n' "j-jump installer: $*" >&2; exit 2; }
valid_version() {
    [ "${#1}" -le 64 ] && printf '%s\n' "$1" |
        grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
}

version=''
repository=mreasonyang/j-jump
prefix=${HOME:+$HOME/.local}
replace=false
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version|--repository|--prefix)
            [ "$#" -ge 2 ] || fail "Missing value for $1; use --help."
            case "$1" in
                --version) version=${2#v}; valid_version "$version" || fail 'Version must be X.Y.Z (optional v prefix).';;
                --repository) repository=$2;;
                --prefix) prefix=$2;;
            esac
            shift 2;;
        --replace) replace=true; shift;;
        --help|-h) usage; exit 0;;
        *) fail "Unknown option: $1; use --help.";;
    esac
done
case "$repository" in
    */*) owner=${repository%%/*}; repo=${repository#*/};;
    *) fail 'Repository must be OWNER/REPO.';;
esac
case "$owner" in ''|*[!a-zA-Z0-9-]*) fail 'Invalid repository owner.';; esac
case "$repo" in ''|.|..|*[!a-zA-Z0-9._-]*) fail 'Invalid repository name.';; esac
[ "${#repository}" -le 200 ] || fail 'Repository name is too long.'
case "$prefix" in /*) ;; *) fail 'Prefix must be an absolute directory.';; esac
while [ "$prefix" != / ] && [ "${prefix%/}" != "$prefix" ]; do prefix=${prefix%/}; done
# Refuse redirected installation components before any download or prefix write.
for component in "$prefix" "$prefix/bin" "$prefix/share" "$prefix/share/j-jump-install"; do
    [ ! -L "$component" ] || fail "Refusing symbolic link at $component; pass a real installation directory."
done

system=$(uname -s) || fail 'Cannot detect the operating system.'
machine=$(uname -m) || fail 'Cannot detect the CPU architecture.'
case "$machine" in
    x86_64|amd64) arch=x86_64;;
    aarch64|arm64) arch=aarch64;;
    *) fail "Unsupported CPU architecture: $machine (requires x86-64 or ARM64).";;
esac
case "$system" in
    Darwin)
        command -v sw_vers >/dev/null 2>&1 || fail 'Cannot check the macOS version (sw_vers is missing).'
        macos=$(sw_vers -productVersion) || fail 'Cannot check the macOS version.'
        major=${macos%%.*}
        case "$major" in ''|*[!0-9]*) fail 'Cannot parse the macOS version.';; esac
        [ "$major" -ge 15 ] || fail 'These release archives require macOS 15 or newer.'
        target=$arch-apple-darwin;;
    Linux) target=$arch-unknown-linux-musl;;
    *) fail "Unsupported operating system: $system (requires macOS or Linux).";;
esac
for utility in curl tar awk grep mktemp wc install readlink; do
    command -v "$utility" >/dev/null 2>&1 || fail "Required tool is missing: $utility."
done
if command -v sha256sum >/dev/null 2>&1; then
    hash_tool=sha256sum
elif command -v shasum >/dev/null 2>&1; then
    hash_tool=shasum
else
    fail 'Install sha256sum or shasum to verify downloads.'
fi
hash() {
    if [ "$hash_tool" = sha256sum ]; then
        digest_output=$(sha256sum "$1") || return 1
    else
        digest_output=$(shasum -a 256 "$1") || return 1
    fi
    printf '%s\n' "${digest_output%% *}"
}
# -q must be first: a user's .curlrc must not weaken TLS or change destinations.
fetch() {
    curl -q --proto '=https' --proto-redir '=https' --fail --location \
        --silent --show-error --connect-timeout 10 --max-time 120 \
        --max-redirs 5 "$@"
}
release_url=https://github.com/$repository/releases
if [ -z "$version" ]; then
    latest=$(fetch --head --output /dev/null --write-out '%{url_effective}' "$release_url/latest") ||
        fail "Cannot discover a release at $release_url. It must be public and contain release assets; use --version or --repository, or build from source."
    case "$latest" in
        "$release_url/tag/v"*) version=${latest#"$release_url/tag/v"};;
        *) fail "Latest release did not resolve to a version tag in $repository; use --version X.Y.Z.";;
    esac
    valid_version "$version" || fail 'Latest release tag must be vX.Y.Z; use --version X.Y.Z.'
fi

umask 077
scratch=$(mktemp -d "${TMPDIR:-/tmp}/j-jump-download.XXXXXX") || fail 'Cannot create a private download directory.'
cleanup() { rm -rf -- "$scratch"; }
trap cleanup 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
name=j-jump-$version-$target
asset=$name.tar.gz
archive=$scratch/$asset
sidecar=$archive.sha256
asset_url=$release_url/download/v$version/$asset
printf '%s\n' "Downloading J-Jump $version for $target from $repository..."
fetch --max-filesize 1024 --output "$sidecar" "$asset_url.sha256" ||
    fail "Cannot download the checksum for $asset. Check the public repository, version and target."
[ "$(wc -c < "$sidecar")" -le 1024 ] || fail 'Checksum sidecar is too large.'
# Never pass an untrusted sidecar to sha256sum -c: it could name another file.
expected=$(awk -v filename="$asset" '
    NR != 1 { bad=1 }
    NR == 1 {
        if (length($1) != 64 || $1 ~ /[^0-9a-f]/ || NF != 2 || $2 != filename ||
            $0 != $1 "  " filename) bad=1
        digest=$1
    }
    END { if (NR != 1 || bad) exit 1; print digest }
' "$sidecar") || fail 'Invalid checksum sidecar or unexpected archive filename.'
fetch --max-filesize 134217728 --output "$archive" "$asset_url" ||
    fail "Cannot download $asset. Check connectivity and public release availability."
[ "$(wc -c < "$archive")" -le 134217728 ] || fail 'Release archive is too large (limit: 128 MiB).'
actual=$(hash "$archive") || fail 'Cannot compute the archive SHA256.'
[ "$actual" = "$expected" ] || fail 'Archive SHA256 mismatch; nothing was installed.'

# The current archive has a fixed root and only regular files, directories and
# one relative command alias. Validate BEFORE extraction or running install.sh.
tar -tzf "$archive" > "$scratch/members" || fail 'Cannot read the release archive.'
awk -v root="$name" '
    {
        if (++count > 4096 || seen[$0]++ || $0 ~ /[^a-zA-Z0-9_.\/+-]/) exit 1
        if ($0 == root "/") next
        if (index($0, root "/") != 1) exit 1
        path=substr($0, length(root)+2)
        if (path ~ /(^|\/)\.\.?($|\/)/ || path ~ /\/\//) exit 1
        if (path == "jjump" || path == "j-jump" || path == "install.sh" ||
            path == "README.md" || path == "LICENSE.md" || path == "binary.sha256" ||
            path == "manifest.json" || path == "dependencies.json") next
        if (path ~ /^licenses\//) next
        exit 1
    }
    END { if (!count) exit 1 }
' "$scratch/members" || fail 'Unsafe, duplicate or unexpected archive member.'
tar -tvzf "$archive" > "$scratch/types" || fail 'Cannot inspect archive member types.'
awk -v alias="$name/j-jump" '
    {
        type=substr($0,1,1)
        if ($0 ~ / link to /) exit 1
        if (type == "l") {
            if (++links != 1 || $(NF-2) != alias || $(NF-1) != "->" || $NF != "jjump") exit 1
        } else if (type != "-" && type != "d") exit 1
    }
    END { if (links != 1) exit 1 }
' "$scratch/types" || fail 'Unsafe archive link or member type.'
for member in jjump j-jump install.sh README.md LICENSE.md binary.sha256 manifest.json dependencies.json; do
    grep -Fxq "$name/$member" "$scratch/members" || fail "Incomplete archive: missing $member."
done
mkdir "$scratch/extracted"
tar -xzf "$archive" -C "$scratch/extracted" || fail 'Cannot extract the verified archive.'
package=$scratch/extracted/$name
for member in jjump install.sh README.md LICENSE.md binary.sha256 manifest.json dependencies.json; do
    [ -f "$package/$member" ] && [ ! -L "$package/$member" ] || fail "Invalid archive file: $member."
done
[ -L "$package/j-jump" ] && [ "$(readlink "$package/j-jump")" = jjump ] || fail 'Invalid archive command alias.'
printf '%s\n' "Verified SHA256: $actual"
set -- --prefix "$prefix"
if $replace; then set -- "$@" --replace; fi
sh "$package/install.sh" "$@"
