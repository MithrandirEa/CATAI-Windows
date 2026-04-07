# package.ps1 - Build release CATAI + ZIP archive
# Usage: .\package.ps1 [-Version "1.0.0"]
param([string]$Version = "1.0.0")

$ProjectRoot = $PSScriptRoot
Set-Location $ProjectRoot

Write-Host "=== CATAI $Version - Build release ===" -ForegroundColor Cyan

# --- 1. Compilation release ---
cargo build --release
if ($LASTEXITCODE -ne 0) { Write-Error "Echec cargo build --release (code $LASTEXITCODE)"; exit 1 }

$Exe = Join-Path $ProjectRoot "target\release\catai.exe"
if (-not (Test-Path $Exe)) { Write-Error "Introuvable : $Exe"; exit 1 }

$ExeSize = [math]::Round((Get-Item $Exe).Length / 1MB, 2)
Write-Host "  catai.exe : $ExeSize MB" -ForegroundColor Green

# --- 2. Staging directory ---
$Staging = Join-Path $ProjectRoot "dist\CATAI-$Version"
if (Test-Path $Staging) { Remove-Item $Staging -Recurse -Force }
New-Item $Staging -ItemType Directory | Out-Null

Copy-Item $Exe -Destination $Staging

$Assets = Join-Path $ProjectRoot "cute_orange_cat"
if (-not (Test-Path $Assets)) { Write-Error "Dossier assets introuvable : $Assets"; exit 1 }
Copy-Item $Assets -Destination $Staging -Recurse

Write-Host "  Assets copies depuis cute_orange_cat/" -ForegroundColor Green

# --- 3. ZIP archive ---
$ZipPath = Join-Path $ProjectRoot "dist\CATAI-$Version.zip"
if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }

Compress-Archive -Path $Staging -DestinationPath $ZipPath
$ZipSize = [math]::Round((Get-Item $ZipPath).Length / 1MB, 2)

Write-Host ""
Write-Host "=== Packaging termine ===" -ForegroundColor Cyan
Write-Host "  Archive : $ZipPath" -ForegroundColor Green
Write-Host "  Taille  : $ZipSize MB" -ForegroundColor Green
Write-Host ""
Write-Host "Contenu :"
Get-ChildItem $Staging | ForEach-Object { Write-Host "  $($_.Name)" }