<#
.SYNOPSIS
    Build (if needed) and run pafos in QEMU.

.EXAMPLE
    .\run.ps1
#>

$env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")

Set-Location $PSScriptRoot

Write-Host "Building image..." -ForegroundColor Cyan
cargo bootimage
if ($LASTEXITCODE -ne 0) {
    Write-Host "Build failed." -ForegroundColor Red
    exit 1
}

$image = Get-ChildItem -Path "$PSScriptRoot\target" -Recurse -Filter "bootimage-*.bin" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1

if (-not $image) {
    Write-Host "Image not found under target\...\bootimage-*.bin" -ForegroundColor Red
    exit 1
}

$qemuExe = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-not (Test-Path $qemuExe)) {
    $qemuExe = "qemu-system-x86_64"
}

Write-Host "Starting QEMU: $($image.FullName)" -ForegroundColor Cyan
& $qemuExe -drive "format=raw,file=$($image.FullName)"
