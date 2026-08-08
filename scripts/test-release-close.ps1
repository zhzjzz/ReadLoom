param(
    [string]$ExecutablePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri\target\release\readloom.exe'),
    [ValidateRange(1, 30)]
    [int]$TimeoutSeconds = 5
)

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class ReadloomCloseHarness
{
    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
}
'@

if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
    throw "Release executable not found: $ExecutablePath"
}

$metricDirectory = Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation'
New-Item -ItemType Directory -Force -Path $metricDirectory | Out-Null
$readyMarker = Join-Path $metricDirectory ("close-ready-{0}.json" -f [guid]::NewGuid())
$previousMetricOutput = $env:READLOOM_BASELINE_OUTPUT
$env:READLOOM_BASELINE_OUTPUT = $readyMarker
$process = Start-Process -FilePath $ExecutablePath -PassThru -WindowStyle Hidden
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 50
        $process.Refresh()
        if ($process.HasExited) {
            throw "Readloom exited before creating its main window (exit code $($process.ExitCode))."
        }
        if ([DateTime]::UtcNow -gt $deadline) {
            throw 'Timed out waiting for the Readloom main window.'
        }
    } while ($process.MainWindowHandle -eq [IntPtr]::Zero)

    $readyDeadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath $readyMarker -PathType Leaf)) {
        if ($process.HasExited) {
            throw "Readloom exited before reporting frontend readiness (exit code $($process.ExitCode))."
        }
        if ([DateTime]::UtcNow -gt $readyDeadline) {
            throw 'Timed out waiting for the Readloom frontend-ready marker.'
        }
        Start-Sleep -Milliseconds 20
        $process.Refresh()
    }

    if (-not [ReadloomCloseHarness]::PostMessage(
        $process.MainWindowHandle,
        0x0010,
        [IntPtr]::Zero,
        [IntPtr]::Zero
    )) {
        throw 'WM_CLOSE could not be delivered to the Readloom main window.'
    }

    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        throw "Readloom did not exit within $TimeoutSeconds seconds after WM_CLOSE."
    }

    [pscustomobject]@{
        exited = $true
        exitCode = $process.ExitCode
        timeoutSeconds = $TimeoutSeconds
    }
}
finally {
    $env:READLOOM_BASELINE_OUTPUT = $previousMetricOutput
    if (-not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $process.WaitForExit(5000) | Out-Null
    }
    Remove-Item -LiteralPath $readyMarker -Force -ErrorAction SilentlyContinue
}
