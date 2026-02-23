#Requires -Version 5.1
<#
.SYNOPSIS
    Idempotently sets up a static FFmpeg build and LLVM for compiling peaking-daemon.

.DESCRIPTION
    - Installs LLVM via winget (if not already present) — required by bindgen
    - Clones vcpkg to C:\vcpkg (if not already present)
    - Bootstraps vcpkg (if not already done)
    - Installs ffmpeg:x64-windows-static with NVENC support (if not already installed)
    - Sets FFMPEG_DIR, FFMPEG_STATIC, and LIBCLANG_PATH as persistent user environment variables

    Safe to run multiple times; each step is skipped if already complete.

.NOTES
    After running this script, open a new terminal before running `cargo build --release`
    so the updated environment variables are picked up.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$VcpkgRoot    = 'C:\vcpkg'
$VcpkgExe     = Join-Path $VcpkgRoot 'vcpkg.exe'
$FfmpegDir    = Join-Path $VcpkgRoot 'installed\x64-windows-static'
$LlvmBinDir   = 'C:\Program Files\LLVM\bin'
$LibclangPath = Join-Path $LlvmBinDir 'libclang.dll'

# Features needed by peaking-daemon:
#   avcodec    - NVENC H.264 and AAC encoding
#   avformat   - MP4 muxing
#   swresample - audio resampling
#   swscale    - pixel format conversion
#   nvcodec    - NVIDIA NVENC/NVDEC headers
# Note: avutil is an internal FFmpeg library included automatically, not a vcpkg feature.
$FfmpegFeatures = 'avcodec,avformat,swresample,swscale,nvcodec'
$FfmpegPort     = "ffmpeg[$FfmpegFeatures]:x64-windows-static"

function Write-Step([string]$Message) {
    Write-Host "`n==> $Message" -ForegroundColor Cyan
}

function Write-Skip([string]$Message) {
    Write-Host "    (skip) $Message" -ForegroundColor DarkGray
}

# ── Prerequisites ──────────────────────────────────────────────────────────────

Write-Step 'Checking prerequisites'

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Error 'git is not on PATH. Install Git for Windows first: https://git-scm.com/download/win'
}
Write-Host "    git $(git --version)"

if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    Write-Error 'winget is not available. Update Windows App Installer from the Microsoft Store.'
}

# ── LLVM (libclang — required by bindgen) ──────────────────────────────────────

Write-Step 'LLVM - install'

if (Test-Path $LibclangPath) {
    Write-Skip "libclang.dll already present at $LlvmBinDir"
} else {
    Write-Host '    Installing LLVM via winget ...'
    winget install --id LLVM.LLVM --exact --accept-source-agreements --accept-package-agreements
    if ($LASTEXITCODE -ne 0) { throw "winget install LLVM exited with code $LASTEXITCODE" }
}

# ── vcpkg clone ────────────────────────────────────────────────────────────────

Write-Step 'vcpkg - clone'

if (Test-Path $VcpkgRoot) {
    Write-Skip "Already present at $VcpkgRoot"
} else {
    Write-Host "    Cloning to $VcpkgRoot ..."
    git clone https://github.com/microsoft/vcpkg $VcpkgRoot
}

# ── vcpkg bootstrap ────────────────────────────────────────────────────────────

Write-Step 'vcpkg - bootstrap'

if (Test-Path $VcpkgExe) {
    Write-Skip 'vcpkg.exe already exists'
} else {
    Write-Host '    Bootstrapping vcpkg ...'
    Push-Location $VcpkgRoot
    try {
        cmd /c 'bootstrap-vcpkg.bat -disableMetrics'
        if ($LASTEXITCODE -ne 0) { throw "bootstrap-vcpkg.bat exited with code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
}

# ── FFmpeg static install ──────────────────────────────────────────────────────

Write-Step 'FFmpeg - install static build'

# Use the presence of avcodec.h as the install marker
$InstallMarker = Join-Path $FfmpegDir 'include\libavcodec\avcodec.h'

if (Test-Path $InstallMarker) {
    Write-Skip 'ffmpeg:x64-windows-static already installed'
} else {
    Write-Host "    Installing $FfmpegPort"
    Write-Host '    This downloads and compiles FFmpeg from source - expect 10 - 30 minutes.'
    & $VcpkgExe install $FfmpegPort
    if ($LASTEXITCODE -ne 0) { throw "vcpkg install exited with code $LASTEXITCODE" }
}

# ── Environment variables ──────────────────────────────────────────────────────

Write-Step 'Environment variables'

$currentDir       = [System.Environment]::GetEnvironmentVariable('FFMPEG_DIR',      'User')
$currentStatic    = [System.Environment]::GetEnvironmentVariable('FFMPEG_STATIC',   'User')
$currentLibclang  = [System.Environment]::GetEnvironmentVariable('LIBCLANG_PATH',   'User')

if ($currentDir -eq $FfmpegDir) {
    Write-Skip "FFMPEG_DIR already set to $FfmpegDir"
} else {
    [System.Environment]::SetEnvironmentVariable('FFMPEG_DIR', $FfmpegDir, 'User')
    Write-Host "    FFMPEG_DIR = $FfmpegDir"
}

if ($currentStatic -eq '1') {
    Write-Skip 'FFMPEG_STATIC already set to 1'
} else {
    [System.Environment]::SetEnvironmentVariable('FFMPEG_STATIC', '1', 'User')
    Write-Host '    FFMPEG_STATIC = 1'
}

if ($currentLibclang -eq $LlvmBinDir) {
    Write-Skip "LIBCLANG_PATH already set to $LlvmBinDir"
} else {
    [System.Environment]::SetEnvironmentVariable('LIBCLANG_PATH', $LlvmBinDir, 'User')
    Write-Host "    LIBCLANG_PATH = $LlvmBinDir"
}

# Also apply to the current session so the user can build immediately
# without opening a new terminal (though a new terminal is cleaner)
$env:FFMPEG_DIR    = $FfmpegDir
$env:FFMPEG_STATIC = '1'
$env:LIBCLANG_PATH = $LlvmBinDir

# ── Done ───────────────────────────────────────────────────────────────────────

Write-Host ''
Write-Host 'All done.' -ForegroundColor Green
Write-Host ''
Write-Host 'Next steps:'
Write-Host '  1. Open a new terminal (so the environment variables take effect system-wide)'
Write-Host '  2. cd into the daemon directory'
Write-Host '  3. cargo build --release'
