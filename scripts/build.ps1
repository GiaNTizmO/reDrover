# reDrover build script - Windows.
#
# Builds both halves of the project and assembles a ready-to-ship folder:
#   1. Rust GUI installer  (reDrover.exe)        via cargo
#   2. C++ injected DLL    (version.dll, matching Discord bitness) via cmake + msvc
#   3. Stages everything (with dist/drover.ini and dist/strategies/*) into
#      build-output/ so the result is one drag-and-drop folder.
#
# Usage (from any directory):
#   pwsh -File scripts\build.ps1
#   pwsh -File scripts\build.ps1 -Config Debug
#   pwsh -File scripts\build.ps1 -Clean
#   pwsh -File scripts\build.ps1 -Skip Dll        # only build the GUI
#   pwsh -File scripts\build.ps1 -Skip Gui        # only build the DLL
#
# Requirements (the script checks these up front):
#   - cargo  (rustup default toolchain, stable)
#   - cmake  (3.20+)
#   - Visual Studio 2022 with the C++ x86/x64 build tools installed.
#     CMake's Visual Studio generator discovers the toolchain directly;
#     no VsDevCmd / Developer Prompt environment is required.

[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')]
    [string]$Config = 'Release',

    [ValidateSet('None', 'Gui', 'Dll')]
    [string]$Skip = 'None',

    # x64 matches current 64-bit Discord (since ~1.0.9000). Use Win32
    # only for legacy 32-bit Discord installs.
    [ValidateSet('x64', 'Win32')]
    [string]$Arch = 'x64',

    [switch]$Clean,
    [switch]$VerboseLog
)

$ErrorActionPreference = 'Stop'
$InformationPreference = 'Continue'

# --- Paths -------------------------------------------------------------------

$RepoRoot      = Resolve-Path (Join-Path $PSScriptRoot '..')
$DllDir        = Join-Path $RepoRoot 'dll'
$DllBuildDir   = Join-Path $DllDir 'build'
$DistDir       = Join-Path $RepoRoot 'dist'
$OutputDir     = Join-Path $RepoRoot 'build-output'
$CargoTargetDir = Join-Path $RepoRoot 'target'

function Write-Step($message) {
    Write-Host ''
    Write-Host "==> $message" -ForegroundColor Cyan
}

function Fail($message) {
    Write-Host "ERROR: $message" -ForegroundColor Red
    exit 1
}

function Test-Command($name) {
    [bool](Get-Command $name -ErrorAction SilentlyContinue)
}

# --- Toolchain detection -----------------------------------------------------

function Ensure-Cargo {
    if (-not (Test-Command 'cargo')) {
        Fail "cargo not found on PATH. Install Rust via https://rustup.rs/."
    }
    $version = (cargo --version) -join ' '
    Write-Host "cargo: $version"
}

function Ensure-CMake {
    if (-not (Test-Command 'cmake')) {
        Fail "cmake not found on PATH. Install CMake 3.20+ (https://cmake.org/download/)."
    }
    $version = (cmake --version | Select-Object -First 1)
    Write-Host "cmake: $version"
}

# --- Build steps -------------------------------------------------------------

function Invoke-Clean {
    if (Test-Path $DllBuildDir) {
        Write-Step "Removing $DllBuildDir"
        Remove-Item -Recurse -Force $DllBuildDir
    }
    if (Test-Path $CargoTargetDir) {
        Write-Step "Removing $CargoTargetDir"
        Remove-Item -Recurse -Force $CargoTargetDir
    }
    if (Test-Path $OutputDir) {
        Write-Step "Removing $OutputDir"
        Remove-Item -Recurse -Force $OutputDir
    }
}

function Build-Gui {
    Write-Step "Building Rust GUI ($Config) ..."
    Push-Location $RepoRoot
    try {
        $cargoArgs = @('build', '-p', 'redrover-gui')
        if ($Config -eq 'Release') { $cargoArgs += '--release' }
        if ($VerboseLog) { $cargoArgs += '--verbose' }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { Fail "cargo build failed (exit $LASTEXITCODE)." }
    }
    finally {
        Pop-Location
    }
}

function Build-Dll {
    Write-Step "Configuring C++ DLL ($Config, $Arch) ..."
    if (-not (Test-Path $DllBuildDir)) {
        New-Item -ItemType Directory -Path $DllBuildDir | Out-Null
    }

    Push-Location $DllDir
    try {
        $configureArgs = @('-S', '.', '-B', 'build', '-A', $Arch)
        & cmake @configureArgs
        if ($LASTEXITCODE -ne 0) { Fail "cmake configure failed (exit $LASTEXITCODE)." }

        Write-Step "Building C++ DLL ..."
        $buildArgs = @('--build', 'build', '--config', $Config)
        if ($VerboseLog) { $buildArgs += '--verbose' }
        & cmake @buildArgs
        if ($LASTEXITCODE -ne 0) { Fail "cmake build failed (exit $LASTEXITCODE)." }
    }
    finally {
        Pop-Location
    }
}

function Stage-Output {
    Write-Step "Staging artifacts into $OutputDir ..."

    if (-not (Test-Path $OutputDir)) {
        New-Item -ItemType Directory -Path $OutputDir | Out-Null
    }

    $cargoProfile = if ($Config -eq 'Release') { 'release' } else { 'debug' }
    $guiExe       = Join-Path $CargoTargetDir "$cargoProfile\reDrover.exe"
    $dllPath      = Join-Path $DllBuildDir   "$Config\version.dll"

    if ($Skip -ne 'Gui') {
        if (-not (Test-Path $guiExe)) {
            Fail "Expected GUI binary not found: $guiExe"
        }
        $legacyGuiExe = Get-ChildItem -Path $OutputDir -File -Filter '*.exe' |
            Where-Object { $_.Name -ceq 'redrover.exe' } |
            Select-Object -First 1
        if ($legacyGuiExe) {
            Remove-Item -Force -LiteralPath $legacyGuiExe.FullName
        }
        Copy-Item -Force $guiExe (Join-Path $OutputDir 'reDrover.exe')
    }
    if ($Skip -ne 'Dll') {
        if (-not (Test-Path $dllPath)) {
            Fail "Expected DLL not found: $dllPath"
        }
        Copy-Item -Force $dllPath (Join-Path $OutputDir 'version.dll')
    }

    # Reference config + curated payloads.
    Copy-Item -Force (Join-Path $DistDir 'drover.ini') (Join-Path $OutputDir 'drover.ini')

    # Optional UDP prefix payload — the GUI installer copies it into every
    # Discord app-* folder when present. Used by the `classic` UDP strategy.
    $packetSrc = Join-Path $DistDir 'drover-packet.bin'
    if (Test-Path $packetSrc) {
        Copy-Item -Force $packetSrc (Join-Path $OutputDir 'drover-packet.bin')
    }

    $stratSrc = Join-Path $DistDir 'strategies'
    if (Test-Path $stratSrc) {
        $stratDst = Join-Path $OutputDir 'strategies'
        if (Test-Path $stratDst) {
            Remove-Item -Recurse -Force -LiteralPath $stratDst
        }
        New-Item -ItemType Directory -Path $stratDst | Out-Null
        Get-ChildItem $stratSrc -File | Where-Object {
            $_.Extension -ne '.bin' -or $_.Length -gt 0
        } | ForEach-Object {
            Copy-Item -Force $_.FullName (Join-Path $stratDst $_.Name)
        }
    }
}

function Print-Summary {
    Write-Step 'Done.'
    Write-Host ''
    Write-Host 'Artifacts:' -ForegroundColor Green
    Get-ChildItem -Recurse $OutputDir | ForEach-Object {
        $relative = Resolve-Path -Relative $_.FullName
        if ($_.PSIsContainer) {
            Write-Host "  [dir]  $relative"
        } else {
            $sizeKb = [math]::Round($_.Length / 1KB, 1)
            Write-Host "         $relative  ($sizeKb KB)"
        }
    }
    Write-Host ''
    Write-Host "Run with: $(Join-Path $OutputDir 'reDrover.exe')" -ForegroundColor Yellow
}

# --- Main --------------------------------------------------------------------

Write-Host "reDrover build  ($Config)" -ForegroundColor White
Write-Host "Repo root: $RepoRoot"

if ($Clean) { Invoke-Clean }

if ($Skip -ne 'Gui') { Ensure-Cargo }
if ($Skip -ne 'Dll') {
    Ensure-CMake
}

if ($Skip -ne 'Gui') { Build-Gui }
if ($Skip -ne 'Dll') { Build-Dll }

Stage-Output
Print-Summary
