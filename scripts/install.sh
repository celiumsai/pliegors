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
verify_sealed_selection() {
  directory="$1"
  selected_archive="$2"
  expected_version="$3"
  command -v node >/dev/null 2>&1 || {
    echo "Node.js is required for internal Ed25519 release verification; use cargo install or verify the full bundle manually" >&2
    exit 1
  }
  node - "$directory" "$selected_archive" "$expected_version" <<'NODE'
const { createHash, createPublicKey, verify } = require('node:crypto');
const { lstatSync, readFileSync } = require('node:fs');
const path = require('node:path');
const [directory, archive, expectedVersion] = process.argv.slice(2);
const fingerprint = 'sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250';
function bytes(name, limit) {
  const file = path.join(directory, name);
  const stat = lstatSync(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size < 1 || stat.size > limit) {
    throw new Error(`invalid release verification file: ${name}`);
  }
  return readFileSync(file);
}
const manifestBytes = bytes('RELEASE-MANIFEST.json', 1024 * 1024);
const signatureText = bytes('RELEASE-MANIFEST.json.sig', 1024).toString('utf8');
const publicPem = bytes('PLIEGORS-CANDIDATE-RELEASE.pub.pem', 16 * 1024);
const key = createPublicKey(publicPem);
const actualFingerprint = `sha256:${createHash('sha256').update(key.export({ type: 'spki', format: 'der' })).digest('hex')}`;
if (actualFingerprint !== fingerprint) throw new Error('release public key fingerprint mismatch');
if (!/^[A-Za-z0-9+/]{86}==\n?$/u.test(signatureText)) throw new Error('invalid Ed25519 signature encoding');
if (!verify(null, manifestBytes, key, Buffer.from(signatureText.trim(), 'base64'))) {
  throw new Error('release manifest signature verification failed');
}
const manifestText = manifestBytes.toString('utf8');
const manifest = JSON.parse(manifestText);
if (`${JSON.stringify(manifest, null, 2)}\n` !== manifestText) throw new Error('release manifest is not canonical JSON');
if (manifest.schema !== 'dev.pliegors.release-manifest/v1' || !Array.isArray(manifest.assets)) {
  throw new Error('unsupported release manifest schema');
}
if (manifest.signing?.algorithm !== 'Ed25519' || manifest.signing?.publicKeySha256 !== fingerprint) {
  throw new Error('release manifest signing identity mismatch');
}
if (expectedVersion && expectedVersion !== 'local' && manifest.release?.version !== expectedVersion) {
  throw new Error('selected release version does not match signed manifest');
}
if (manifest.release?.tag !== `v${manifest.release?.version}`) throw new Error('release tag/version mismatch');
const names = new Set();
for (const asset of manifest.assets) {
  if (!asset || typeof asset.name !== 'string' || names.has(asset.name)) throw new Error('duplicate or invalid manifest asset');
  names.add(asset.name);
}
for (const [name, role, limit] of [[archive, 'cli-archive', 128 * 1024 * 1024], [`${archive}.sha256`, 'integrity-sidecar', 1024]]) {
  const asset = manifest.assets.find((candidate) => candidate.name === name);
  if (!asset || asset.role !== role || !/^[0-9a-f]{64}$/u.test(asset.sha256)) throw new Error(`signed manifest lacks ${name}`);
  const value = bytes(name, limit);
  if (value.length !== asset.bytes || createHash('sha256').update(value).digest('hex') !== asset.sha256) {
    throw new Error(`signed release asset mismatch: ${name}`);
  }
}
const sidecar = readFileSync(path.join(directory, `${archive}.sha256`), 'utf8');
const archiveAsset = manifest.assets.find((asset) => asset.name === archive);
if (sidecar !== `${archiveAsset.sha256}  ${archive}`) throw new Error('signed checksum sidecar content mismatch');
process.stdout.write(`Verified Ed25519 release manifest for ${archive}\n`);
NODE
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
  bundle_dir=$(dirname "$archive_path")
  sealed=1
  for metadata in RELEASE-MANIFEST.json RELEASE-MANIFEST.json.sig PLIEGORS-CANDIDATE-RELEASE.pub.pem; do
    if [ ! -f "$bundle_dir/$metadata" ]; then sealed=0; break; fi
    cp "$bundle_dir/$metadata" "$tmp/$metadata"
  done
  if [ "$sealed" -ne 1 ] && [ "${PLIEGORS_INSTALLER_ALLOW_UNSEALED:-0}" != "1" ]; then
    echo "Local archive must be accompanied by the signed PliegoRS release manifest and public key" >&2
    exit 1
  fi
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
  curl_get "$base/RELEASE-MANIFEST.json" "$tmp/RELEASE-MANIFEST.json"
  curl_get "$base/RELEASE-MANIFEST.json.sig" "$tmp/RELEASE-MANIFEST.json.sig"
  curl_get "$base/PLIEGORS-CANDIDATE-RELEASE.pub.pem" "$tmp/PLIEGORS-CANDIDATE-RELEASE.pub.pem"
  sealed=1
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
if [ "${sealed:-0}" -eq 1 ]; then
  verify_sealed_selection "$tmp" "$archive" "$version"
fi
unzip -q "$tmp/$archive" -d "$tmp/unpacked"
candidate=$(find "$tmp/unpacked" -type f -name pliego -print -quit)
[ -n "$candidate" ] || { echo "Archive lacks pliego" >&2; exit 1; }
mkdir -p "$bin_dir" "$home/rollback"
[ ! -f "$binary" ] || cp "$binary" "$backup"
install -m 755 "$candidate" "$binary"
echo "Installed PliegoRS $release_label at $binary"
