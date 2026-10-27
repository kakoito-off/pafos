<#
.SYNOPSIS
    Just build the pafos bootimage, without running QEMU.

.DESCRIPTION
    Extra arguments go to cargo, e.g. to pick which programs are built in.

.EXAMPLE
    .\build.ps1

.EXAMPLE
    .\build.ps1 --no-default-features --features shell,calc
#>

$env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")

Set-Location $PSScriptRoot

Write-Host "Building image..." -ForegroundColor Cyan
cargo build @args
if ($LASTEXITCODE -ne 0) {
    Write-Host "Build failed." -ForegroundColor Red
    exit 1
}
# The bootloader embeds the kernel and rebuilds only if the kernel file is
# newer than its last build. Switching back to a feature set cargo already
# has cached restores an old kernel with its old timestamp, and the image
# would silently keep the previous kernel. Bump the timestamp to prevent that.
(Get-Item "$PSScriptRoot\target\x86_64-pafos\debug\pafos").LastWriteTime = Get-Date
cargo bootimage @args
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
