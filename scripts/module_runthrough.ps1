# Per-module live smoke test — http://localhost:8080
$Base = "http://localhost:8080"
$results = @()

function Get-Csrf($html) {
    if ($html -match 'name="csrf_token" value="([^"]+)"') { return $Matches[1] }
    return $null
}

function New-Session { New-Object Microsoft.PowerShell.Commands.WebRequestSession }

function Try-Login($code, $pin) {
    $s = New-Session
    $l = Invoke-WebRequest "$Base/login" -WebSession $s -UseBasicParsing
    $csrf = Get-Csrf $l.Content
    if (-not $csrf) { return @{ Ok = $false; Session = $s; Detail = "no csrf" } }
    $b = "employee_code=$code&pin=$pin&csrf_token=$csrf"
    $r = Invoke-WebRequest "$Base/login" -Method POST -WebSession $s -Body $b `
        -ContentType "application/x-www-form-urlencoded" -UseBasicParsing
    $path = $r.BaseResponse.ResponseUri.AbsolutePath
    $ok = $path -ne "/login"
    return @{ Ok = $ok; Session = $s; Detail = $path }
}

function Check-Page($session, $path, $label) {
    try {
        $p = Invoke-WebRequest "$Base$path" -WebSession $session -UseBasicParsing
        return @{ Module = $label; Test = "GET $path"; Pass = ($p.StatusCode -eq 200); Detail = "200" }
    } catch {
        $code = [int]$_.Exception.Response.StatusCode
        return @{ Module = $label; Test = "GET $path"; Pass = $false; Detail = "HTTP $code" }
    }
}

# Core
try {
    $h = Invoke-WebRequest "$Base/health" -UseBasicParsing
    $results += [pscustomobject]@{ Module = "Core"; Test = "Health"; Pass = ($h.StatusCode -eq 200); Detail = $h.Content }
} catch {
    $results += [pscustomobject]@{ Module = "Core"; Test = "Health"; Pass = $false; Detail = "down" }
}

# Auth
$bad = Try-Login "INVALID" "0000"
$results += [pscustomobject]@{ Module = "Auth"; Test = "Bad login rejected"; Pass = (-not $bad.Ok); Detail = $bad.Detail }
$emp = Try-Login "SAMPLE01" "7391"
$results += [pscustomobject]@{ Module = "Auth"; Test = "Employee login"; Pass = $emp.Ok; Detail = $emp.Detail }
$adm = Try-Login "ADMIN" "1234"
$results += [pscustomobject]@{ Module = "Auth"; Test = "Admin login"; Pass = $adm.Ok; Detail = $adm.Detail }

# Employee module
if ($emp.Ok) {
    foreach ($p in @("/", "/me/eod", "/me/leave", "/me/timesheet", "/me/profile", "/me/requirements", "/me/holidays")) {
        $c = Check-Page $emp.Session $p "Employee"
        $results += [pscustomobject]$c
    }
    $pg = Invoke-WebRequest "$Base/" -WebSession $emp.Session -UseBasicParsing
    $csrf = Get-Csrf $pg.Content
    if ($csrf) {
        try {
            Invoke-WebRequest "$Base/clock/in" -Method POST -WebSession $emp.Session `
                -Body "csrf_token=$csrf" -ContentType "application/x-www-form-urlencoded" `
                -MaximumRedirection 0 -ErrorAction SilentlyContinue | Out-Null
        } catch {}
        $after = Invoke-WebRequest "$Base/" -WebSession $emp.Session -UseBasicParsing
        $flash = $after.Content -match "alert-error|alert-success|alert-info"
        $results += [pscustomobject]@{ Module = "Employee"; Test = "Clock POST returns flash"; Pass = $flash; Detail = "flash=$flash" }
    }
}

# Manager / Admin dashboards
$mgrSession = if ($adm.Ok) { $adm.Session } elseif ($emp.Ok) { $emp.Session } else { $null }
if ($mgrSession) {
    foreach ($p in @("/manager", "/manager/leave", "/manager/pin-resets", "/manager/team", "/manager/eod")) {
        $c = Check-Page $mgrSession $p "Manager"
        $results += [pscustomobject]$c
    }
}
if ($adm.Ok) {
    foreach ($p in @("/admin/employees", "/admin/payroll", "/admin/reports", "/admin/settings", "/admin/holidays", "/admin/deduction-types", "/admin/requirements", "/admin/eod", "/admin/audit", "/admin/corrections")) {
        $c = Check-Page $adm.Session $p "Admin"
        $results += [pscustomobject]$c
    }
}

Write-Host ""
$results | Format-Table -AutoSize
$failed = @($results | Where-Object { -not $_.Pass })
Write-Host "Total: $($results.Count)  Passed: $($results.Count - $failed.Count)  Failed: $($failed.Count)"
if ($failed.Count -gt 0) {
    Write-Host "`nFailures:" -ForegroundColor Red
    $failed | Format-Table -AutoSize
    exit 1
}
exit 0