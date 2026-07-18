# SPDX-License-Identifier: Apache-2.0
param(
    [string]$Version,
    [string]$Channel,
    [string]$ArchivePath,
    [string]$InstallDir = $(if ($env:PLIEGO_HOME) { $env:PLIEGO_HOME } else { Join-Path $HOME '.pliego' }),
    [switch]$Rollback,
    [switch]$Uninstall
)
$ErrorActionPreference = 'Stop'
$downloadBase = 'https://github.com/celiumsai/pliegors/releases/download'
$binDir = Join-Path $InstallDir 'bin'
$binary = Join-Path $binDir 'pliego.exe'
$backup = Join-Path $InstallDir 'rollback\pliego.exe'

function Invoke-SealedSelectionVerification {
    param([string]$Directory, [string]$Archive, [string]$ExpectedVersion)
    $node = Get-Command node -ErrorAction SilentlyContinue
    if (-not $node) {
        throw 'Node.js is required for internal Ed25519 release verification; use cargo install or verify the full bundle manually'
    }
    $verifier = Join-Path $Directory 'verify-selection.cjs'
    @'
const { createHash, createPublicKey, verify } = require('node:crypto');
const { lstatSync, readFileSync } = require('node:fs');
const path = require('node:path');
const [directory, archive, expectedVersion] = process.argv.slice(2);
const fingerprint = 'sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250';
function bytes(name, limit) {
  const file = path.join(directory, name);
  const stat = lstatSync(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size < 1 || stat.size > limit) throw new Error(`invalid release verification file: ${name}`);
  return readFileSync(file);
}
const manifestBytes = bytes('RELEASE-MANIFEST.json', 1024 * 1024);
const signatureText = bytes('RELEASE-MANIFEST.json.sig', 1024).toString('utf8');
const publicPem = bytes('PLIEGORS-CANDIDATE-RELEASE.pub.pem', 16 * 1024);
const key = createPublicKey(publicPem);
const actualFingerprint = `sha256:${createHash('sha256').update(key.export({ type: 'spki', format: 'der' })).digest('hex')}`;
if (actualFingerprint !== fingerprint) throw new Error('release public key fingerprint mismatch');
if (!/^[A-Za-z0-9+/]{86}==\n?$/u.test(signatureText)) throw new Error('invalid Ed25519 signature encoding');
if (!verify(null, manifestBytes, key, Buffer.from(signatureText.trim(), 'base64'))) throw new Error('release manifest signature verification failed');
const manifestText = manifestBytes.toString('utf8');
const manifest = JSON.parse(manifestText);
if (`${JSON.stringify(manifest, null, 2)}\n` !== manifestText) throw new Error('release manifest is not canonical JSON');
if (manifest.schema !== 'dev.pliegors.release-manifest/v1' || !Array.isArray(manifest.assets)) throw new Error('unsupported release manifest schema');
if (manifest.signing?.algorithm !== 'Ed25519' || manifest.signing?.publicKeySha256 !== fingerprint) throw new Error('release manifest signing identity mismatch');
if (expectedVersion && expectedVersion !== 'local' && manifest.release?.version !== expectedVersion) throw new Error('selected release version does not match signed manifest');
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
  if (value.length !== asset.bytes || createHash('sha256').update(value).digest('hex') !== asset.sha256) throw new Error(`signed release asset mismatch: ${name}`);
}
const sidecar = readFileSync(path.join(directory, `${archive}.sha256`), 'utf8');
const archiveAsset = manifest.assets.find((asset) => asset.name === archive);
if (sidecar !== `${archiveAsset.sha256}  ${archive}`) throw new Error('signed checksum sidecar content mismatch');
process.stdout.write(`Verified Ed25519 release manifest for ${archive}\n`);
'@ | Set-Content -LiteralPath $verifier -Encoding utf8NoBOM
    & $node.Source $verifier $Directory $Archive $ExpectedVersion
    if ($LASTEXITCODE -ne 0) { throw 'internal Ed25519 release verification failed' }
}

if ($Uninstall) {
    Remove-Item -LiteralPath $binary -Force -ErrorAction SilentlyContinue
    Write-Output "Removed $binary"
    exit 0
}
if ($Rollback) {
    if (-not (Test-Path -LiteralPath $backup -PathType Leaf)) { throw "No rollback binary at $backup" }
    New-Item -ItemType Directory -Force $binDir | Out-Null
    Move-Item -LiteralPath $backup -Destination $binary -Force
    Write-Output "Restored $binary"
    exit 0
}

$headers = @{ 'User-Agent' = 'PliegoRS-Installer' }
if ($ArchivePath -and $Channel) {
    throw '-Channel cannot be combined with -ArchivePath'
}
if (-not $ArchivePath) {
    if ($Version -and $Channel) { throw 'Choose either -Version <version> or -Channel latest' }
    if (-not $Version -and -not $Channel) {
        throw 'A release selector is required; use -Version <version> or -Channel latest'
    }
    if ($Channel -and $Channel -cne 'latest') { throw "Invalid PliegoRS channel: $Channel" }
}
$temp = Join-Path ([System.IO.Path]::GetTempPath()) ("pliegors-" + [guid]::NewGuid())
New-Item -ItemType Directory $temp | Out-Null
try {
    if ($ArchivePath) {
        $sourceArchive = (Resolve-Path -LiteralPath $ArchivePath).Path
        $sourceChecksum = "$sourceArchive.sha256"
        if (-not (Test-Path -LiteralPath $sourceChecksum -PathType Leaf)) { throw "Checksum does not exist: $sourceChecksum" }
        $archive = Split-Path $sourceArchive -Leaf
        $archiveFile = Join-Path $temp $archive
        $checksumPath = "$archiveFile.sha256"
        Copy-Item -LiteralPath $sourceArchive -Destination $archiveFile
        Copy-Item -LiteralPath $sourceChecksum -Destination $checksumPath
        $bundleDir = Split-Path $sourceArchive
        $sealed = $true
        foreach ($metadata in 'RELEASE-MANIFEST.json', 'RELEASE-MANIFEST.json.sig', 'PLIEGORS-CANDIDATE-RELEASE.pub.pem') {
            $sourceMetadata = Join-Path $bundleDir $metadata
            if (-not (Test-Path -LiteralPath $sourceMetadata -PathType Leaf)) { $sealed = $false; break }
            Copy-Item -LiteralPath $sourceMetadata -Destination (Join-Path $temp $metadata)
        }
        if (-not $sealed -and $env:PLIEGORS_INSTALLER_ALLOW_UNSEALED -ne '1') {
            throw 'Local archive must be accompanied by the signed PliegoRS release manifest and public key'
        }
        if (-not $Version) { $Version = 'local' }
        $releaseLabel = $Version
    } else {
        if ($Version -and $Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?(\+[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$') {
            throw "Invalid PliegoRS version: $Version"
        }
        $architecture = if ($env:PROCESSOR_ARCHITEW6432) {
            $env:PROCESSOR_ARCHITEW6432
        } else {
            $env:PROCESSOR_ARCHITECTURE
        }
        if ($architecture -ne 'AMD64') {
            throw "Unsupported Windows architecture: $architecture"
        }
        $target = 'x86_64-pc-windows-msvc'
        $archive = "pliego-$target.zip"
        if ($Version) {
            $base = "$downloadBase/v$Version"
            $releaseLabel = $Version
        } else {
            $base = 'https://github.com/celiumsai/pliegors/releases/latest/download'
            $releaseLabel = 'latest'
        }
        $archiveFile = Join-Path $temp $archive
        $checksumPath = "$archiveFile.sha256"
        Invoke-WebRequest -Headers $headers -Uri "$base/$archive" -OutFile $archiveFile
        Invoke-WebRequest -Headers $headers -Uri "$base/$archive.sha256" -OutFile $checksumPath
        Invoke-WebRequest -Headers $headers -Uri "$base/RELEASE-MANIFEST.json" -OutFile (Join-Path $temp 'RELEASE-MANIFEST.json')
        Invoke-WebRequest -Headers $headers -Uri "$base/RELEASE-MANIFEST.json.sig" -OutFile (Join-Path $temp 'RELEASE-MANIFEST.json.sig')
        Invoke-WebRequest -Headers $headers -Uri "$base/PLIEGORS-CANDIDATE-RELEASE.pub.pem" -OutFile (Join-Path $temp 'PLIEGORS-CANDIDATE-RELEASE.pub.pem')
        $sealed = $true
    }
    $expected = ((Get-Content -Raw $checksumPath) -split '\s+')[0].ToLowerInvariant()
    if ($expected -notmatch '^[0-9a-f]{64}$') { throw "Invalid sha256 sidecar for $archive" }
    $actual = (Get-FileHash -LiteralPath $archiveFile -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw "sha256 mismatch for $archive" }
    if ($sealed) { Invoke-SealedSelectionVerification -Directory $temp -Archive $archive -ExpectedVersion $Version }
    $unpacked = Join-Path $temp 'unpacked'
    Expand-Archive -LiteralPath $archiveFile -DestinationPath $unpacked
    $candidate = Get-ChildItem -Path $unpacked -Recurse -File -Filter pliego.exe | Select-Object -First 1
    if (-not $candidate) { throw 'Archive lacks pliego.exe' }
    New-Item -ItemType Directory -Force $binDir, (Split-Path $backup) | Out-Null
    if (Test-Path -LiteralPath $binary -PathType Leaf) { Copy-Item -LiteralPath $binary -Destination $backup -Force }
    Copy-Item -LiteralPath $candidate.FullName -Destination "$binary.new" -Force
    Move-Item -LiteralPath "$binary.new" -Destination $binary -Force
    Write-Output "Installed PliegoRS $releaseLabel at $binary"
} finally {
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
