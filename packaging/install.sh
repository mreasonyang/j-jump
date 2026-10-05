#!/bin/sh
# J-Jump private-archive installer.
#
# Ownership model. A prefix is treated as owned when it holds either
#   * share/j-jump-install/installed.sha256 - a receipt this installer wrote, exactly 64
#     lowercase hex characters, or
#   * share/j-jump-install/.install-in-progress - the marker written immediately before
#     the publication renames, so interrupted publication stays identifiable.
# An owned installation is replaced only while it is unchanged and is removed
# even after its executable was modified. When ownership cannot be proven, or an
# installation path is not a plain object this installer manages, it changes
# nothing and prints the exact commands that recover the prefix, so neither
# --replace nor --uninstall can be permanently blocked. It never operates through
# a symbolic link at the prefix, bin, share directory, canonical executable or
# receipt path. Symbolic links above the prefix (for example /tmp on
# macOS) are not a reason to refuse: only the installation's own components are
# checked. The only managed link is bin/j-jump -> jjump; its exact link text
# and installation ownership are checked before any mutation.
set -eu

usage() {
	printf '%s\n' \
		'Usage: ./install.sh [--prefix ABSOLUTE_DIR] [--replace|--uninstall]' \
		'  --replace    replace an unchanged owned installation; only the current executable is retained' \
		'  --uninstall  remove an owned installation; user config, visits, cache, credentials and shellrc are retained' >&2
}

# $1 is the offending path. Used for anything that is not the plain file this
# installer manages, and for links, at the executable, receipt and ownership
# marker paths. Prints a removal list that always returns
# the prefix to a state where a fresh install or an uninstall can proceed.
refuse_unsafe_object() {
	printf '%s\n' \
		"Refusing $1: $2" \
		"Inspect it with: ls -ld $1" \
		'Nothing was changed. Remove the offending path, then re-run this installer:' \
		"  rm -rf -- '$1'" \
		'If this installation is yours and should go completely, delete these as well:' \
		"  rm -rf -- '$target'" \
		"  rm -rf -- '$alias_target'" \
		"  rm -rf -- '$receipt'" >&2
	exit 2
}

# $1 is a reason sentence. Used when no receipt and no marker prove ownership.
refuse_unowned() {
	printf '%s\n' \
		"Refusing to change the installation at $prefix: this installer cannot prove it owns it." \
		"$1" \
		"  executable: $target" \
		"  receipt:    $receipt" \
		'Nothing was changed. Inspect them, and if this installation is yours remove exactly these, then re-run:' \
		"  rm -rf -- '$target'" \
		"  rm -rf -- '$alias_target'" \
		"  rm -rf -- '$receipt'" >&2
	exit 2
}

# $1 is the prefix, bin, share or share/j-jump-install directory.
refuse_directory_link() {
	printf '%s\n' \
		"Refusing $1: it is a symbolic link and this installer installs only into real directories." \
		"A receipt cannot identify an installation whose location can be repointed." \
		"Inspect it with: ls -ld $1" \
		'Remove the link or replace it with a real directory, or pass the real directory as --prefix, then re-run.' >&2
	exit 2
}

check_directory_links() {
	for directory in "$prefix" "$prefix/bin" "$prefix/share" "$prefix/share/j-jump-install"; do
		if [ -L "$directory" ]; then refuse_directory_link "$directory"; fi
	done
}

prefix=${HOME}/.local
replace=false
uninstall=false
while [ "$#" -gt 0 ]; do
	case "$1" in
		--prefix) [ "$#" -ge 2 ] || { usage; exit 2; }; prefix=$2; shift 2;;
		--replace) replace=true; shift;;
		--uninstall) uninstall=true; shift;;
		*) usage; exit 2;;
	esac
done
case "$prefix" in /*) ;; *) echo 'Prefix must be absolute' >&2; exit 2;; esac
while [ "$prefix" != / ] && [ "${prefix%/}" != "$prefix" ]; do prefix=${prefix%/}; done
root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
target=$prefix/bin/jjump
alias_target=$prefix/bin/j-jump
receipt=$prefix/share/j-jump-install/installed.sha256
marker=$prefix/share/j-jump-install/.install-in-progress
hash() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi; }

check_directory_links
if [ -L "$target" ]; then refuse_unsafe_object "$target" 'it is a symbolic link, and this installer never removes or overwrites through a link.'; fi
if [ -L "$receipt" ]; then refuse_unsafe_object "$receipt" 'it is a symbolic link, and this installer never reads or writes a receipt through a link.'; fi
# The ownership marker is checked exactly like the other owned paths. Writing or
# removing through a link here would damage whatever the link names, and a link
# at this path would let an unrelated tree be claimed as owned.
if [ -L "$marker" ]; then refuse_unsafe_object "$marker" 'it is a symbolic link, and this installer never reads, writes or removes an ownership marker through a link.'; fi

target_present=false
if [ -e "$target" ]; then target_present=true; fi
receipt_present=false
if [ -e "$receipt" ]; then receipt_present=true; fi
marker_present=false
if [ -e "$marker" ] || [ -L "$marker" ]; then marker_present=true; fi
if $target_present && [ ! -f "$target" ]; then refuse_unsafe_object "$target" 'it exists but is not a regular file this installer manages.'; fi
if $receipt_present && [ ! -f "$receipt" ]; then refuse_unsafe_object "$receipt" 'it exists but is not a regular file this installer manages.'; fi
if $marker_present && [ ! -f "$marker" ]; then refuse_unsafe_object "$marker" 'it exists but is not a regular file this installer manages.'; fi

# A receipt is valid only when it is exactly one lower-case SHA-256. Anything
# else - empty, garbage, truncated, upper case - proves nothing.
receipt_sha=''
if $receipt_present; then receipt_sha=$(cat -- "$receipt" 2>/dev/null || true); fi
receipt_valid=false
case "$receipt_sha" in
	'') ;;
	*[!0123456789abcdef]*) ;;
	*) if [ "${#receipt_sha}" -eq 64 ]; then receipt_valid=true; fi ;;
esac
owned=false
if $receipt_valid || $marker_present; then owned=true; fi

# Both names are preflighted before touching the executable, alias or receipt.
# Do not follow a link to decide whether it is the alias we manage.
alias_present=false
if [ -e "$alias_target" ] || [ -L "$alias_target" ]; then
	alias_present=true
	if [ ! -L "$alias_target" ]; then
		refuse_unsafe_object "$alias_target" 'it is not the managed j-jump -> jjump symbolic link.'
	fi
	if [ "$(readlink "$alias_target")" != jjump ]; then
		refuse_unsafe_object "$alias_target" 'the symbolic link was retargeted; expected the relative target jjump.'
	fi
	if ! $owned; then
		refuse_unsafe_object "$alias_target" 'no receipt or interrupted-install marker proves ownership of this symbolic link.'
	fi
fi

if $uninstall; then
	if $owned && $target_present && $receipt_valid && [ -x "$target" ] && [ "$(hash "$target")" = "$receipt_sha" ]; then "$target" adapter stop >/dev/null 2>&1 || true; fi
	if ! $owned; then
		if $target_present || $receipt_present; then
			refuse_unowned 'No installer receipt and no interrupted-install marker was found, so this tree cannot be shown to be a J-Jump installation.'
		fi
		echo 'No owned J-Jump installation to remove' >&2
		exit 2
	fi
	if $target_present; then
		target_hash=$(hash "$target" 2>/dev/null || true)
		rm -f -- "$target"
		printf '%s\n' "Removed owned executable $target."
		if $receipt_valid && [ "$target_hash" != "$receipt_sha" ]; then
			printf '%s\n' 'Note: the executable did not match its receipt; it had been modified after installation and was removed as an owned file.'
		fi
	else
		printf '%s\n' "No installed executable was present at $target; removed the leftover installer state."
	fi
	if $alias_present; then
		rm -f -- "$alias_target"
		printf '%s\n' "Removed owned alias $alias_target."
	fi
	if $receipt_present; then rm -f -- "$receipt"; fi
	if $marker_present; then rm -f -- "$marker"; fi
	rmdir -- "$prefix/share/j-jump-install" 2>/dev/null || true
	echo 'User config, visits, cache, credentials and shellrc retained.'
	exit 0
fi

# Verify the package before anything in the prefix is changed, so a bad archive
# can never destroy or displace the executable that is already installed.
if [ ! -f "$root/jjump" ] || [ ! -f "$root/binary.sha256" ]; then
	echo 'Package is incomplete: jjump and binary.sha256 must both sit next to install.sh' >&2
	exit 2
fi
if [ "$(hash "$root/jjump")" != "$(cat -- "$root/binary.sha256")" ]; then
	printf '%s\n' 'Package checksum mismatch: jjump does not match binary.sha256.' \
		'Nothing in the prefix was changed. Re-obtain the archive and verify its .sha256 sidecar.' >&2
	exit 2
fi

if $target_present && $owned && $receipt_valid && ! $marker_present && $alias_present && [ "$(hash "$target")" = "$receipt_sha" ] && [ "$(hash "$root/jjump")" = "$receipt_sha" ]; then
    printf '%s\n' 'Same archive already installed; nothing changed.'
    exit 0
fi
if $target_present; then
	if ! $owned; then
		refuse_unowned 'No installer receipt and no interrupted-install marker was found, so the existing executable cannot be shown to belong to J-Jump.'
	fi
	if $receipt_valid && [ "$(hash "$target" 2>/dev/null || true)" != "$receipt_sha" ]; then
		printf '%s\n' \
			"Refusing to overwrite $target: it does not match the hash recorded in $receipt." \
			'The executable changed after installation, so this installer will not overwrite it.' \
			'This is recoverable: restore the recorded executable, or remove the owned installation and install again:' \
			"  $0 --prefix '$prefix' --uninstall" \
			"  $0 --prefix '$prefix'" >&2
		exit 2
	fi
	if ! $replace; then
		echo 'Use --replace to upgrade the owned installation' >&2
		exit 2
	fi
fi

if $alias_present && ! $replace; then
	echo 'Use --replace to repair or upgrade the owned installation' >&2
	exit 2
fi

mkdir -p -- "$prefix/bin" "$prefix/share/j-jump-install"
check_directory_links
draft=$(mktemp "$prefix/bin/.j-jump-install.XXXXXX")
receipt_draft=$(mktemp "$prefix/share/j-jump-install/.receipt.XXXXXX")
marker_draft=$(mktemp "$prefix/share/j-jump-install/.j-jump-marker.XXXXXX")
alias_dir=$(mktemp -d "$prefix/bin/.j-jump-alias.XXXXXX")
alias_draft=$alias_dir/j-jump
trap 'rm -f -- "$draft" "$receipt_draft" "$marker_draft" "$alias_draft"; rmdir -- "$alias_dir" 2>/dev/null || true' EXIT HUP INT TERM
install -m 755 "$root/jjump" "$draft"
ln -s jjump "$alias_draft"
printf '%s\n' "$(hash "$draft")" > "$receipt_draft"
# Marker first: if this run dies between the renames below, the prefix stays
# identifiable as owned by both --replace and --uninstall. It is created by
# mktemp and renamed into place rather than with ": >", because a redirection
# follows a pre-existing symlink and would truncate whatever that link names. A
# rename replaces the destination entry itself, so it cannot be redirected.
mv -- "$marker_draft" "$marker"
mv -- "$draft" "$target"
mv -f -- "$alias_draft" "$alias_target"
mv -- "$receipt_draft" "$receipt"
rm -f -- "$marker" 2>/dev/null || true
printf '%s\n' "Installed $target and equivalent alias $alias_target" 'Add the prefix bin directory to PATH and activate your shell. First j/ji opens configuration automatically.' 'Shell integration: eval "$(jjump init zsh)" (Bash: use bash; Fish: jjump init fish | source).' 'shellrc is unchanged. Both names use the same current executable.'
