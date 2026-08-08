param(
    [string]$ExecutablePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri\target\release\readloom.exe'),
    [string]$EpubPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\readloom-stage3.epub'),
    [string]$TextFallbackPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\readloom-fallback.markdown'),
    [ValidateRange(1024, 65535)]
    [int]$DebugPort = 9237
)

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Threading;
using System.Runtime.InteropServices;

public static class ReadloomUiCloseHarness
{
    private delegate bool EnumWindowsCallback(IntPtr window, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsCallback callback, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(IntPtr window, StringBuilder value, int maximumCount);

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    private static extern IntPtr GetDlgItem(IntPtr dialog, int controlId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wParam, string lParam);

    [DllImport("user32.dll")]
    private static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    public static IntPtr FindFileDialog(uint expectedProcessId)
    {
        IntPtr result = IntPtr.Zero;
        EnumWindows((window, _) => {
            GetWindowThreadProcessId(window, out uint processId);
            if (processId != expectedProcessId || !IsWindowVisible(window)) return true;
            var className = new StringBuilder(64);
            GetClassName(window, className, className.Capacity);
            if (className.ToString() != "#32770") return true;
            result = window;
            return false;
        }, IntPtr.Zero);
        return result;
    }

    public static bool SubmitFileDialog(IntPtr dialog, string path)
    {
        IntPtr fileName = GetDlgItem(dialog, 0x47c);
        if (fileName == IntPtr.Zero) fileName = GetDlgItem(dialog, 0x480);
        IntPtr openButton = GetDlgItem(dialog, 1);
        if (fileName == IntPtr.Zero || openButton == IntPtr.Zero) return false;
        SetForegroundWindow(dialog);
        SendMessage(fileName, 0x000c, IntPtr.Zero, path);
        Thread.Sleep(150);
        SendMessage(openButton, 0x00f5, IntPtr.Zero, IntPtr.Zero);
        return true;
    }
}
'@

if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
    throw "Release executable not found: $ExecutablePath"
}
if (-not (Test-Path -LiteralPath $EpubPath -PathType Leaf)) {
    throw "EPUB fixture not found: $EpubPath"
}

$projectRoot = Split-Path -Parent $PSScriptRoot
$artifactDirectory = Join-Path $projectRoot 'target\validation'
New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null
$screenshotPath = Join-Path $artifactDirectory 'stage3-epub-ui.png'
$fallbackScreenshotPath = Join-Path $artifactDirectory 'stage3-unified-open-ui.png'
$fallbackMarker = '未知扩展名已按文本成功打开'
[System.IO.File]::WriteAllText($TextFallbackPath, $fallbackMarker, [System.Text.UTF8Encoding]::new($false))

$previousBrowserArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort"
$process = Start-Process -FilePath $ExecutablePath -PassThru
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousBrowserArguments

$socket = $null
$nextCommandId = 0

function Invoke-CdpCommand {
    param(
        [System.Net.WebSockets.ClientWebSocket]$Socket,
        [string]$Method,
        [hashtable]$Parameters = @{}
    )

    $script:nextCommandId += 1
    $commandId = $script:nextCommandId
    $payload = @{ id = $commandId; method = $Method; params = $Parameters } | ConvertTo-Json -Depth 20 -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($payload)
    $null = $Socket.SendAsync(
        [ArraySegment[byte]]::new($bytes),
        [System.Net.WebSockets.WebSocketMessageType]::Text,
        $true,
        [Threading.CancellationToken]::None
    ).GetAwaiter().GetResult()

    while ($true) {
        $buffer = [byte[]]::new(65536)
        $message = [System.IO.MemoryStream]::new()
        do {
            $received = $Socket.ReceiveAsync(
                [ArraySegment[byte]]::new($buffer),
                [Threading.CancellationToken]::None
            ).GetAwaiter().GetResult()
            $message.Write($buffer, 0, $received.Count)
        } while (-not $received.EndOfMessage)

        $json = [System.Text.Encoding]::UTF8.GetString($message.ToArray()) | ConvertFrom-Json
        $message.Dispose()
        if ($json.id -eq $commandId) {
            if ($null -ne $json.error) {
                throw "CDP $Method failed: $($json.error.message)"
            }
            return $json.result
        }
    }
}

function Get-EpubFrameState {
    param([System.Net.WebSockets.ClientWebSocket]$Socket)

    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $tree = Invoke-CdpCommand -Socket $Socket -Method 'Page.getFrameTree'
        $child = @($tree.frameTree.childFrames)[0]
        if ($null -ne $child) {
            $world = Invoke-CdpCommand -Socket $Socket -Method 'Page.createIsolatedWorld' -Parameters @{
                frameId = $child.frame.id
                worldName = 'readloom-stage3-validation'
                grantUniveralAccess = $false
            }
            $evaluated = Invoke-CdpCommand -Socket $Socket -Method 'Runtime.evaluate' -Parameters @{
                expression = '({ text: document.body?.innerText ?? "", htmlLength: document.documentElement?.outerHTML.length ?? 0, imageCount: document.images.length, internalImageCount: [...document.images].filter((image) => image.currentSrc.startsWith("http://readloom-epub.localhost/")).length, externalImageSourceCount: [...document.images].filter((image) => /^https?:/i.test(image.getAttribute("src") ?? "") && !(image.getAttribute("src") ?? "").startsWith("http://readloom-epub.localhost/")).length, title: document.title, mimeType: document.contentType, publisherScriptExecuted: window.publisherScriptExecuted === true, externalLinkCount: document.querySelectorAll("a[href^=\"readloom-external:\"]").length, externalNetworkResourceCount: performance.getEntriesByType("resource").filter((entry) => /^https?:/i.test(entry.name) && !entry.name.startsWith("http://readloom-epub.localhost/")).length })'
                contextId = $world.executionContextId
                returnByValue = $true
            }
            return [pscustomobject]@{
                url = $child.frame.url
                mimeType = $evaluated.result.value.mimeType
                text = $evaluated.result.value.text
                htmlLength = $evaluated.result.value.htmlLength
                imageCount = $evaluated.result.value.imageCount
                internalImageCount = $evaluated.result.value.internalImageCount
                externalImageSourceCount = $evaluated.result.value.externalImageSourceCount
                title = $evaluated.result.value.title
                publisherScriptExecuted = $evaluated.result.value.publisherScriptExecuted
                externalLinkCount = $evaluated.result.value.externalLinkCount
                externalNetworkResourceCount = $evaluated.result.value.externalNetworkResourceCount
            }
        }

        $targetResponse = Invoke-RestMethod -Uri "http://127.0.0.1:$DebugPort/json/list" -TimeoutSec 2
        $iframeTarget = $null
        foreach ($candidate in $targetResponse) {
            if ($candidate.type -eq 'iframe' -and
                $candidate.url -like 'http://readloom-epub.localhost/*') {
                $iframeTarget = $candidate
                break
            }
        }
        if ($null -ne $iframeTarget) {
            $frameSocket = [System.Net.WebSockets.ClientWebSocket]::new()
            try {
                $null = $frameSocket.ConnectAsync(
                    [Uri]$iframeTarget.webSocketDebuggerUrl,
                    [Threading.CancellationToken]::None
                ).GetAwaiter().GetResult()
                Invoke-CdpCommand -Socket $frameSocket -Method 'Runtime.enable' | Out-Null
                $evaluated = Invoke-CdpCommand -Socket $frameSocket -Method 'Runtime.evaluate' -Parameters @{
                    expression = '({ text: document.body?.innerText ?? "", htmlLength: document.documentElement?.outerHTML.length ?? 0, imageCount: document.images.length, internalImageCount: [...document.images].filter((image) => image.currentSrc.startsWith("http://readloom-epub.localhost/")).length, externalImageSourceCount: [...document.images].filter((image) => /^https?:/i.test(image.getAttribute("src") ?? "") && !(image.getAttribute("src") ?? "").startsWith("http://readloom-epub.localhost/")).length, title: document.title, mimeType: document.contentType, publisherScriptExecuted: window.publisherScriptExecuted === true, externalLinkCount: document.querySelectorAll("a[href^=\"readloom-external:\"]").length, externalNetworkResourceCount: performance.getEntriesByType("resource").filter((entry) => /^https?:/i.test(entry.name) && !entry.name.startsWith("http://readloom-epub.localhost/")).length })'
                    returnByValue = $true
                }
                return [pscustomobject]@{
                    url = $iframeTarget.url
                    mimeType = $evaluated.result.value.mimeType
                    text = $evaluated.result.value.text
                    htmlLength = $evaluated.result.value.htmlLength
                    imageCount = $evaluated.result.value.imageCount
                    internalImageCount = $evaluated.result.value.internalImageCount
                    externalImageSourceCount = $evaluated.result.value.externalImageSourceCount
                    title = $evaluated.result.value.title
                    publisherScriptExecuted = $evaluated.result.value.publisherScriptExecuted
                    externalLinkCount = $evaluated.result.value.externalLinkCount
                    externalNetworkResourceCount = $evaluated.result.value.externalNetworkResourceCount
                }
            }
            finally {
                $frameSocket.Dispose()
            }
        }

        if ([DateTime]::UtcNow -gt $deadline) {
            throw 'The EPUB iframe did not create a debuggable child frame in the release WebView2.'
        }
        Start-Sleep -Milliseconds 100
    } while ($true)
}

function Invoke-JavaScript {
    param(
        [System.Net.WebSockets.ClientWebSocket]$Socket,
        [string]$Expression
    )

    $result = Invoke-CdpCommand -Socket $Socket -Method 'Runtime.evaluate' -Parameters @{
        expression = $Expression
        returnByValue = $true
        awaitPromise = $true
    }
    if ($null -ne $result.exceptionDetails) {
        throw "JavaScript evaluation failed: $($result.exceptionDetails.text)"
    }
    return $result.result.value
}

function Get-ProcessTreeIds {
    param([int]$RootProcessId)

    $processes = @(Get-CimInstance Win32_Process)
    $ids = [System.Collections.Generic.HashSet[int]]::new()
    $pending = [System.Collections.Generic.Queue[int]]::new()
    $ids.Add($RootProcessId) | Out-Null
    $pending.Enqueue($RootProcessId)
    while ($pending.Count -gt 0) {
        $parentId = $pending.Dequeue()
        foreach ($candidate in $processes) {
            if ($candidate.ParentProcessId -eq $parentId -and $ids.Add([int]$candidate.ProcessId)) {
                $pending.Enqueue([int]$candidate.ProcessId)
            }
        }
    }
    return @($ids)
}

try {
    $endpoint = "http://127.0.0.1:$DebugPort/json/list"
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        try {
            $targets = @(Invoke-RestMethod -Uri $endpoint -TimeoutSec 1)
        }
        catch {
            $targets = @()
        }
        $page = $targets | Where-Object {
            $_.type -eq 'page' -and ($_.url -like 'tauri://*' -or $_.url -like 'http://tauri.localhost*')
        } | Select-Object -First 1
        if ($null -eq $page) {
            if ($process.HasExited) {
                throw "Readloom exited during startup (exit code $($process.ExitCode))."
            }
            if ([DateTime]::UtcNow -gt $deadline) {
                throw 'Timed out waiting for the release WebView2 debug target.'
            }
            Start-Sleep -Milliseconds 100
            $process.Refresh()
        }
    } while ($null -eq $page)

    $socket = [System.Net.WebSockets.ClientWebSocket]::new()
    $null = $socket.ConnectAsync(
        [Uri]$page.webSocketDebuggerUrl,
        [Threading.CancellationToken]::None
    ).GetAwaiter().GetResult()

    Invoke-CdpCommand -Socket $socket -Method 'Page.enable' | Out-Null
    Invoke-CdpCommand -Socket $socket -Method 'Runtime.enable' | Out-Null
    $clicked = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const button = [...document.querySelectorAll('button')]
    .find((candidate) => candidate.textContent?.trim() === '打开文件');
  if (!button) return false;
  button.click();
  return true;
})()
'@
    if (-not $clicked) {
        throw 'The release UI did not expose the unified Open File button.'
    }

    $dialogDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $dialogHandle = [ReadloomUiCloseHarness]::FindFileDialog([uint32]$process.Id)
        if ([DateTime]::UtcNow -gt $dialogDeadline) {
            throw 'Timed out waiting for the native file dialog.'
        }
    } while ($dialogHandle -eq [IntPtr]::Zero)

    $openStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    if (-not [ReadloomUiCloseHarness]::SubmitFileDialog($dialogHandle, $EpubPath)) {
        throw 'The native file dialog did not expose its standard file-name/Open control IDs.'
    }

    $loaded = $false
    $loadDeadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 200
        $state = Invoke-JavaScript -Socket $socket -Expression @'
(() => ({
  text: document.body.innerText,
  iframeCount: document.querySelectorAll('iframe[sandbox="allow-scripts"]').length,
  iframeSource: document.querySelector('iframe')?.getAttribute('src') ?? null
}))()
'@
        $loaded = $state.text -like '*阅织阶段三验收书*' -and
            $state.iframeCount -eq 1 -and
            $state.iframeSource -like 'http://readloom-epub.localhost/*'
        if ([DateTime]::UtcNow -gt $loadDeadline) {
            throw "Timed out waiting for EPUB UI. Current text: $($state.text)"
        }
    } while (-not $loaded)

    $restoredFrame = Get-EpubFrameState -Socket $socket
    $openStopwatch.Stop()
    $restoredChapter = if ($restoredFrame.url -like '*EPUB/text/chapter-2.xhtml') { 2 } else { 1 }
    if ($restoredChapter -eq 2) {
        if ($restoredFrame.text -notlike '*第二章用于验证目录跳转*') {
            throw "The restored second EPUB chapter did not render. Frame state: $($restoredFrame | ConvertTo-Json -Compress)"
        }
        $chapterRewound = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const button = document.querySelector('button[aria-label="上一章"]');
  if (!(button instanceof HTMLButtonElement) || button.disabled) return false;
  button.click();
  return true;
})()
'@
        if (-not $chapterRewound) {
            throw 'The previous-chapter control was unavailable after restoring chapter 2.'
        }
        Start-Sleep -Seconds 1
    }

    $firstFrame = Get-EpubFrameState -Socket $socket
    if ($firstFrame.url -notlike '*EPUB/text/chapter-1.xhtml' -or
        $firstFrame.text -notlike '*这是 Readloom 阶段三的自有 EPUB 验收内容*' -or
        $firstFrame.internalImageCount -ne 1 -or
        $firstFrame.externalImageSourceCount -ne 0 -or
        $firstFrame.publisherScriptExecuted -or
        $firstFrame.externalLinkCount -ne 1 -or
        $firstFrame.externalNetworkResourceCount -ne 0) {
        throw "The first EPUB chapter or its image did not render. Frame state: $($firstFrame | ConvertTo-Json -Compress)"
    }

    $chapterSwitchStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $chapterAdvanced = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const button = document.querySelector('button[aria-label="下一章"]');
  if (!(button instanceof HTMLButtonElement) || button.disabled) return false;
  button.click();
  return true;
})()
'@
    if (-not $chapterAdvanced) {
        throw 'The next-chapter control was unavailable after opening the test EPUB.'
    }

    $switchDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 25
        $secondFrame = Get-EpubFrameState -Socket $socket
        if ([DateTime]::UtcNow -gt $switchDeadline) {
            throw 'Timed out waiting for the second EPUB chapter frame.'
        }
    } while ($secondFrame.url -notlike '*EPUB/text/chapter-2.xhtml' -or
        $secondFrame.text -notlike '*第二章用于验证目录跳转*')
    $chapterSwitchStopwatch.Stop()
    $afterNavigation = Invoke-JavaScript -Socket $socket -Expression 'document.body.innerText'
    if ($afterNavigation -notlike '*2 / 2*') {
        throw 'The EPUB reader did not advance to the second spine item.'
    }
    $searchPanelOpened = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const button = [...document.querySelectorAll('.reader-actions button')]
    .find((candidate) => candidate.textContent?.trim() === '搜索');
  if (!button) return false;
  button.click();
  return true;
})()
'@
    if (-not $searchPanelOpened) {
        throw 'The EPUB search panel could not be opened.'
    }
    Start-Sleep -Milliseconds 50
    $searchStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $searchStarted = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const input = document.querySelector('input[aria-label="书内搜索"]');
  if (!(input instanceof HTMLInputElement) || !input.form) return false;
  input.value = '继续阅读';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  input.form.requestSubmit();
  return true;
})()
'@
    if (-not $searchStarted) {
        throw 'The EPUB search request could not be submitted.'
    }
    $searchDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 25
        $searchResultCount = Invoke-JavaScript -Socket $socket -Expression "document.querySelectorAll('.results-panel > button').length"
        if ([DateTime]::UtcNow -gt $searchDeadline) {
            throw 'Timed out waiting for the EPUB search result.'
        }
    } while ($searchResultCount -lt 1)
    $searchStopwatch.Stop()

    $bookmarkAdded = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const button = [...document.querySelectorAll('.reader-actions button')]
    .find((candidate) => candidate.textContent?.trim() === '添加书签');
  if (!button) return false;
  button.click();
  return true;
})()
'@
    if (-not $bookmarkAdded) {
        throw 'The EPUB bookmark action was unavailable.'
    }
    Start-Sleep -Milliseconds 200

    $treeIds = @(Get-ProcessTreeIds -RootProcessId $process.Id)
    $processTree = @($treeIds | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
    $epubWorkingSetBytes = ($processTree | Measure-Object -Property WorkingSet64 -Sum).Sum
    $epubPrivateMemoryBytes = ($processTree | Measure-Object -Property PrivateMemorySize64 -Sum).Sum

    $screenshot = Invoke-CdpCommand -Socket $socket -Method 'Page.captureScreenshot' -Parameters @{
        format = 'png'
        captureBeyondViewport = $false
    }
    [System.IO.File]::WriteAllBytes($screenshotPath, [Convert]::FromBase64String($screenshot.data))

    $fallbackClicked = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const button = [...document.querySelectorAll('button')]
    .find((candidate) => candidate.textContent?.trim() === '打开文件');
  if (!button) return false;
  button.click();
  return true;
})()
'@
    if (-not $fallbackClicked) {
        throw 'The unified Open File button disappeared after opening an EPUB.'
    }

    $dialogDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $dialogHandle = [ReadloomUiCloseHarness]::FindFileDialog([uint32]$process.Id)
        if ([DateTime]::UtcNow -gt $dialogDeadline) {
            throw 'Timed out waiting for the native file dialog for the fallback text file.'
        }
    } while ($dialogHandle -eq [IntPtr]::Zero)

    if (-not [ReadloomUiCloseHarness]::SubmitFileDialog($dialogHandle, $TextFallbackPath)) {
        throw 'The native file dialog did not accept the fallback text file.'
    }

    $fallbackDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $fallbackState = Invoke-JavaScript -Socket $socket -Expression @'
(() => ({
  editorText: document.querySelector('.cm-content')?.textContent ?? '',
  epubFrameCount: document.querySelectorAll('iframe[sandbox="allow-scripts"]').length,
  tabText: document.querySelector('.document-tabs')?.textContent ?? document.body.innerText
}))()
'@
        if ([DateTime]::UtcNow -gt $fallbackDeadline) {
            throw "Timed out waiting for unknown extension text fallback. Current state: $($fallbackState | ConvertTo-Json -Compress)"
        }
    } while ($fallbackState.editorText -notlike "*$fallbackMarker*" -or $fallbackState.epubFrameCount -ne 0)

    $fallbackScreenshot = Invoke-CdpCommand -Socket $socket -Method 'Page.captureScreenshot' -Parameters @{
        format = 'png'
        captureBeyondViewport = $false
    }
    [System.IO.File]::WriteAllBytes(
        $fallbackScreenshotPath,
        [Convert]::FromBase64String($fallbackScreenshot.data)
    )

    $process.Refresh()
    if (-not [ReadloomUiCloseHarness]::PostMessage(
        $process.MainWindowHandle,
        0x0010,
        [IntPtr]::Zero,
        [IntPtr]::Zero
    )) {
        throw 'WM_CLOSE could not be delivered after the EPUB was loaded.'
    }
    if (-not $process.WaitForExit(5000)) {
        throw 'Readloom did not exit within 5 seconds after closing an EPUB reading session.'
    }

    [pscustomobject]@{
        openedTitle = '阅织阶段三验收书'
        openToFirstFrameMs = [Math]::Round($openStopwatch.Elapsed.TotalMilliseconds, 2)
        restoredChapter = $restoredChapter
        iframeSandbox = 'allow-scripts'
        iframeSource = $state.iframeSource
        firstFrameText = $firstFrame.text
        internalImagesLoaded = $firstFrame.internalImageCount
        externalImageSources = $firstFrame.externalImageSourceCount
        publisherScriptBlocked = -not $firstFrame.publisherScriptExecuted
        externalLinksInert = $firstFrame.externalLinkCount
        externalNetworkResources = $firstFrame.externalNetworkResourceCount
        chapter = '2 / 2'
        chapterSwitchMs = [Math]::Round($chapterSwitchStopwatch.Elapsed.TotalMilliseconds, 2)
        secondFrameText = $secondFrame.text
        searchMs = [Math]::Round($searchStopwatch.Elapsed.TotalMilliseconds, 2)
        searchResults = $searchResultCount
        bookmarkRequestSent = $bookmarkAdded
        fallbackFile = $TextFallbackPath
        fallbackOpenedAsText = $fallbackState.editorText -like "*$fallbackMarker*"
        fallbackKeptEpubTab = $fallbackState.tabText -like '*阅织阶段三验收书*'
        epubWorkingSetBytes = [int64]$epubWorkingSetBytes
        epubPrivateMemoryBytes = [int64]$epubPrivateMemoryBytes
        exited = $true
        exitCode = $process.ExitCode
        screenshot = $screenshotPath
        fallbackScreenshot = $fallbackScreenshotPath
    }
}
finally {
    if ($null -ne $socket) {
        $socket.Dispose()
    }
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousBrowserArguments
    if (-not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $process.WaitForExit(5000) | Out-Null
    }
}
