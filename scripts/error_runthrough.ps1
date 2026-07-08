# Live error-path runthrough against http://localhost:8080
# Requires test accounts ADMIN/1234 and SAMPLE01/7391

$Base = "http://localhost:8080"
$ErrorActionPreference = "Stop"

function Get-Csrf($html) {
    if ($html -match 'name="csrf_token" value="([^"]+)"') { return $Matches[1] }
    throw "CSRF token not found"
}

function Login($code, $pin) {
    $session = New-Object Microsoft.PowerShell.Commands.WebRequestSession
    $login = Invoke-WebRequest -Uri "$Base/login" -WebSession $session -UseBasicParsing
    $csrf = Get-Csrf $login.Content
    $body = "employee_code=$code&pin=$pin&csrf_token=$csrf"
    $null = Invoke-WebRequest -Uri "$Base/login" -Method POST -WebSession $session -UseBasicParsing `
        -ContentType "application/x-www-form-urlencoded" -Body $body -MaximumRedirection 0 -ErrorAction SilentlyContinue
    return $session
}

function Post-Form($session, $path, $fields) {
    $page = Invoke-WebRequest -Uri "$Base$path" -WebSession $session -UseBasicParsing
    $csrf = Get-Csrf $page.Content
    $pairs = @("csrf_token=$csrf")
    foreach ($kv in $fields.GetEnumerator()) { $pairs += "$($kv.Key)=$([uri]::EscapeDataString([string]$kv.Value))" }
    $body = $pairs -join "&"
    try {
        $resp = Invoke-WebRequest -Uri "$Base$path" -Method POST -WebSession $session -UseBasicParsing `
            -ContentType "application/x-www-form-urlencoded" -Body $body -MaximumRedirection 0
        return @{ Status = $resp.StatusCode; Location = $resp.Headers.Location; Body = $resp.Content }
    } catch {
        $r = $_.Exception.Response
        if ($r) {
            $loc = $r.Headers["Location"]
            return @{ Status = [int]$r.StatusCode; Location = $loc; Body = "" }
        }
        throw
    }
}

function Follow-And-Check($session, $path, $patterns) {
    $page = Invoke-WebRequest -Uri "$Base$path" -WebSession $session -UseBasicParsing
    $hits = @()
    foreach ($p in $patterns) {
        if ($page.Content -match $p) { $hits += $p }
    }
    return @{ Path = $path; Status = $page.StatusCode; Hits = $hits; Ok = ($hits.Count -gt 0) }
}

$results = @()

Write-Host "=== Health ===" -ForegroundColor Cyan
$h = Invoke-WebRequest -Uri "$Base/health" -UseBasicParsing
$results += [pscustomobject]@{ Area = "Health"; Test = "GET /health"; Pass = ($h.StatusCode -eq 200); Detail = $h.Content }

Write-Host "=== Employee clock errors ===" -ForegroundColor Cyan
$emp = Login "SAMPLE01" "7391"
# Clock in (may already be clocked in from prior state)
$clockPage = Invoke-WebRequest -Uri "$Base/" -WebSession $emp -UseBasicParsing
$csrf = Get-Csrf $clockPage.Content
$body = "csrf_token=$csrf"
try {
    $post = Invoke-WebRequest -Uri "$Base/clock/in" -Method POST -WebSession $emp -UseBasicParsing `
        -ContentType "application/x-www-form-urlencoded" -Body $body -MaximumRedirection 0 -ErrorAction SilentlyContinue
    $clkStatus = [int]$post.StatusCode
} catch {
    $clkStatus = [int]$_.Exception.Response.StatusCode
}
$check1 = Follow-And-Check $emp "/" @("alert-error", "alert-success", "Clocked in", "Already clocked", "completed")
$results += [pscustomobject]@{ Area = "Clock"; Test = "POST /clock/in"; Pass = ($clkStatus -in 302,303); Detail = "status=$clkStatus; flash=$($check1.Hits -join ',')" }

$clockPage2 = Invoke-WebRequest -Uri "$Base/" -WebSession $emp -UseBasicParsing
$csrf2 = Get-Csrf $clockPage2.Content
$body2 = "csrf_token=$csrf2"
try {
    $null = Invoke-WebRequest -Uri "$Base/clock/in" -Method POST -WebSession $emp -UseBasicParsing `
        -ContentType "application/x-www-form-urlencoded" -Body $body2 -MaximumRedirection 0 -ErrorAction SilentlyContinue
} catch { }
$check2 = Follow-And-Check $emp "/" @("alert-error", "Already clocked", "Already completed")
$results += [pscustomobject]@{ Area = "Clock"; Test = "Double clock-in shows flash not 400"; Pass = ($post2.Status -in 302,303) -and $check2.Ok; Detail = "status=$($post2.Status); flash=$($check2.Hits -join ',')" }

Write-Host "=== Admin pages load ===" -ForegroundColor Cyan
$admin = Login "ADMIN" "1234"
foreach ($p in @("/manager", "/admin/employees", "/admin/payroll", "/admin/reports", "/admin/settings")) {
    $page = Invoke-WebRequest -Uri "$Base$p" -WebSession $admin -UseBasicParsing
    $results += [pscustomobject]@{ Area = "Admin"; Test = "GET $p"; Pass = ($page.StatusCode -eq 200); Detail = "len=$($page.Content.Length)" }
}

Write-Host "=== Create employee validation ===" -ForegroundColor Cyan
$page = Invoke-WebRequest -Uri "$Base/admin/employees" -WebSession $admin -UseBasicParsing
$csrf = Get-Csrf $page.Content
$body = "employee_code=ADMIN&full_name=Dup&pin=12&department=Test&role=employee&csrf_token=$csrf"
try {
    $resp = Invoke-WebRequest -Uri "$Base/admin/employees" -Method POST -WebSession $admin -UseBasicParsing `
        -ContentType "application/x-www-form-urlencoded" -Body $body -MaximumRedirection 0 -ErrorAction SilentlyContinue
    $loc = $resp.Headers.Location
} catch {
    $loc = $_.Exception.Response.Headers["Location"]
}
$dupPage = Invoke-WebRequest -Uri "$Base$loc" -WebSession $admin -UseBasicParsing
$hasFlash = $dupPage.Content -match "alert-error"
$hasAdd = $loc -match "add=1"
$results += [pscustomobject]@{ Area = "Employees"; Test = "Duplicate code -> flash + ?add=1"; Pass = $hasFlash -and $hasAdd; Detail = "loc=$loc; flash=$hasFlash" }

Write-Host "=== EOD empty submit ===" -ForegroundColor Cyan
$eodPage = Invoke-WebRequest -Uri "$Base/me/eod" -WebSession $emp -UseBasicParsing
$csrf = Get-Csrf $eodPage.Content
$body = "action=submit&summary=&csrf_token=$csrf"
try {
    $r = Invoke-WebRequest -Uri "$Base/me/eod" -Method POST -WebSession $emp -UseBasicParsing `
        -ContentType "application/x-www-form-urlencoded" -Body $body -MaximumRedirection 0 -ErrorAction SilentlyContinue
} catch { }
$eodAfter = Invoke-WebRequest -Uri "$Base/me/eod" -WebSession $emp -UseBasicParsing
$eodFlash = $eodAfter.Content -match "alert-error"
$results += [pscustomobject]@{ Area = "EOD"; Test = "Empty submit shows flash"; Pass = $eodFlash; Detail = "flash=$eodFlash" }

Write-Host ""
$results | Format-Table -AutoSize
$failed = @($results | Where-Object { -not $_.Pass })
if ($failed.Count -gt 0) {
    Write-Host "FAILED: $($failed.Count)" -ForegroundColor Red
    exit 1
}
Write-Host "All $($results.Count) checks passed." -ForegroundColor Green