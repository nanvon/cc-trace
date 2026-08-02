#!/usr/bin/env bash

set -euo pipefail

die() {
	printf 'package-macos-zip: %s\n' "$1" >&2
	exit 1
}

if [[ "$(uname -s)" != "Darwin" ]]; then
	die 'macOS is required to package a .app as ZIP.'
fi

bundle_dir="${1:-}"
zip_path="${2:-}"

[[ -n "$bundle_dir" ]] || die 'missing macOS bundle directory.'
[[ -n "$zip_path" ]] || die 'missing ZIP output path.'
[[ -d "$bundle_dir" ]] || die "bundle directory does not exist: $bundle_dir"

app_path=""
for candidate in "$bundle_dir"/*.app; do
	if [[ -d "$candidate" ]]; then
		app_path="$candidate"
		break
	fi
done
[[ -n "$app_path" ]] || die "no .app bundle found in $bundle_dir"

# Do not publish a ZIP when the app bundle itself is structurally invalid.
codesign --verify --deep --strict "$app_path"
ditto -c -k --sequesterRsrc --keepParent "$app_path" "$zip_path"

printf 'Packaged app: %s\n' "$app_path"
printf 'ZIP: %s\n' "$zip_path"
