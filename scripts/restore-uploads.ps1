param(
    [Parameter(Mandatory = $true)]
    [string]$Archive,
    [string]$TargetDir = $(if ($env:UPLOAD_DIR) { $env:UPLOAD_DIR } else { "./uploads" }),
    [switch]$Yes
)

if (-not (Test-Path $Archive)) {
    throw "Archive not found: $Archive"
}

if (-not $Yes) {
    Write-Host "WARNING: This will replace files in $TargetDir"
    Write-Host "Archive: $Archive"
    $confirm = Read-Host "Type RESTORE to continue"
    if ($confirm -ne "RESTORE") {
        Write-Host "Aborted."
        exit 1
    }
}

$parent = Split-Path -Parent $TargetDir
if (-not (Test-Path $parent)) {
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
}
if (Test-Path $TargetDir) {
    Remove-Item -Recurse -Force $TargetDir
}

tar -xzf $Archive -C $parent

Write-Host "Uploads restored to $TargetDir from $Archive"