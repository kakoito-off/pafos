<#
.SYNOPSIS
    Just build the pafos bootimage, without running QEMU.

.EXAMPLE
    .\build.ps1
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

if ($image) {
    Write-Host "Build OK: $($image.FullName)" -ForegroundColor Green
} else {
    Write-Host "Build finished, but no bootimage-*.bin was found under target\." -ForegroundColor Yellow
}
