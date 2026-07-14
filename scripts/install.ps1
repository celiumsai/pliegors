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
    }
    $expected = ((Get-Content -Raw $checksumPath) -split '\s+')[0].ToLowerInvariant()
    if ($expected -notmatch '^[0-9a-f]{64}$') { throw "Invalid sha256 sidecar for $archive" }
    $actual = (Get-FileHash -LiteralPath $archiveFile -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw "sha256 mismatch for $archive" }
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
