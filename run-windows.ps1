<#
One-shot build and launch for Windows (PowerShell 5.1 or 7+).
Double-click run-windows.cmd, or from a terminal:

  .\run-windows.ps1              build everything, start on http://127.0.0.1:8610
  .\run-windows.ps1 -Demo        also seed a DEMO project on first start
  .\run-windows.ps1 -Port 9000   use another port
  .\run-windows.ps1 -NoBuild     start the last build without rebuilding
  .\run-windows.ps1 -NoOpen      don't open a browser

Data lives in .\data\kanban.db (override with $env:KANBAN_DB). Ctrl+C stops it.
#>
param(
    [switch]$Demo,
    [switch]$NoOpen,
    [switch]$NoBuild,
    [int]$Port = 8610
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

function Say($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Die($msg) { Write-Host "error: $msg" -ForegroundColor Red; exit 1 }
function Need($cmd, $hint) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) { Die "$cmd not found. $hint" }
}
function Check($what) {
    if ($LASTEXITCODE -ne 0) { Die "$what failed (exit code $LASTEXITCODE)" }
}

# rustup installs here but only updates PATH for new terminals.
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:PATH = "$cargoBin;$env:PATH" }

$bin = Join-Path $PSScriptRoot 'server\target\release\kanban-server.exe'

if (-not $NoBuild) {
    Need cargo 'Install Rust from https://rustup.rs  (or: winget install Rustlang.Rustup)'
    Need node 'Install Node.js 20.19+ from https://nodejs.org  (or: winget install OpenJS.NodeJS.LTS)'
    Need npm 'npm comes with Node.js: https://nodejs.org'

    $v = (node -p "process.versions.node").Split('.') | ForEach-Object { [int]$_ }
    $nodeOk = $v[0] -gt 22 -or ($v[0] -eq 22 -and $v[1] -ge 12) -or ($v[0] -eq 20 -and $v[1] -ge 19)
    if (-not $nodeOk) { Die "Node.js $(node -v) is too old; the frontend build needs 20.19+ or 22.12+" }

    # The default Rust toolchain on Windows links with the MSVC C++ build tools.
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $hasMsvc = (Test-Path $vswhere) -and (& $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
    if (-not $hasMsvc) {
        Write-Host 'warning: Visual Studio C++ build tools not found; the Rust build will fail to link without them.' -ForegroundColor Yellow
        Write-Host '  Install: winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"' -ForegroundColor Yellow
    }

    Say 'Building the frontend'
    Push-Location web
    try {
        $stamp = 'node_modules\.package-lock.json'
        if (-not (Test-Path $stamp) -or (Get-Item 'package-lock.json').LastWriteTime -gt (Get-Item $stamp).LastWriteTime) {
            npm ci --no-audit --no-fund; Check 'npm ci'
        }
        npm run build; Check 'Frontend build'
    } finally { Pop-Location }

    Say 'Building the server (the first build takes a few minutes)'
    Push-Location server
    try { cargo build --release --locked; Check 'Server build' } finally { Pop-Location }
} elseif (-not (Test-Path $bin)) {
    Die "no build found at $bin; run without -NoBuild first"
}

New-Item -ItemType Directory -Force (Join-Path $PSScriptRoot 'data') | Out-Null
if (-not $env:KANBAN_DB) { $env:KANBAN_DB = Join-Path $PSScriptRoot 'data\kanban.db' }
$env:KANBAN_PORT = "$Port"
$url = "http://127.0.0.1:$Port"

Say "Starting on $url (database: $env:KANBAN_DB). Ctrl+C to stop."
$start = @{ FilePath = $bin; NoNewWindow = $true; PassThru = $true }
if ($Demo) { $start.ArgumentList = @('--demo') }
$server = Start-Process @start

if (-not $NoOpen) {
    for ($i = 0; $i -lt 80 -and -not $server.HasExited; $i++) {
        & $bin healthcheck
        if ($LASTEXITCODE -eq 0) { Start-Process $url; break }
        Start-Sleep -Milliseconds 250
    }
}

$server.WaitForExit()
exit $server.ExitCode
