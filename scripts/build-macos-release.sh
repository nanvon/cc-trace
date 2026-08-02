#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

usage() {
	printf '%s\n' \
		'Usage: scripts/build-macos-release.sh [--target <target-triple>]' \
		'' \
		'Builds a macOS .app and .dmg, then creates a ZIP containing the .app.'
}

die() {
	printf 'build-macos-release: %s\n' "$1" >&2
	exit 1
}

if [[ "$(uname -s)" != "Darwin" ]]; then
	die 'macOS is required to build DMG and ZIP bundles.'
fi

target=""
while (($# > 0)); do
	case "$1" in
	--target)
		(($# >= 2)) || die '--target requires a Rust target triple.'
		target="$2"
		shift 2
		;;
	-h|--help)
		usage
		exit 0
		;;
	*)
		usage >&2
		die "unknown argument: $1"
		;;
	esac
done

build_args=(tauri build --bundles app,dmg)
bundle_dir="$ROOT_DIR/src-tauri/target/release/bundle/macos"

if [[ -n "$target" ]]; then
	build_args+=(--target "$target")
	bundle_dir="$ROOT_DIR/src-tauri/target/$target/release/bundle/macos"
fi

pnpm "${build_args[@]}"

version="$(node -e 'const fs = require("node:fs"); process.stdout.write(JSON.parse(fs.readFileSync("package.json", "utf8")).version);')"

case "$target" in
aarch64-apple-darwin)
	platform_label='macOS-Apple-Silicon'
	;;
x86_64-apple-darwin)
	platform_label='macOS-Intel'
	;;
universal-apple-darwin)
	platform_label='macOS-Universal'
	;;
'')
	case "$(uname -m)" in
	arm64|aarch64)
		platform_label='macOS-Apple-Silicon'
		;;
	x86_64)
		platform_label='macOS-Intel'
		;;
	*)
		platform_label="macOS-$(uname -m)"
		;;
	esac
	;;
*)
	platform_label="macOS-$target"
	;;
esac

zip_path="$bundle_dir/CC-Trace_${version}_${platform_label}.zip"
scripts/package-macos-zip.sh "$bundle_dir" "$zip_path"

printf 'DMG and app bundle: %s\n' "$bundle_dir"
printf 'ZIP: %s\n' "$zip_path"
