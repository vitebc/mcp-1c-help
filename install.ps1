#!/usr/bin/env pwsh
param(
    [string]$Tag = "latest",
    [string]$InstallDir = ""
)

$Repo = "vitebc/mcp-1c-help"
$BinName = "mcp-1c-help"

$ErrorActionPreference = "Stop"

# --- detect arch ---
$Arch = switch ([Environment]::Is64BitOperatingSystem) {
    $true  { "x86_64" }
    $false { "i686"   }
}

$Asset = "${BinName}-windows-${Arch}.zip"
$InstallDir = if ($InstallDir -eq "") { "${env:LOCALAPPDATA}\${BinName}" } else { $InstallDir }

# --- resolve download url ---
if ($Tag -eq "latest") {
    $apiUrl = "https://api.github.com/repos/$Repo/releases/latest"
    $release = Invoke-RestMethod -Uri $apiUrl -UseBasicParsing
    $assetInfo = $release.assets | Where-Object { $_.name -eq $Asset }
    if (-not $assetInfo) {
        Write-Error "Asset not found: $Asset"
        exit 1
    }
    $downloadUrl = $assetInfo.browser_download_url
} else {
    $downloadUrl = "https://github.com/$Repo/releases/download/$Tag/$Asset"
}

Write-Host "==> Downloading $BinName $Tag..." -ForegroundColor Green
$tmpDir = Join-Path $env:TEMP "mcp-1c-help-install"
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
$zipPath = Join-Path $tmpDir $Asset

Invoke-WebRequest -Uri $downloadUrl -OutFile $zipPath

Write-Host "==> Extracting..." -ForegroundColor Green
Expand-Archive -Path $zipPath -DestinationPath $tmpDir -Force

Write-Host "==> Installing to $InstallDir" -ForegroundColor Green
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
Copy-Item -Path (Join-Path $tmpDir "$BinName.exe") -Destination $InstallDir -Force

# --- add to PATH ---
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$InstallDir*") {
    $newPath = "$InstallDir;$userPath"
    [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
    Write-Host "==> Added $InstallDir to user PATH" -ForegroundColor Green
    # Update current session
    $env:PATH = "$InstallDir;$env:PATH"
}

Write-Host "==> Installed! Run '$BinName --help' to get started." -ForegroundColor Green

# --- cleanup ---
Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
