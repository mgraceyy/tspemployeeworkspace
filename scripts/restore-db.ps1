param(
    [Parameter(Mandatory = $true)]
    [string]$Backup,
    [switch]$Yes
)

if (-not (Test-Path $Backup)) {
    throw "Backup file not found: $Backup"
}

if (-not $Yes) {
    Write-Host "WARNING: This will replace data in the target database."
    Write-Host "Backup file: $Backup"
    $confirm = Read-Host "Type RESTORE to continue"
    if ($confirm -ne "RESTORE") {
        Write-Host "Aborted."
        exit 1
    }
}

$running = docker compose ps db --status running 2>$null
if ($LASTEXITCODE -eq 0) {
    docker compose exec -T db psql -U dtr -d postgres -v ON_ERROR_STOP=1 `
        -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = 'dtr' AND pid <> pg_backend_pid();"
    docker compose exec -T db psql -U dtr -d postgres -v ON_ERROR_STOP=1 -c "DROP DATABASE IF EXISTS dtr;"
    docker compose exec -T db psql -U dtr -d postgres -v ON_ERROR_STOP=1 -c "CREATE DATABASE dtr;"
    Get-Content $Backup | docker compose exec -T db psql -U dtr -d dtr -v ON_ERROR_STOP=1
} else {
    if (-not $env:DATABASE_URL) {
        throw "DATABASE_URL must be set when Docker Compose db is not running"
    }
    psql $env:DATABASE_URL -v ON_ERROR_STOP=1 -f $Backup
}

Write-Host "Restore completed from $Backup"