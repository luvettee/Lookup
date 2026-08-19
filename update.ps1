# Lookup Windows Updater Script
# Requires PowerShell 5.1+

$ErrorActionPreference = "Stop"

function Write-Info {
    param([string]$Message)
    Write-Host $Message
}

function Write-Fatal {
    param([string]$Message)
    Write-Error $Message
    exit 1
}

$ProjectDir = $PSScriptRoot
if (-not $ProjectDir) {
    $ProjectDir = (Get-Location).Path
}

$CargoCmd = Get-Command "cargo" -ErrorAction SilentlyContinue
$CargoBin = if ($CargoCmd) { $CargoCmd.Source } else { $null }

if (-not $CargoBin) {
    $UserCargo = Join-Path $HOME ".cargo\bin\cargo.exe"
    if (Test-Path $UserCargo) {
        $CargoBin = $UserCargo
        $env:PATH = "$HOME\.cargo\bin;" + $env:PATH
    }
}

if (-not $CargoBin) {
    Write-Fatal "Cargo is required to rebuild Lookup. Run .\setup.ps1 first."
}

Write-Info "Updating and building Lookup..."

Push-Location $ProjectDir
try {
    if (Test-Path (Join-Path $ProjectDir ".git")) {
        $GitCmd = Get-Command "git" -ErrorAction SilentlyContinue
        if ($GitCmd) {
            Write-Info "Pulling latest changes from git..."
            git pull --ff-only 2>$null
        }
    }

    & $CargoBin build --release
    if ($LASTEXITCODE -ne 0) {
        Write-Fatal "Cargo build failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

$LookupBin = Join-Path $ProjectDir "target\release\lookup.exe"
if (-not (Test-Path $LookupBin)) {
    Write-Fatal "Lookup binary was not found at $LookupBin."
}

Write-Info "Validating new binary..."
$InitJson = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
$CheckOutput = $InitJson | & $LookupBin

if (-not $CheckOutput -or -not ($CheckOutput -match '"serverInfo"')) {
    Write-Fatal "MCP startup check did not receive a valid initialize response."
}

Write-Info "Lookup is up to date and ready."
