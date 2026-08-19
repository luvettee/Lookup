# Lookup Windows Setup Script
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

# 1. Locate or install Cargo
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
    Write-Info "Rust/Cargo was not found; downloading rustup-init for Windows..."
    
    $TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("lookup-setup-" + [System.Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $TempDir -Force | Out-Null
    $Installer = Join-Path $TempDir "rustup-init.exe"

    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        $WebClient = New-Object System.Net.WebClient
        $WebClient.DownloadFile("https://win.rustup.rs/x86_64", $Installer)

        Write-Info "Installing Rust toolchain (minimal profile)..."
        $Process = Start-Process -FilePath $Installer -ArgumentList "-y", "--default-toolchain", "stable", "--profile", "minimal" -NoNewWindow -Wait -PassThru

        if ($Process.ExitCode -ne 0) {
            Write-Fatal "rustup-init failed with exit code $($Process.ExitCode)."
        }

        $UserCargo = Join-Path $HOME ".cargo\bin\cargo.exe"
        if (Test-Path $UserCargo) {
            $CargoBin = $UserCargo
            $env:PATH = "$HOME\.cargo\bin;" + $env:PATH
        } else {
            Write-Fatal "Rust installed, but cargo.exe could not be found at $UserCargo."
        }
    }
    finally {
        if (Test-Path $TempDir) {
            Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
        }
    }
}

Write-Info "Using Cargo: $CargoBin"
Write-Info "Building Lookup release binary..."

Push-Location $ProjectDir
try {
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

Write-Info "Running an MCP startup check..."
$InitJson = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
$CheckOutput = $InitJson | & $LookupBin

if (-not $CheckOutput -or -not ($CheckOutput -match '"serverInfo"')) {
    Write-Fatal "MCP startup check did not receive a valid initialize response."
}

# Format escaped path for JSON
$EscapedPath = $LookupBin.Replace('\', '\\')

Write-Info ""
Write-Info "Lookup is ready."
Write-Info ""
Write-Info "Configuration:"
Write-Info "{"
Write-Info '  "mcpServers": {'
Write-Info '    "Lookup": {'
Write-Info "      `"command`": `"$EscapedPath`","
Write-Info '      "args": []'
Write-Info '    }'
Write-Info '  }'
Write-Info "}"
