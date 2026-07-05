# sign-athanor.ps1 - Sign Athanor Lite binaries with Azure Trusted Signing
# Prerequisites:
#   1. winget install -e --id Microsoft.Azure.ArtifactSigningClientTools
#   2. az login (Azure CLI authenticated as tonywinslow@bbasecure.com)
#
# Usage: .\sign-athanor.ps1 [-Target <path>]
#   Default target: ..\athanor-lite\src-tauri\target\release\bundle

param(
    [string]$Target
)

$ErrorActionPreference = "Stop"

# Paths
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$MetadataJson = Join-Path $ScriptDir "metadata.json"

# Default install location for Artifact Signing Client Tools
$DlibPaths = @(
    "${env:LOCALAPPDATA}\Microsoft\MicrosoftArtifactSigningClientTools\Azure.CodeSigning.Dlib.dll",
    "${env:LOCALAPPDATA}\Microsoft\MicrosoftTrustedSigningClientTools\Azure.CodeSigning.Dlib.dll",
    "${env:ProgramFiles}\Azure\ArtifactSigningClientTools\x64\Azure.CodeSigning.Dlib.dll",
    "${env:ProgramFiles(x86)}\Azure\ArtifactSigningClientTools\x64\Azure.CodeSigning.Dlib.dll"
)

$SignToolPaths = @(
    "${env:LOCALAPPDATA}\Microsoft\MicrosoftArtifactSigningClientTools\signtool.exe",
    "${env:LOCALAPPDATA}\Microsoft\MicrosoftTrustedSigningClientTools\signtool.exe",
    "${env:ProgramFiles}\Azure\ArtifactSigningClientTools\x64\signtool.exe",
    "${env:ProgramFiles(x86)}\Azure\ArtifactSigningClientTools\x64\signtool.exe"
)

# Find dlib
$Dlib = $DlibPaths | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $Dlib) {
    Write-Error "Azure.CodeSigning.Dlib.dll not found. Install: winget install -e --id Microsoft.Azure.ArtifactSigningClientTools"
    exit 1
}

# Find signtool (prefer the one from Artifact Signing Client Tools, fall back to SDK)
$SignTool = $SignToolPaths | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $SignTool) {
    # Fall back to Windows SDK signtool
    $SdkSignTool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
        Sort-Object { [version]($_.Directory.Parent.Name) } -Descending |
        Select-Object -First 1
    if ($SdkSignTool) { $SignTool = $SdkSignTool.FullName }
}
if (-not $SignTool) {
    Write-Error "signtool.exe not found. Install Windows SDK or Artifact Signing Client Tools."
    exit 1
}

Write-Host "SignTool: $SignTool" -ForegroundColor Cyan
Write-Host "Dlib:     $Dlib" -ForegroundColor Cyan
Write-Host "Metadata: $MetadataJson" -ForegroundColor Cyan

# Default target: Athanor Lite build output
if (-not $Target) {
    $Target = Join-Path (Split-Path $ScriptDir -Parent) "athanor-lite\src-tauri\target\release\bundle"
}

# Find files to sign
$FilesToSign = @()

if (Test-Path $Target -PathType Leaf) {
    $FilesToSign += $Target
} elseif (Test-Path $Target -PathType Container) {
    # Sign exe and msi files in the bundle directory
    $FilesToSign += Get-ChildItem -Path $Target -Recurse -Include "*.exe","*.msi" | ForEach-Object { $_.FullName }
} else {
    Write-Error "Target not found: $Target"
    exit 1
}

if ($FilesToSign.Count -eq 0) {
    Write-Error "No .exe or .msi files found in: $Target"
    exit 1
}

Write-Host "`nSigning $($FilesToSign.Count) file(s):" -ForegroundColor Green
$FilesToSign | ForEach-Object { Write-Host "  $_" -ForegroundColor Gray }

# Sign each file
foreach ($File in $FilesToSign) {
    Write-Host "`nSigning: $(Split-Path $File -Leaf)" -ForegroundColor Yellow
    & $SignTool sign /v /fd SHA256 `
        /tr "http://timestamp.acs.microsoft.com" /td SHA256 `
        /dlib $Dlib `
        /dmdf $MetadataJson `
        $File

    if ($LASTEXITCODE -ne 0) {
        Write-Error "Failed to sign: $File"
        exit 1
    }
    Write-Host "Signed: $(Split-Path $File -Leaf)" -ForegroundColor Green
}

Write-Host "`nAll files signed successfully." -ForegroundColor Green

# Verify signatures
Write-Host "`nVerifying signatures..." -ForegroundColor Cyan
foreach ($File in $FilesToSign) {
    & $SignTool verify /pa /v $File
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "Verification issue with: $File"
    }
}
