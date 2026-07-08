# Apply government auto-deduction, labor premium, and leave balance patches.
# Run from repo root:
#   powershell -ExecutionPolicy Bypass -File patches/apply-all.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$patchRoot = Join-Path $PSScriptRoot 'replacements'

$mappings = @{
    'src_models_settings.rs' = 'src/models/settings.rs'
    'src_models_mod.rs' = 'src/models/mod.rs'
    'src_models_payroll_run.rs' = 'src/models/payroll_run.rs'
    'src_services_mod.rs' = 'src/services/mod.rs'
    'src_services_settings.rs' = 'src/services/settings.rs'
    'src_services_employees.rs' = 'src/services/employees.rs'
    'src_services_leave.rs' = 'src/services/leave.rs'
    'src_services_payroll_compute.rs' = 'src/services/payroll/compute.rs'
    'src_services_payroll_mod.rs' = 'src/services/payroll/mod.rs'
    'src_services_payroll_deductions.rs' = 'src/services/payroll/deductions.rs'
    'src_services_payroll_runs.rs' = 'src/services/payroll/runs.rs'
    'src_services_payroll_payslips.rs' = 'src/services/payroll/payslips.rs'
    'src_services_payroll_export.rs' = 'src/services/payroll/export.rs'
    'src_handlers_admin_settings.rs' = 'src/handlers/admin/settings.rs'
    'src_handlers_admin_payroll.rs' = 'src/handlers/admin/payroll.rs'
    'src_handlers_admin_mod.rs' = 'src/handlers/admin/mod.rs'
    'src_handlers_leave.rs' = 'src/handlers/leave.rs'
    'src_handlers_profile.rs' = 'src/handlers/profile.rs'
    'src_handlers_payslips.rs' = 'src/handlers/payslips.rs'
    'src_app.rs' = 'src/app.rs'
    'templates_admin_settings.html' = 'templates/admin/settings.html'
    'templates_admin_employee_profile.html' = 'templates/admin/employee_profile.html'
    'templates_admin_payroll_run.html' = 'templates/admin/payroll_run.html'
    'templates_admin_payroll.html' = 'templates/admin/payroll.html'
    'templates_admin_payroll_line_deductions.html' = 'templates/admin/payroll_line_deductions.html'
    'templates_employee_leave.html' = 'templates/employee/leave.html'
    'templates_manager_leave.html' = 'templates/manager/leave.html'
    'templates_payslip.html' = 'templates/payslip.html'
}

foreach ($entry in $mappings.GetEnumerator()) {
    $src = Join-Path $patchRoot $entry.Key
    $dst = Join-Path $root $entry.Value
    if (-not (Test-Path $src)) {
        Write-Warning "Skip missing $($entry.Key)"
        continue
    }
    $parent = Split-Path $dst -Parent
    if (-not (Test-Path $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
    Copy-Item $src $dst -Force
    Write-Host "Applied $($entry.Value)"
}

$cargoToml = Join-Path $root 'Cargo.toml'
$content = Get-Content $cargoToml -Raw
if ($content -notmatch 'build\s*=\s*"build\.rs"') {
    $content = $content -replace '(license = "MIT"\r?\n)', "`$1build = `"build.rs`"`n"
    Set-Content $cargoToml $content -Encoding UTF8 -NoNewline
    Write-Host 'Added build = "build.rs" to Cargo.toml'
}

Write-Host 'Patch apply complete. Run: cargo run --bin dtr-migrate && cargo test'