# Run as Administrator: right-click PowerShell -> Run as administrator, then:
#   Set-Location D:\talasoraprime\dtr
#   powershell -ExecutionPolicy Bypass -File patches\fix-permissions-and-apply.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

Write-Host "Fixing ACLs on $root ..."
icacls $root /grant "${env:USERNAME}:(OI)(CI)F" /T
icacls (Join-Path $root 'src') /grant 'BUILTIN\Users:(OI)(CI)M' /T
icacls (Join-Path $root 'templates') /grant 'BUILTIN\Users:(OI)(CI)M' /T
icacls (Join-Path $root 'Cargo.toml') /grant "${env:USERNAME}:F"

& (Join-Path $PSScriptRoot 'apply-all.ps1')

Write-Host 'Done. Run: $env:CARGO_TARGET_DIR="$env:TEMP\dtr-target"; cargo test'