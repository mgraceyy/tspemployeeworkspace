# Applies government auto-deduction wiring. Run from repo root:
#   powershell -ExecutionPolicy Bypass -File patches/apply-government-deductions.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

function Copy-Replacement($relPath) {
    $src = Join-Path $PSScriptRoot "replacements" ($relPath -replace '/', '\')
    $dst = Join-Path $root $relPath
    if (-not (Test-Path $src)) {
        throw "Missing replacement: $src"
    }
    Copy-Item $src $dst -Force
    Write-Host "Applied $relPath"
}

$replacements = @(
    'src/models/settings.rs',
    'src/services/settings.rs',
    'src/services/payroll/mod.rs'
)

foreach ($r in $replacements) { Copy-Replacement $r }

Write-Host 'Done. Apply template/handler patches from patches/government_auto_deductions_wiring.md manually if not yet copied.'