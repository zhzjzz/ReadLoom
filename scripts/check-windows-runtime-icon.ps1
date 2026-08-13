param(
    [string]$Executable = "target\release\readloom.exe"
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$resolvedExecutable = (Resolve-Path (Join-Path $repository $Executable)).Path
$validationRoot = Join-Path $repository 'target\validation\runtime-icon'
$stateDatabase = Join-Path $validationRoot 'readloom-state.sqlite3'
$capturePath = Join-Path $validationRoot 'window-icon.png'
$previousStateDatabase = $env:READLOOM_STATE_DB
$application = $null

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ReadloomWindowIconProbe {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", EntryPoint = "GetClassLongPtrW")]
    public static extern IntPtr GetClassLongPtr(IntPtr hWnd, int index);
}
'@

New-Item -ItemType Directory -Force -Path $validationRoot | Out-Null

try {
    $env:READLOOM_STATE_DB = $stateDatabase
    $application = Start-Process -FilePath $resolvedExecutable -WorkingDirectory $repository -WindowStyle Hidden -PassThru
    for ($attempt = 0; $attempt -lt 50; $attempt += 1) {
        Start-Sleep -Milliseconds 100
        $application.Refresh()
        if ($application.HasExited) { throw 'Readloom exited before the icon probe completed.' }
        if ($application.MainWindowHandle -ne 0) { break }
    }
    if ($application.MainWindowHandle -eq 0) { throw 'Readloom did not create a main window.' }

    $wmGetIcon = 0x007F
    $iconSmall2 = [IntPtr]2
    $gclpHiconSmall = -34
    $iconHandle = [ReadloomWindowIconProbe]::SendMessage($application.MainWindowHandle, $wmGetIcon, $iconSmall2, [IntPtr]::Zero)
    if ($iconHandle -eq [IntPtr]::Zero) {
        $iconHandle = [ReadloomWindowIconProbe]::GetClassLongPtr($application.MainWindowHandle, $gclpHiconSmall)
    }
    if ($iconHandle -eq [IntPtr]::Zero) { throw 'The runtime window has no small icon.' }

    $icon = [System.Drawing.Icon]::FromHandle($iconHandle)
    $bitmap = $icon.ToBitmap()
    $bitmap.Save($capturePath, [System.Drawing.Imaging.ImageFormat]::Png)
    $darkPixels = 0
    $whitePixels = 0
    for ($y = 0; $y -lt $bitmap.Height; $y += 1) {
        for ($x = 0; $x -lt $bitmap.Width; $x += 1) {
            $pixel = $bitmap.GetPixel($x, $y)
            if ($pixel.A -gt 180 -and $pixel.R -lt 70 -and $pixel.G -lt 70 -and $pixel.B -lt 70) { $darkPixels += 1 }
            if ($pixel.A -gt 180 -and $pixel.R -gt 220 -and $pixel.G -gt 220 -and $pixel.B -gt 220) { $whitePixels += 1 }
        }
    }
    $bitmap.Dispose()
    if ($darkPixels -lt 15 -or $whitePixels -lt 2) {
        throw "Runtime icon is not the Readloom dark tile: dark=$darkPixels white=$whitePixels capture=$capturePath"
    }
    [ordered]@{ capture = $capturePath; darkPixels = $darkPixels; whitePixels = $whitePixels } | ConvertTo-Json
}
finally {
    if ($null -ne $application -and -not $application.HasExited) {
        Stop-Process -Id $application.Id -Force
        $application.WaitForExit(5000) | Out-Null
    }
    $env:READLOOM_STATE_DB = $previousStateDatabase
}
