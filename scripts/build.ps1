# Redrover build script — Windows.
#
# Builds both halves of the project and assembles a ready-to-ship folder:
#   1. Rust GUI installer  (redrover.exe)        via cargo
#   2. C++ injected DLL    (version.dll, 32-bit) via cmake + msvc
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
#   - cl.exe / link.exe via a Visual Studio 2022 "x86" developer environment.
#     The script auto-detects VS via vswhere and imports the x86 dev env if
#     cl.exe isn't already on PATH.

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

# Locate Visual Studio's x86 developer environment if cl.exe isn't already
# usable. We don't ship VsDevCmd.bat invocations into every cmake call —
# instead we import the env vars once and reuse them.
function Ensure-VsX86Env {
    if (Test-Command 'cl.exe') {
        Write-Host 'cl.exe already on PATH — assuming a developer prompt.'
        return
    }

    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) {
        Fail @"
Visual Studio not found and cl.exe is not on PATH.
Install Visual Studio 2022 with the 'Desktop development with C++' workload
(and the C++ x86/x64 build tools component), then run this script again.
"@
    }

    $installPath = & $vswhere -latest -products '*' `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath
    if (-not $installPath) {
        Fail "vswhere couldn't locate a Visual Studio install with the C++ x86/x64 tools."
    }

    $vsDevCmd = Join-Path $installPath 'Common7\Tools\VsDevCmd.bat'
    if (-not (Test-Path $vsDevCmd)) {
        Fail "VsDevCmd.bat not found at $vsDevCmd."
    }

    $vsArch = if ($Arch -eq 'x64') { 'amd64' } else { 'x86' }
    Write-Host "Importing $vsArch dev environment from $installPath ..."
    # Run VsDevCmd in a child cmd.exe and pull every env var back into our session.
    $marker = '<<<REDROVER_ENV>>>'
    $cmd = "`"$vsDevCmd`" -arch=$vsArch -host_arch=x64 -no_logo && echo $marker && set"
    $output = & cmd.exe /c $cmd
    if ($LASTEXITCODE -ne 0) {
        Fail "VsDevCmd.bat failed (exit $LASTEXITCODE)."
    }

    $afterMarker = $false
    foreach ($line in $output) {
        if (-not $afterMarker) {
            if ($line -eq $marker) { $afterMarker = $true }
            continue
        }
        if ($line -match '^([^=]+)=(.*)$') {
            $name  = $matches[1]
            $value = $matches[2]
            Set-Item -Path ("Env:$name") -Value $value
        }
    }

    if (-not (Test-Command 'cl.exe')) {
        Fail "Imported VsDevCmd but cl.exe is still missing — please open the matching 'Native Tools Command Prompt for VS 2022' manually and re-run."
    }
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
    $guiExe       = Join-Path $CargoTargetDir "$cargoProfile\redrover.exe"
    $dllPath      = Join-Path $DllBuildDir   "$Config\version.dll"

    if ($Skip -ne 'Gui') {
        if (-not (Test-Path $guiExe)) {
            Fail "Expected GUI binary not found: $guiExe"
        }
        Copy-Item -Force $guiExe (Join-Path $OutputDir 'redrover.exe')
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
        if (-not (Test-Path $stratDst)) {
            New-Item -ItemType Directory -Path $stratDst | Out-Null
        }
        Get-ChildItem $stratSrc -File | ForEach-Object {
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
    Write-Host "Run with: $(Join-Path $OutputDir 'redrover.exe')" -ForegroundColor Yellow
}

# --- Main --------------------------------------------------------------------

Write-Host "Redrover build  ($Config)" -ForegroundColor White
Write-Host "Repo root: $RepoRoot"

if ($Clean) { Invoke-Clean }

if ($Skip -ne 'Gui') { Ensure-Cargo }
if ($Skip -ne 'Dll') {
    Ensure-CMake
    Ensure-VsX86Env
}

if ($Skip -ne 'Gui') { Build-Gui }
if ($Skip -ne 'Dll') { Build-Dll }

Stage-Output
Print-Summary
