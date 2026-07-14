#!/usr/bin/env sh
# SPDX-License-Identifier: Apache-2.0
set -eu

download_base="https://github.com/celiumsai/pliegors/releases/download"
home="${PLIEGO_HOME:-$HOME/.pliego}"
version=""
channel=""
mode="install"
archive_path=""

usage() {
  echo "usage: install.sh --version <version> [--install-dir <path>]"
  echo "       install.sh --channel latest [--install-dir <path>]"
  echo "       install.sh --archive <zip> [--version <version>] [--install-dir <path>]"
  echo "       install.sh --rollback|--uninstall [--install-dir <path>]"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version) version="${2:?--version requires a value}"; shift 2 ;;
    --channel) channel="${2:?--channel requires a value}"; shift 2 ;;
    --archive) archive_path="${2:?--archive requires a value}"; shift 2 ;;
    --install-dir) home="${2:?--install-dir requires a value}"; shift 2 ;;
    --rollback) mode="rollback"; shift ;;
    --uninstall) mode="uninstall"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

bin_dir="$home/bin"
binary="$bin_dir/pliego"
backup="$home/rollback/pliego"

if [ "$mode" = "uninstall" ]; then
  rm -f "$binary"
  echo "Removed $binary"
  exit 0
fi
if [ "$mode" = "rollback" ]; then
  [ -f "$backup" ] || { echo "No rollback binary at $backup" >&2; exit 1; }
  mkdir -p "$bin_dir"
  mv -f "$backup" "$binary"
  chmod 755 "$binary"
  echo "Restored $binary"
  exit 0
fi

curl_get() {
  curl -fsSL --proto '=https' --tlsv1.2 --user-agent 'PliegoRS-Installer' "$1" -o "$2"
}
if [ -n "$archive_path" ] && [ -n "$channel" ]; then
  echo "--channel cannot be combined with --archive" >&2
  exit 2
fi
if [ -z "$archive_path" ]; then
  if [ -n "$version" ] && [ -n "$channel" ]; then
    echo "Choose either --version <version> or --channel latest" >&2
    exit 2
  fi
  if [ -z "$version" ] && [ -z "$channel" ]; then
    echo "A release selector is required; use --version <version> or --channel latest" >&2
    exit 2
  fi
  if [ -n "$channel" ] && [ "$channel" != "latest" ]; then
    echo "Invalid PliegoRS channel: $channel" >&2
    exit 2
  fi
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
if [ -n "$archive_path" ]; then
  [ -f "$archive_path" ] || { echo "Archive does not exist: $archive_path" >&2; exit 1; }
  [ -f "$archive_path.sha256" ] || { echo "Checksum does not exist: $archive_path.sha256" >&2; exit 1; }
  archive=$(basename "$archive_path")
  version="${version:-local}"
  release_label="$version"
  cp "$archive_path" "$tmp/$archive"
  cp "$archive_path.sha256" "$tmp/$archive.sha256"
else
  if [ -n "$version" ]; then
    echo "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?(\+[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$' || {
      echo "Invalid PliegoRS version: $version" >&2
      exit 2
    }
    base="$download_base/v$version"
    release_label="$version"
  else
    base="https://github.com/celiumsai/pliegors/releases/latest/download"
    release_label="latest"
  fi
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) target="x86_64-unknown-linux-gnu" ;;
    Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-gnu" ;;
    Darwin-x86_64) target="x86_64-apple-darwin" ;;
    Darwin-arm64) target="aarch64-apple-darwin" ;;
    *) echo "Unsupported platform: $(uname -s) $(uname -m)" >&2; exit 1 ;;
  esac
  archive="pliego-$target.zip"
  curl_get "$base/$archive" "$tmp/$archive"
  curl_get "$base/$archive.sha256" "$tmp/$archive.sha256"
fi
expected=$(awk 'NR == 1 {print tolower($1)}' "$tmp/$archive.sha256")
echo "$expected" | grep -Eq '^[0-9a-f]{64}$' || {
  echo "Invalid sha256 sidecar for $archive" >&2
  exit 1
}
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$archive" | awk '{print $1}')
else
  actual=$(shasum -a 256 "$tmp/$archive" | awk '{print $1}')
fi
[ "$actual" = "$expected" ] || { echo "sha256 mismatch for $archive" >&2; exit 1; }
unzip -q "$tmp/$archive" -d "$tmp/unpacked"
candidate=$(find "$tmp/unpacked" -type f -name pliego -print -quit)
[ -n "$candidate" ] || { echo "Archive lacks pliego" >&2; exit 1; }
mkdir -p "$bin_dir" "$home/rollback"
[ ! -f "$binary" ] || cp "$binary" "$backup"
install -m 755 "$candidate" "$binary"
echo "Installed PliegoRS $release_label at $binary"
