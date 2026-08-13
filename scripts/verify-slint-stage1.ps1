param(
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$executable = Join-Path $repository 'target\release\readloom.exe'
$validationRoot = Join-Path $repository 'target\validation'
$smokeDirectory = Join-Path $validationRoot ("slint-stage1-{0}" -f [guid]::NewGuid().ToString('N'))
$stateDatabase = Join-Path $smokeDirectory 'readloom-state.sqlite3'
$previousStateDatabase = $env:READLOOM_STATE_DB
$application = $null

New-Item -ItemType Directory -Force -Path $smokeDirectory | Out-Null

try {
    if (-not $SkipBuild) {
        & cargo test -p readloom-core
        if ($LASTEXITCODE -ne 0) { throw 'readloom-core tests failed.' }
        & cargo build -p readloom --release
        if ($LASTEXITCODE -ne 0) { throw 'readloom release build failed.' }
    }
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        throw "Slint executable not found: $executable"
    }
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'check-windows-gui-subsystem.ps1') -Executable $executable
    if ($LASTEXITCODE -ne 0) { throw 'Slint executable is not a Windows GUI application.' }

    $env:READLOOM_STATE_DB = $stateDatabase
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $application = Start-Process -FilePath $executable -WorkingDirectory $repository -PassThru
    $windowReady = $false
    for ($attempt = 0; $attempt -lt 50; $attempt += 1) {
        Start-Sleep -Milliseconds 100
        $application.Refresh()
        if ($application.HasExited) {
            throw "Slint application exited during startup with code $($application.ExitCode)."
        }
        if ($application.MainWindowHandle -ne 0) {
            $windowReady = $true
            break
        }
    }
    if (-not $windowReady) { throw 'Slint main window was not ready within 5 seconds.' }
    $stopwatch.Stop()
    Start-Sleep -Milliseconds 800

    $processTable = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name)
    $knownIds = @([int]$application.Id)
    do {
        $newIds = @(
            $processTable |
                Where-Object { $_.ParentProcessId -in $knownIds -and $_.ProcessId -notin $knownIds } |
                ForEach-Object { [int]$_.ProcessId }
        )
        if ($newIds.Count -gt 0) { $knownIds += $newIds }
    } while ($newIds.Count -gt 0)
    $descendants = @($processTable | Where-Object { $_.ProcessId -in $knownIds -and $_.ProcessId -ne $application.Id })
    $webViewProcesses = @($descendants | Where-Object { $_.Name -ieq 'msedgewebview2.exe' })
    $application.Refresh()
    $result = [ordered]@{
        executable = $executable
        startupMs = $stopwatch.ElapsedMilliseconds
        workingSetBytes = $application.WorkingSet64
        privateMemoryBytes = $application.PrivateMemorySize64
        childProcesses = @($descendants | ForEach-Object { $_.Name })
        webView2ProcessCount = $webViewProcesses.Count
        stateDatabase = $stateDatabase
    }
    $result | ConvertTo-Json -Depth 4
    if ($webViewProcesses.Count -ne 0) {
        throw "Slint application started $($webViewProcesses.Count) WebView2 process(es)."
    }
}
finally {
    if ($null -ne $application -and -not $application.HasExited) {
        Stop-Process -Id $application.Id -Force
        $application.WaitForExit(5000) | Out-Null
    }
    $env:READLOOM_STATE_DB = $previousStateDatabase
}
