param(
    [ValidateRange(1, 20)]
    [int]$StartupRuns = 5,

    [ValidateRange(1, 30)]
    [int]$IdleSeconds = 5
)

$ErrorActionPreference = 'Stop'

$projectRoot = Split-Path -Parent $PSScriptRoot
$executablePath = Join-Path $projectRoot 'src-tauri\target\release\readloom.exe'
$distPath = Join-Path $projectRoot 'dist'
$metricDirectory = Join-Path $projectRoot 'target\stage0-metrics'

if (-not (Test-Path -LiteralPath $executablePath -PathType Leaf)) {
    throw "Release executable not found: $executablePath"
}

if (-not (Test-Path -LiteralPath $distPath -PathType Container)) {
    throw "Frontend dist directory not found: $distPath"
}

New-Item -ItemType Directory -Force -Path $metricDirectory | Out-Null

function Get-ProcessTreeIds {
    param([int]$RootProcessId)

    $processes = @(Get-CimInstance Win32_Process)
    $ids = [System.Collections.Generic.HashSet[int]]::new()
    $pending = [System.Collections.Generic.Queue[int]]::new()
    $ids.Add($RootProcessId) | Out-Null
    $pending.Enqueue($RootProcessId)

    while ($pending.Count -gt 0) {
        $parentId = $pending.Dequeue()
        foreach ($process in $processes) {
            if ($process.ParentProcessId -eq $parentId -and $ids.Add([int]$process.ProcessId)) {
                $pending.Enqueue([int]$process.ProcessId)
            }
        }
    }

    return @($ids)
}

function Get-GzipSize {
    param([string]$Path)

    $source = [System.IO.File]::ReadAllBytes($Path)
    $stream = [System.IO.MemoryStream]::new()
    $gzip = [System.IO.Compression.GZipStream]::new(
        $stream,
        [System.IO.Compression.CompressionMode]::Compress,
        $true
    )
    try {
        $gzip.Write($source, 0, $source.Length)
    }
    finally {
        $gzip.Dispose()
    }
    $size = $stream.Length
    $stream.Dispose()
    return $size
}

$assetFiles = @(Get-ChildItem -LiteralPath $distPath -Recurse -File)
$frontendRawBytes = ($assetFiles | Measure-Object -Property Length -Sum).Sum
$frontendGzipBytes = 0
foreach ($asset in $assetFiles) {
    $frontendGzipBytes += Get-GzipSize -Path $asset.FullName
}

$runs = @()
$previousMetricOutput = $env:READLOOM_BASELINE_OUTPUT

try {
    for ($run = 1; $run -le $StartupRuns; $run++) {
        $markerPath = Join-Path $metricDirectory ("startup-{0}-{1}.json" -f $run, [guid]::NewGuid())
        $env:READLOOM_BASELINE_OUTPUT = $markerPath
        $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
        $process = Start-Process -FilePath $executablePath -PassThru -WindowStyle Hidden

        try {
            $deadline = [DateTime]::UtcNow.AddSeconds(30)
            while (-not (Test-Path -LiteralPath $markerPath -PathType Leaf)) {
                if ($process.HasExited) {
                    throw "Readloom exited before reporting frontend readiness. Exit code: $($process.ExitCode)"
                }
                if ([DateTime]::UtcNow -gt $deadline) {
                    throw 'Timed out waiting for the frontend-ready marker.'
                }
                Start-Sleep -Milliseconds 10
                $process.Refresh()
            }

            $stopwatch.Stop()
            $marker = Get-Content -Raw -LiteralPath $markerPath | ConvertFrom-Json
            Start-Sleep -Seconds $IdleSeconds

            $treeIds = @(Get-ProcessTreeIds -RootProcessId $process.Id)
            $processTree = @()
            foreach ($processId in $treeIds) {
                $candidate = Get-Process -Id $processId -ErrorAction SilentlyContinue
                if ($null -ne $candidate) {
                    $processTree += $candidate
                }
            }

            $workingSetBytes = ($processTree | Measure-Object -Property WorkingSet64 -Sum).Sum
            $privateMemoryBytes = ($processTree | Measure-Object -Property PrivateMemorySize64 -Sum).Sum

            $runs += [pscustomobject]@{
                run = $run
                launchToFrontendReadyMs = [Math]::Round($stopwatch.Elapsed.TotalMilliseconds, 2)
                mainToFrontendReadyMs = [int64]$marker.mainToFrontendReadyMs
                idleWorkingSetBytes = [int64]$workingSetBytes
                idlePrivateMemoryBytes = [int64]$privateMemoryBytes
                processCount = $processTree.Count
            }
        }
        finally {
            if (-not $process.HasExited) {
                Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
                $process.WaitForExit(5000) | Out-Null
            }
        }
    }
}
finally {
    $env:READLOOM_BASELINE_OUTPUT = $previousMetricOutput
}

$result = [pscustomobject]@{
    recordedAt = [DateTime]::UtcNow.ToString('o')
    definition = [pscustomobject]@{
        coldStart = 'First launch after the release build; Windows file caches were not cleared.'
        idleMemory = "Sum of Readloom and descendant WebView2 processes after $IdleSeconds seconds."
        frontendGzip = 'Sum of each dist file compressed independently with the Windows PowerShell GZip default.'
    }
    environment = [pscustomobject]@{
        operatingSystem = [System.Environment]::OSVersion.VersionString
        logicalProcessors = [System.Environment]::ProcessorCount
        totalPhysicalMemoryBytes = [int64](Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    }
    executableBytes = (Get-Item -LiteralPath $executablePath).Length
    frontendRawBytes = [int64]$frontendRawBytes
    frontendGzipBytes = [int64]$frontendGzipBytes
    frontendFiles = @($assetFiles | ForEach-Object {
        [pscustomobject]@{
            path = $_.FullName.Substring($distPath.Length + 1).Replace('\', '/')
            bytes = $_.Length
        }
    })
    coldStartProxyMs = $runs[0].launchToFrontendReadyMs
    runs = $runs
}

$result | ConvertTo-Json -Depth 8
