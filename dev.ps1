<#
.SYNOPSIS
  Launches Parzi in isolated development mode side-by-side with your running Parzi instance.

.DESCRIPTION
  This script isolates:
  1. CARGO_TARGET_DIR -> target-dev (so it never locks or conflicts with target/debug/parzi-app.exe)
  2. PARZI_HOME -> ~/.parzi-dev (separate session store, database, and logs)
  3. App identifier -> com.parzi.app.dev via tauri.dev.conf.json (bypasses single-instance check)
#>

$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $RepoRoot

$Env:CARGO_TARGET_DIR = "$RepoRoot\target-dev"
$HomeDir = [System.Environment]::GetFolderPath('UserProfile')
$Env:PARZI_HOME = "$HomeDir\.parzi-dev"

if (-not (Test-Path $Env:PARZI_HOME)) {
    New-Item -ItemType Directory -Path $Env:PARZI_HOME -Force | Out-Null
}

# Agents Parzi installs itself (Antigravity) live under the real PARZI_HOME.
# Share them, so the dev instance offers the same agents and models; their
# sign-ins are the agents' own and already shared.
$DevAgents = "$Env:PARZI_HOME\agents"
$RealAgents = "$HomeDir\.parzi\agents"
if (-not (Test-Path $DevAgents) -and (Test-Path $RealAgents)) {
    New-Item -ItemType Junction -Path $DevAgents -Target $RealAgents | Out-Null
}

Write-Host "Starting Parzi in Isolated Dev Mode..." -ForegroundColor Cyan
Write-Host "  CARGO_TARGET_DIR: $Env:CARGO_TARGET_DIR" -ForegroundColor Gray
Write-Host "  PARZI_HOME:       $Env:PARZI_HOME" -ForegroundColor Gray
Write-Host "  Identifier:       com.parzi.app.dev (tauri.dev.conf.json)" -ForegroundColor Gray

# Run tauri dev with the dev config overlay via npx or cargo
if (Get-Command "cargo-tauri" -ErrorAction SilentlyContinue) {
    cargo tauri dev --config src-tauri/tauri.dev.conf.json
} else {
    npx --yes @tauri-apps/cli dev --config src-tauri/tauri.dev.conf.json
}
