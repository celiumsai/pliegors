# SPDX-License-Identifier: Apache-2.0
[CmdletBinding()]
param(
    [string]$ChromeDriver = $env:CHROMEDRIVER
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$repo = Split-Path -Parent $PSScriptRoot

if ([string]::IsNullOrWhiteSpace($ChromeDriver)) {
    $driverCommand = Get-Command chromedriver -ErrorAction SilentlyContinue
    if ($null -ne $driverCommand) {
        $ChromeDriver = $driverCommand.Source
    } else {
        throw "ChromeDriver was not found. Set CHROMEDRIVER to a driver matching local Chrome."
    }
}

$runner = Get-Command wasm-bindgen-test-runner -ErrorAction SilentlyContinue
if ($null -eq $runner) {
    throw "wasm-bindgen-test-runner 0.2.126 is required on PATH."
}

$env:CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER = $runner.Source
$env:CHROMEDRIVER = (Resolve-Path -LiteralPath $ChromeDriver).Path
$env:WASM_BINDGEN_TEST_WEBDRIVER_JSON = Join-Path $repo "crates/pliego-dom/tests/webdriver.json"
$env:WASM_BINDGEN_TEST_DRIVER_TIMEOUT = "15"
$env:WASM_BINDGEN_TEST_TIMEOUT = "120"
Remove-Item Env:NO_HEADLESS -ErrorAction SilentlyContinue

Push-Location $repo
try {
    $driverVersion = & $env:CHROMEDRIVER --version
    if ($LASTEXITCODE -ne 0) {
        throw "ChromeDriver version check failed with exit code $LASTEXITCODE."
    }
    Write-Output $driverVersion
    $runnerVersion = & $runner.Source --version
    if ($LASTEXITCODE -ne 0) {
        throw "wasm-bindgen-test-runner version check failed with exit code $LASTEXITCODE."
    }
    if ($runnerVersion -notmatch "0\.2\.126") {
        throw "Expected wasm-bindgen-test-runner 0.2.126, found: $runnerVersion"
    }
    Write-Output $runnerVersion
    & cargo test -p pliego-dom --target wasm32-unknown-unknown --test browser_lifecycle --locked --offline -- --nocapture
    if ($LASTEXITCODE -ne 0) {
        throw "Browser lifecycle tests failed with exit code $LASTEXITCODE."
    }
} finally {
    Pop-Location
}
