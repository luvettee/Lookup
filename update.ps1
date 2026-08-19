# Lookup Windows Updater Script
# Requires PowerShell 5.1+

$ErrorActionPreference = "Stop"

function Pause-Updater {
    Write-Host ""
    Read-Host "Press Enter to close"
}

function Fail {
    param([string]$Message)

    Write-Host ""
    Write-Host "Error: $Message"
    Pause-Updater
    exit 1
}

$ProjectDir = $PSScriptRoot

if (-not $ProjectDir) {
    $ProjectDir = (Get-Location).Path
}

try {
    $CargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
    $CargoBin = if ($CargoCmd) { $CargoCmd.Source } else { $null }

    if (-not $CargoBin) {
        $UserCargo = Join-Path $HOME ".cargo\bin\cargo.exe"

        if (Test-Path $UserCargo) {
            $CargoBin = $UserCargo
            $env:PATH = "$HOME\.cargo\bin;" + $env:PATH
        }
    }

    if (-not $CargoBin) {
        Fail "Cargo was not found. Run setup.ps1 first."
    }

    Push-Location $ProjectDir

    try {
        if (Test-Path (Join-Path $ProjectDir ".git")) {
            $GitCmd = Get-Command git -ErrorAction SilentlyContinue

            if ($GitCmd) {
                Write-Host "Checking for updates..."

                & $GitCmd.Source pull --ff-only

                if ($LASTEXITCODE -ne 0) {
                    Fail "Git pull failed."
                }
            }
            else {
                Write-Host "Git was not found, skipping pull."
            }
        }

        Write-Host "Building Lookup..."

        & $CargoBin build --release

        if ($LASTEXITCODE -ne 0) {
            Fail "Build failed."
        }
    }
    finally {
        Pop-Location
    }

    $LookupBin = Join-Path $ProjectDir "target\release\lookup.exe"

    if (-not (Test-Path $LookupBin)) {
        Fail "lookup.exe was not found."
    }

    Write-Host "Checking Lookup..."

    $InitJson = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
    $CheckOutput = $InitJson | & $LookupBin

    if (-not $CheckOutput -or -not ($CheckOutput -match '"serverInfo"')) {
        Fail "Lookup did not return a valid MCP initialize response."
    }

    Write-Host ""
    Write-Host "Lookup is up to date."
}
catch {
    Fail $_.Exception.Message
}

Pause-Updater
