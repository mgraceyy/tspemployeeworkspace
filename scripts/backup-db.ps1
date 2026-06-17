param(
    [string]$Output = "backup-dtr-$(Get-Date -Format 'yyyyMMdd_HHmmss').sql"
)

$running = docker compose ps db --status running 2>$null
if ($LASTEXITCODE -eq 0) {
    docker compose exec -T db pg_dump -U dtr dtr | Set-Content -Encoding utf8 $Output
} else {
    if (-not $env:DATABASE_URL) {
        throw "DATABASE_URL must be set when Docker Compose db is not running"
    }
    pg_dump $env:DATABASE_URL | Set-Content -Encoding utf8 $Output
}

Write-Host "Backup written to $Output"