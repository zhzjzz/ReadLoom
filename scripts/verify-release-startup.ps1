param(
    [Parameter(Mandatory = $false)]
    [string]$ExecutablePath = "src-tauri/target/release/readloom.exe",

    [ValidateRange(1, 60)]
    [int]$TimeoutSeconds = 30
)

$ErrorActionPreference = "Stop"
$resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath -ErrorAction Stop).Path
$validationDirectory = Join-Path (Split-Path -Parent $PSScriptRoot) "target\validation"
New-Item -ItemType Directory -Force -Path $validationDirectory | Out-Null
$readyMarker = Join-Path $validationDirectory ("release-ready-{0}.json" -f [guid]::NewGuid())
$previousOutput = $env:READLOOM_BASELINE_OUTPUT
$process = $null

try {
    $env:READLOOM_BASELINE_OUTPUT = $readyMarker
    $process = Start-Process -FilePath $resolvedExecutable -PassThru -WindowStyle Hidden
    $env:READLOOM_BASELINE_OUTPUT = $previousOutput

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while (-not (Test-Path -LiteralPath $readyMarker -PathType Leaf)) {
        $process.Refresh()
        if ($process.HasExited) {
            throw "Readloom exited before its bundled frontend became ready (exit code $($process.ExitCode))."
        }
        if ([DateTime]::UtcNow -gt $deadline) {
            throw "Readloom did not render its bundled frontend within $TimeoutSeconds seconds. The executable may still be loading the Vite development URL."
        }
        Start-Sleep -Milliseconds 50
    }

    $metrics = Get-Content -Raw -LiteralPath $readyMarker | ConvertFrom-Json
    if ($metrics.processId -ne $process.Id) {
        throw "Frontend-ready marker belongs to process $($metrics.processId), expected $($process.Id)."
    }

    [pscustomobject]@{
        executable = $resolvedExecutable
        processId = $metrics.processId
        frontendReadyMs = $metrics.mainToFrontendReadyMs
        bundledFrontendRendered = $true
    }
}
finally {
    $env:READLOOM_BASELINE_OUTPUT = $previousOutput
    if ($null -ne $process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $process.WaitForExit(5000) | Out-Null
    }
    Remove-Item -LiteralPath $readyMarker -Force -ErrorAction SilentlyContinue
}
