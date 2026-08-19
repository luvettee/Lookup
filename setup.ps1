# Lookup Windows Setup Script
# Requires PowerShell 5.1+

$ErrorActionPreference = "Stop"

function Pause-Setup {
    Write-Host ""
    Read-Host "Press Enter to close"
}

function Fail {
    param([string]$Message)

    Write-Host ""
    Write-Host "Error: $Message"
    Pause-Setup
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
        Write-Host "Cargo not found. Installing Rust..."

        $TempDir = Join-Path `
            ([System.IO.Path]::GetTempPath()) `
            ("lookup-" + [System.Guid]::NewGuid().ToString("N"))

        New-Item -ItemType Directory -Path $TempDir -Force | Out-Null
        $Installer = Join-Path $TempDir "rustup-init.exe"

        try {
            [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

            $WebClient = New-Object System.Net.WebClient
            $WebClient.DownloadFile(
                "https://win.rustup.rs/x86_64",
                $Installer
            )

            & $Installer -y --default-toolchain stable --profile minimal

            if ($LASTEXITCODE -ne 0) {
                Fail "Rust installation failed."
            }

            $UserCargo = Join-Path $HOME ".cargo\bin\cargo.exe"

            if (-not (Test-Path $UserCargo)) {
                Fail "cargo.exe was not found after installing Rust."
            }

            $CargoBin = $UserCargo
            $env:PATH = "$HOME\.cargo\bin;" + $env:PATH
        }
        finally {
            Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    Write-Host "Building Lookup..."

    Push-Location $ProjectDir

    try {
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

    Write-Host "Checking MCP server..."

    $InitJson = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
    $CheckOutput = $InitJson | & $LookupBin

    if (-not $CheckOutput -or -not ($CheckOutput -match '"serverInfo"')) {
        Fail "Lookup did not return a valid MCP initialize response."
    }

    $EscapedPath = $LookupBin.Replace('\', '\\')

    Write-Host ""
    Write-Host "Lookup is ready."
    Write-Host ""
    Write-Host "Add this to your MCP config:"
    Write-Host ""
    Write-Host "{"
    Write-Host '  "mcpServers": {'
    Write-Host '    "Lookup": {'
    Write-Host "      `"command`": `"$EscapedPath`","
    Write-Host '      "args": []'
    Write-Host '    }'
    Write-Host '  }'
    Write-Host "}"
}
catch {
    Fail $_.Exception.Message
}

Pause-Setup
