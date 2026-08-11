param(
    [string]$ExecutablePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri\target\release\readloom.exe'),
    [string]$EpubPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\readloom-stage3.epub'),
    [string]$TextFallbackPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\readloom-fallback.markdown'),
    [ValidateRange(0, 50000)]
    [int]$TextStressParagraphs = 0,
    [switch]$EpubStabilityOnly,
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
            uint processId;
            GetWindowThreadProcessId(window, out processId);
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
$layoutScreenshotPath = Join-Path $artifactDirectory 'stage3-epub-layout-ui.png'
$fallbackScreenshotPath = Join-Path $artifactDirectory 'stage3-unified-open-ui.png'
$libraryScreenshotPath = Join-Path $artifactDirectory 'stage3-library-ui.png'
$settingsScreenshotPath = Join-Path $artifactDirectory 'stage3-settings-typography-ui.png'
$epubStabilityInitialScreenshotPath = Join-Path $artifactDirectory 'epub-stability-initial.png'
$epubStabilityFinalScreenshotPath = Join-Path $artifactDirectory 'epub-stability-final.png'
$fallbackMarker = '未知扩展名已按文本成功打开'
if ($TextStressParagraphs -gt 0) {
    $fixture = [System.Text.StringBuilder]::new()
    $null = $fixture.AppendLine($fallbackMarker).AppendLine()
    for ($index = 1; $index -le $TextStressParagraphs; $index += 1) {
        $null = $fixture.Append('第 ').Append($index).Append(' 段用于检查 TXT 首次渲染、字体应用和事件循环响应。').AppendLine().AppendLine()
    }
    [System.IO.File]::WriteAllText($TextFallbackPath, $fixture.ToString(), [System.Text.UTF8Encoding]::new($false))
}
else {
    [System.IO.File]::WriteAllText($TextFallbackPath, $fallbackMarker, [System.Text.UTF8Encoding]::new($false))
}

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

    $frameExpression = @'
(() => {
  const paragraphs = [...document.querySelectorAll('p')]
    .filter((element) => (element.innerText ?? '').trim().length > 10);
  const firstParagraph = paragraphs[0] ?? null;
  const firstParagraphRect = firstParagraph?.getBoundingClientRect() ?? null;
  const visibleParagraphCount = paragraphs.filter((element) => {
    const rect = element.getBoundingClientRect();
    return rect.bottom > 0 && rect.top < innerHeight;
  }).length;
  const ancestors = [];
  let ancestor = firstParagraph;
  while (ancestor instanceof HTMLElement && ancestors.length < 6) {
    const rect = ancestor.getBoundingClientRect();
    const style = getComputedStyle(ancestor);
    ancestors.push({
      tag: ancestor.tagName,
      id: ancestor.id,
      className: ancestor.className,
      top: Math.round(rect.top * 10) / 10,
      height: Math.round(rect.height * 10) / 10,
      display: style.display,
      position: style.position,
      marginTop: style.marginTop,
      paddingTop: style.paddingTop,
      backgroundImage: style.backgroundImage
    });
    ancestor = ancestor.parentElement;
  }
  const bodyChildren = [...(document.body?.children ?? [])].slice(0, 8).map((element) => {
    const rect = element.getBoundingClientRect();
    return {
      tag: element.tagName,
      className: element.className,
      text: (element.innerText ?? '').trim().slice(0, 30),
      top: Math.round(rect.top * 10) / 10,
      height: Math.round(rect.height * 10) / 10
    };
  });
  const images = [...document.images].slice(0, 5).map((element) => {
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return {
      parentTag: element.parentElement?.tagName ?? null,
      parentClassName: element.parentElement?.className ?? null,
      source: element.getAttribute('src'),
      naturalWidth: element.naturalWidth,
      naturalHeight: element.naturalHeight,
      width: Math.round(rect.width * 10) / 10,
      height: Math.round(rect.height * 10) / 10,
      maxHeight: style.maxHeight,
      selectorMatched: element.matches('body>:has(>img:only-child):has(+:is(h1,h2,h3,h4,h5,h6))>img:only-child')
    };
  });
  return {
    text: document.body?.innerText ?? '',
    htmlLength: document.documentElement?.outerHTML.length ?? 0,
    imageCount: document.images.length,
    internalImageCount: [...document.images].filter((image) => image.currentSrc.startsWith('http://readloom-epub.localhost/')).length,
    externalImageSourceCount: [...document.images].filter((image) => /^https?:/i.test(image.getAttribute('src') ?? '') && !(image.getAttribute('src') ?? '').startsWith('http://readloom-epub.localhost/')).length,
    title: document.title,
    mimeType: document.contentType,
    publisherScriptExecuted: window.publisherScriptExecuted === true,
    externalLinkCount: document.querySelectorAll('a[href^="readloom-external:"]').length,
    externalNetworkResourceCount: performance.getEntriesByType('resource').filter((entry) => /^https?:/i.test(entry.name) && !entry.name.startsWith('http://readloom-epub.localhost/')).length,
    viewportHeight: innerHeight,
    paragraphCount: paragraphs.length,
    visibleParagraphCount,
    firstParagraphTop: firstParagraphRect?.top ?? null,
    firstParagraphText: (firstParagraph?.innerText ?? '').trim().slice(0, 80),
    firstParagraphAncestors: ancestors,
    bodyChildren,
    images
  };
})()
'@
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $tree = Invoke-CdpCommand -Socket $Socket -Method 'Page.getFrameTree'
        $child = @($tree.frameTree.childFrames)[0]
        if ($null -ne $child) {
            try {
                $world = Invoke-CdpCommand -Socket $Socket -Method 'Page.createIsolatedWorld' -Parameters @{
                    frameId = $child.frame.id
                    worldName = 'readloom-stage3-validation'
                    grantUniveralAccess = $false
                }
                $evaluated = Invoke-CdpCommand -Socket $Socket -Method 'Runtime.evaluate' -Parameters @{
                    expression = $frameExpression
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
                    viewportHeight = $evaluated.result.value.viewportHeight
                    paragraphCount = $evaluated.result.value.paragraphCount
                    visibleParagraphCount = $evaluated.result.value.visibleParagraphCount
                    firstParagraphTop = $evaluated.result.value.firstParagraphTop
                    firstParagraphText = $evaluated.result.value.firstParagraphText
                    firstParagraphAncestors = $evaluated.result.value.firstParagraphAncestors
                    bodyChildren = $evaluated.result.value.bodyChildren
                    images = $evaluated.result.value.images
                }
            }
            catch {
                $transientFrameNavigation = $_.Exception.Message -like '*Cannot find context with specified id*' -or
                    $_.Exception.Message -like '*No frame with given id found*' -or
                    $_.Exception.Message -like '*No frame for given id found*' -or
                    $_.Exception.Message -like '*frame was detached*'
                if (-not $transientFrameNavigation) { throw }
                Start-Sleep -Milliseconds 25
                continue
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
                    expression = $frameExpression
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
                    viewportHeight = $evaluated.result.value.viewportHeight
                    paragraphCount = $evaluated.result.value.paragraphCount
                    visibleParagraphCount = $evaluated.result.value.visibleParagraphCount
                    firstParagraphTop = $evaluated.result.value.firstParagraphTop
                    firstParagraphText = $evaluated.result.value.firstParagraphText
                    firstParagraphAncestors = $evaluated.result.value.firstParagraphAncestors
                    bodyChildren = $evaluated.result.value.bodyChildren
                    images = $evaluated.result.value.images
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

function Get-MainEpubState {
    param([System.Net.WebSockets.ClientWebSocket]$Socket)

    return Invoke-JavaScript -Socket $Socket -Expression @'
(() => {
  const iframe = document.querySelector('iframe[sandbox="allow-scripts"]');
  const rect = iframe?.getBoundingClientRect();
  const style = iframe instanceof HTMLElement ? getComputedStyle(iframe) : null;
  return {
    bodyText: document.body.innerText,
    readerText: document.querySelector('[aria-label="EPUB 阅读器"]')?.innerText ?? '',
    iframeCount: document.querySelectorAll('iframe[sandbox="allow-scripts"]').length,
    iframeSource: iframe?.getAttribute('src') ?? null,
    iframeWidth: rect?.width ?? 0,
    iframeHeight: rect?.height ?? 0,
    iframeDisplay: style?.display ?? null,
    iframeVisibility: style?.visibility ?? null,
    iframeOpacity: style?.opacity ?? null
  };
})()
'@
}

function Save-CdpScreenshot {
    param(
        [System.Net.WebSockets.ClientWebSocket]$Socket,
        [string]$Path
    )

    $capture = Invoke-CdpCommand -Socket $Socket -Method 'Page.captureScreenshot' -Parameters @{
        format = 'png'
        captureBeyondViewport = $false
    }
    [System.IO.File]::WriteAllBytes($Path, [Convert]::FromBase64String($capture.data))
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
        throw "JavaScript evaluation failed: $($result.exceptionDetails | ConvertTo-Json -Depth 8 -Compress)"
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
(async () => {
  let button = [...document.querySelectorAll('button')]
    .find((candidate) => ['打开文件', '选择文件'].includes(candidate.textContent?.trim() ?? ''));
  if (!button) {
    const workspace = [...document.querySelectorAll('button')]
      .find((candidate) => candidate.textContent?.trim() === '阅读与编辑');
    if (!(workspace instanceof HTMLButtonElement)) return false;
    workspace.click();
    await new Promise((resolve) => setTimeout(resolve, 50));
    button = [...document.querySelectorAll('button')]
      .find((candidate) => ['打开文件', '选择文件'].includes(candidate.textContent?.trim() ?? ''));
  }
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
    $stabilitySamples = [System.Collections.Generic.List[object]]::new()
    $initialStabilityScreenshotSaved = $false
    $loadDeadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        Start-Sleep -Milliseconds $(if ($EpubStabilityOnly) { 75 } else { 200 })
        $state = Get-MainEpubState -Socket $socket
        if ($EpubStabilityOnly -and
            $state.iframeCount -eq 1 -and
            $state.iframeSource -like 'http://readloom-epub.localhost/*') {
            $frame = Get-EpubFrameState -Socket $socket
            $sample = [pscustomobject]@{
                elapsedMs = [Math]::Round($openStopwatch.Elapsed.TotalMilliseconds, 1)
                frameUrl = $frame.url
                frameTextLength = $frame.text.Length
                frameHtmlLength = $frame.htmlLength
                viewportHeight = $frame.viewportHeight
                paragraphCount = $frame.paragraphCount
                visibleParagraphCount = $frame.visibleParagraphCount
                firstParagraphTop = if ($null -eq $frame.firstParagraphTop) { $null } else { [Math]::Round($frame.firstParagraphTop, 1) }
                readerTextLength = $state.readerText.Length
                iframeWidth = [Math]::Round($state.iframeWidth, 1)
                iframeHeight = [Math]::Round($state.iframeHeight, 1)
                iframeDisplay = $state.iframeDisplay
                iframeVisibility = $state.iframeVisibility
                iframeOpacity = $state.iframeOpacity
            }
            $stabilitySamples.Add($sample)
            if (-not $initialStabilityScreenshotSaved -and $frame.text.Trim().Length -gt 20) {
                Save-CdpScreenshot -Socket $socket -Path $epubStabilityInitialScreenshotPath
                $initialStabilityScreenshotSaved = $true
            }
        }
        $loaded = if ($EpubStabilityOnly) {
            $state.iframeCount -eq 1 -and $state.iframeSource -like 'http://readloom-epub.localhost/*'
        }
        else {
            $state.bodyText -like '*阅织阶段三验收书*' -and
                $state.iframeCount -eq 1 -and
                $state.iframeSource -like 'http://readloom-epub.localhost/*'
        }
        if ([DateTime]::UtcNow -gt $loadDeadline) {
            throw "Timed out waiting for EPUB UI. Current state: $($state | ConvertTo-Json -Compress)"
        }
    } while (-not $loaded)

    if ($EpubStabilityOnly) {
        $sampleDeadline = [DateTime]::UtcNow.AddSeconds(6)
        do {
            Start-Sleep -Milliseconds 100
            $state = Get-MainEpubState -Socket $socket
            if ($state.iframeCount -eq 1 -and
                $state.iframeSource -like 'http://readloom-epub.localhost/*') {
                $frame = Get-EpubFrameState -Socket $socket
                $sample = [pscustomobject]@{
                    elapsedMs = [Math]::Round($openStopwatch.Elapsed.TotalMilliseconds, 1)
                    frameUrl = $frame.url
                    frameTextLength = $frame.text.Length
                    frameHtmlLength = $frame.htmlLength
                    viewportHeight = $frame.viewportHeight
                    paragraphCount = $frame.paragraphCount
                    visibleParagraphCount = $frame.visibleParagraphCount
                    firstParagraphTop = if ($null -eq $frame.firstParagraphTop) { $null } else { [Math]::Round($frame.firstParagraphTop, 1) }
                    readerTextLength = $state.readerText.Length
                    iframeWidth = [Math]::Round($state.iframeWidth, 1)
                    iframeHeight = [Math]::Round($state.iframeHeight, 1)
                    iframeDisplay = $state.iframeDisplay
                    iframeVisibility = $state.iframeVisibility
                    iframeOpacity = $state.iframeOpacity
                }
                $stabilitySamples.Add($sample)
                if (-not $initialStabilityScreenshotSaved -and $frame.text.Trim().Length -gt 20) {
                    Save-CdpScreenshot -Socket $socket -Path $epubStabilityInitialScreenshotPath
                    $initialStabilityScreenshotSaved = $true
                }
            }
        } while ([DateTime]::UtcNow -lt $sampleDeadline)
        $openStopwatch.Stop()
        Save-CdpScreenshot -Socket $socket -Path $epubStabilityFinalScreenshotPath

        $meaningfulSamples = @($stabilitySamples | Where-Object { $_.frameTextLength -gt 20 })
        $lastSample = @($stabilitySamples)[-1]
        $peakTextLength = ($stabilitySamples | Measure-Object -Property frameTextLength -Maximum).Maximum
        $minimumVisibleTextLength = if ($peakTextLength -gt 0) { [Math]::Max(20, [Math]::Floor($peakTextLength * 0.1)) } else { 20 }
        $textDisappeared = $peakTextLength -gt 20 -and $lastSample.frameTextLength -lt $minimumVisibleTextLength
        $frameHidden = $lastSample.iframeWidth -le 1 -or
            $lastSample.iframeHeight -le 1 -or
            $lastSample.iframeDisplay -eq 'none' -or
            $lastSample.iframeVisibility -eq 'hidden' -or
            $lastSample.iframeOpacity -eq '0'
        $paragraphsPushedOut = $lastSample.paragraphCount -gt 0 -and
            $lastSample.visibleParagraphCount -eq 0 -and
            $lastSample.firstParagraphTop -ge $lastSample.viewportHeight

        $timeline = @($stabilitySamples | Select-Object -First 5) +
            @($stabilitySamples | Select-Object -Last 5)
        Write-Output "EPUB stability samples: $($timeline | ConvertTo-Json -Depth 4 -Compress)"
        Write-Output "EPUB stability summary: samples=$($stabilitySamples.Count); peakText=$peakTextLength; finalText=$($lastSample.frameTextLength); finalUrl=$($lastSample.frameUrl); openMs=$([Math]::Round($openStopwatch.Elapsed.TotalMilliseconds, 1))"
        if ($meaningfulSamples.Count -eq 0 -or $textDisappeared -or $frameHidden -or $paragraphsPushedOut) {
            throw "EPUB visible content became empty or hidden. peakText=$peakTextLength; finalText=$($lastSample.frameTextLength); frameHidden=$frameHidden; paragraphsPushedOut=$paragraphsPushedOut; bodyChildren=$($frame.bodyChildren | ConvertTo-Json -Depth 5 -Compress); images=$($frame.images | ConvertTo-Json -Depth 5 -Compress); ancestors=$($frame.firstParagraphAncestors | ConvertTo-Json -Depth 5 -Compress); final=$($lastSample | ConvertTo-Json -Compress)"
        }
        return
    }

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
  if (!(button instanceof HTMLButtonElement) || button.disabled) return null;
  const rect = button.getBoundingClientRect();
  return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
})()
'@
        if ($null -eq $chapterRewound) {
            throw "The previous-chapter control was unavailable after restoring chapter 2. State: $($chapterRewound | ConvertTo-Json -Compress)"
        }
        Invoke-CdpCommand -Socket $socket -Method 'Input.dispatchMouseEvent' -Parameters @{
            type = 'mousePressed'
            x = $chapterRewound.x
            y = $chapterRewound.y
            button = 'left'
            clickCount = 1
        } | Out-Null
        Invoke-CdpCommand -Socket $socket -Method 'Input.dispatchMouseEvent' -Parameters @{
            type = 'mouseReleased'
            x = $chapterRewound.x
            y = $chapterRewound.y
            button = 'left'
            clickCount = 1
        } | Out-Null
    }

    $firstChapterDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $firstFrame = Get-EpubFrameState -Socket $socket
        $firstChapterReady = $firstFrame.url -like '*EPUB/text/chapter-1.xhtml' -and
            $firstFrame.text -like '*这是 Readloom 阶段三的自有 EPUB 验收内容*'
        if (-not $firstChapterReady) {
            if ([DateTime]::UtcNow -gt $firstChapterDeadline) {
                $mainState = Get-MainEpubState -Socket $socket
                throw "Timed out waiting for the first EPUB chapter after restoring navigation. Click target: $($chapterRewound | ConvertTo-Json -Compress); Main state: $($mainState | ConvertTo-Json -Compress); Frame state: $($firstFrame | ConvertTo-Json -Compress)"
            }
            Start-Sleep -Milliseconds 50
        }
    } while (-not $firstChapterReady)
    if ($firstFrame.url -notlike '*EPUB/text/chapter-1.xhtml' -or
        $firstFrame.text -notlike '*这是 Readloom 阶段三的自有 EPUB 验收内容*' -or
        $firstFrame.internalImageCount -ne 1 -or
        $firstFrame.externalImageSourceCount -ne 0 -or
        $firstFrame.publisherScriptExecuted -or
        $firstFrame.externalLinkCount -ne 1 -or
        $firstFrame.externalNetworkResourceCount -ne 0) {
        throw "The first EPUB chapter or its image did not render. Frame state: $($firstFrame | ConvertTo-Json -Compress)"
    }

    $epubLayout = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const stage = document.querySelector('.editor-stage');
  const reader = document.querySelector('[aria-label="EPUB 阅读器"]');
  const body = document.querySelector('.reader-body');
  if (!(stage instanceof HTMLElement) || !(reader instanceof HTMLElement) || !(body instanceof HTMLElement)) return null;
  const stageRect = stage.getBoundingClientRect();
  const readerRect = reader.getBoundingClientRect();
  const bodyRect = body.getBoundingClientRect();
  return {
    stageHeight: stageRect.height,
    readerHeight: readerRect.height,
    readerBodyHeight: bodyRect.height,
    bottomGap: stageRect.bottom - readerRect.bottom,
    readerBodyBottomGap: readerRect.bottom - bodyRect.bottom
  };
})()
'@
    if ($null -eq $epubLayout -or $epubLayout.bottomGap -gt 2 -or
        $epubLayout.readerBodyBottomGap -gt 2 -or
        $epubLayout.readerHeight -lt $epubLayout.stageHeight - 2) {
        throw "The EPUB reader did not fill the remaining workspace height. Layout: $($epubLayout | ConvertTo-Json -Compress)"
    }

    $layoutScreenshot = Invoke-CdpCommand -Socket $socket -Method 'Page.captureScreenshot' -Parameters @{
        format = 'png'
        captureBeyondViewport = $false
    }
    [System.IO.File]::WriteAllBytes($layoutScreenshotPath, [Convert]::FromBase64String($layoutScreenshot.data))

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
(async () => {
  let button = [...document.querySelectorAll('button')]
    .find((candidate) => ['打开文件', '选择文件'].includes(candidate.textContent?.trim() ?? ''));
  if (!button) {
    const workspace = [...document.querySelectorAll('button')]
      .find((candidate) => candidate.textContent?.trim() === '阅读与编辑');
    if (!(workspace instanceof HTMLButtonElement)) return false;
    workspace.click();
    await new Promise((resolve) => setTimeout(resolve, 50));
    button = [...document.querySelectorAll('button')]
      .find((candidate) => ['打开文件', '选择文件'].includes(candidate.textContent?.trim() ?? ''));
  }
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

    $txtOpenStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
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
        $fallbackLoaded = if ($TextStressParagraphs -gt 0) {
            -not [string]::IsNullOrWhiteSpace($fallbackState.editorText)
        }
        else {
            $fallbackState.editorText -like "*$fallbackMarker*"
        }
    } while (-not $fallbackLoaded -or $fallbackState.epubFrameCount -ne 0)
    $txtOpenStopwatch.Stop()

    $txtResponsiveStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $txtResponsive = Invoke-JavaScript -Socket $socket -Expression @'
new Promise((resolve) => setTimeout(() => resolve({
  text: document.querySelector('.text-reading-surface')?.innerText ?? '',
  blocks: document.querySelectorAll('.text-reading-surface [data-source-start]').length
}), 0))
'@
    $txtResponsiveStopwatch.Stop()
    Start-Sleep -Milliseconds 500
    $txtStable = Invoke-JavaScript -Socket $socket -Expression @'
(() => ({
  text: document.querySelector('.text-reading-surface')?.innerText ?? '',
  blocks: document.querySelectorAll('.text-reading-surface [data-source-start]').length
}))()
'@
    $txtTailVisible = $true
    if ($TextStressParagraphs -gt 0) {
        $txtTailVisible = Invoke-JavaScript -Socket $socket -Expression @"
new Promise((resolve) => {
  const surface = document.querySelector('.text-reading-surface');
  if (!(surface instanceof HTMLElement)) return resolve(false);
  surface.scrollTop = surface.scrollHeight;
  surface.dispatchEvent(new Event('scroll'));
  setTimeout(() => resolve(surface.innerText.includes('第 $TextStressParagraphs 段')), 500);
})
"@
    }
    $txtResponsiveVisible = if ($TextStressParagraphs -gt 0) {
        -not [string]::IsNullOrWhiteSpace($txtResponsive.text)
    }
    else {
        $txtResponsive.text -like "*$fallbackMarker*"
    }
    $txtStableVisible = if ($TextStressParagraphs -gt 0) {
        -not [string]::IsNullOrWhiteSpace($txtStable.text)
    }
    else {
        $txtStable.text -like "*$fallbackMarker*"
    }
    if (-not $txtResponsiveVisible -or
        -not $txtStableVisible -or
        ($TextStressParagraphs -gt 0 -and ($txtStable.blocks -le 0 -or $txtStable.blocks -gt 600)) -or
        -not $txtTailVisible -or
        ($TextStressParagraphs -gt 0 -and $txtOpenStopwatch.Elapsed.TotalMilliseconds -gt 5000) -or
        $txtResponsiveStopwatch.Elapsed.TotalMilliseconds -gt 2500) {
        throw "TXT visible content became empty or opened too slowly. InitialChars: $($txtResponsive.text.Length); stableChars: $($txtStable.text.Length); blocks: $($txtStable.blocks); tailVisible: $txtTailVisible; openMs: $([Math]::Round($txtOpenStopwatch.Elapsed.TotalMilliseconds, 2)); responseMs: $([Math]::Round($txtResponsiveStopwatch.Elapsed.TotalMilliseconds, 2))"
    }

    $initialTextBookmarkCount = Invoke-JavaScript -Socket $socket -Expression "document.querySelectorAll('[aria-label=`"TXT 书签`"] article').length"
    $textSearchStarted = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const input = document.querySelector('input[aria-label="TXT 全文检索"]');
  if (!(input instanceof HTMLInputElement) || !input.form) return false;
  input.value = '未知扩展名';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  input.form.requestSubmit();
  return true;
})()
'@
    if (-not $textSearchStarted) {
        throw 'The TXT full-text search request could not be submitted.'
    }
    $textSearchDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 25
        $textSearchResultCount = Invoke-JavaScript -Socket $socket -Expression "document.querySelectorAll('[aria-label=`"TXT 搜索结果`"] > button').length"
        if ([DateTime]::UtcNow -gt $textSearchDeadline) {
            throw 'Timed out waiting for the TXT full-text search result.'
        }
    } while ($textSearchResultCount -lt 1)

    $textSearchResultOpened = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const result = document.querySelector('[aria-label="TXT 搜索结果"] > button');
  if (!(result instanceof HTMLButtonElement)) return false;
  result.click();
  return true;
})()
'@
    if (-not $textSearchResultOpened) {
        throw 'The TXT search result could not be opened.'
    }

    $textBookmarkAdded = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const button = document.querySelector('button[aria-label="添加 TXT 书签"]');
  if (!(button instanceof HTMLButtonElement)) return false;
  button.click();
  return true;
})()
'@
    if (-not $textBookmarkAdded) {
        throw 'The TXT bookmark action was unavailable.'
    }
    $textBookmarkDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 25
        $textBookmarkCount = Invoke-JavaScript -Socket $socket -Expression "document.querySelectorAll('[aria-label=`"TXT 书签`"] article').length"
        if ([DateTime]::UtcNow -gt $textBookmarkDeadline) {
            throw 'Timed out waiting for the TXT bookmark to appear.'
        }
    } while ($textBookmarkCount -le $initialTextBookmarkCount)

    $deepTextState = $null
    if ($TextStressParagraphs -gt 0) {
        $deepTextState = Invoke-JavaScript -Socket $socket -Expression @'
new Promise((resolve) => {
  const surface = document.querySelector('.text-reading-surface');
  if (!(surface instanceof HTMLElement)) return resolve(null);
  surface.scrollTop = Math.round((surface.scrollHeight - surface.clientHeight) * 0.68);
  surface.dispatchEvent(new Event('scroll'));
  setTimeout(() => {
    const bounds = surface.getBoundingClientRect();
    const blocks = [...surface.querySelectorAll('[data-source-start]')];
    const visible = blocks.filter((block) => {
      const rect = block.getBoundingClientRect();
      return rect.bottom > bounds.top && rect.top < bounds.bottom;
    });
    const firstRect = blocks[0]?.getBoundingClientRect();
    const lastRect = blocks.at(-1)?.getBoundingClientRect();
    const topSpacer = surface.querySelector('.virtual-spacer');
    resolve({
      scrollTop: surface.scrollTop,
      scrollHeight: surface.scrollHeight,
      renderedBlocks: blocks.length,
      visibleBlocks: visible.length,
      visibleText: visible.map((block) => block.textContent ?? '').join('\n'),
      firstRenderedSource: Number(blocks[0]?.getAttribute('data-source-start') ?? -1),
      firstVisibleSource: Number(visible[0]?.getAttribute('data-source-start') ?? -1),
      firstBlockTop: firstRect ? firstRect.top - bounds.top : null,
      lastBlockBottom: lastRect ? lastRect.bottom - bounds.top : null,
      topSpacerHeight: topSpacer instanceof HTMLElement ? topSpacer.getBoundingClientRect().height : 0
    });
  }, 1500);
})
'@
        if ($null -eq $deepTextState -or
            $deepTextState.visibleBlocks -le 0 -or
            [string]::IsNullOrWhiteSpace($deepTextState.visibleText) -or
            $deepTextState.firstVisibleSource -le 0) {
            throw "TXT became blank while jumping to a deep reading position. State: $($deepTextState | ConvertTo-Json -Compress)"
        }
        Start-Sleep -Milliseconds 1200
    }

    $textClosed = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const toolbar = document.querySelector('nav[aria-label="文本文件操作"]');
  const button = [...(toolbar?.querySelectorAll('button') ?? [])]
    .find((candidate) => candidate.textContent?.trim() === '关闭');
  if (!(button instanceof HTMLButtonElement) || button.disabled) return false;
  button.click();
  return true;
})()
'@
    if (-not $textClosed) {
        throw 'The TXT document could not be closed before the persistence check.'
    }
    $textCloseDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 25
        $textEditorCount = Invoke-JavaScript -Socket $socket -Expression "document.querySelectorAll('.cm-editor').length"
        if ([DateTime]::UtcNow -gt $textCloseDeadline) {
            throw 'Timed out waiting for the TXT document to close.'
        }
    } while ($textEditorCount -ne 0)

    $textReopenClicked = Invoke-JavaScript -Socket $socket -Expression @'
(async () => {
  let button = [...document.querySelectorAll('button')]
    .find((candidate) => ['打开文件', '选择文件'].includes(candidate.textContent?.trim() ?? ''));
  if (!button) {
    const workspace = [...document.querySelectorAll('button')]
      .find((candidate) => candidate.textContent?.trim() === '阅读与编辑');
    if (!(workspace instanceof HTMLButtonElement)) return false;
    workspace.click();
    await new Promise((resolve) => setTimeout(resolve, 50));
    button = [...document.querySelectorAll('button')]
      .find((candidate) => ['打开文件', '选择文件'].includes(candidate.textContent?.trim() ?? ''));
  }
  if (!(button instanceof HTMLButtonElement)) return false;
  button.click();
  return true;
})()
'@
    if (-not $textReopenClicked) {
        throw 'The unified Open File button disappeared before the TXT persistence check.'
    }

    $dialogDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $dialogHandle = [ReadloomUiCloseHarness]::FindFileDialog([uint32]$process.Id)
        if ([DateTime]::UtcNow -gt $dialogDeadline) {
            throw 'Timed out waiting for the native file dialog for the TXT persistence check.'
        }
    } while ($dialogHandle -eq [IntPtr]::Zero)

    if (-not [ReadloomUiCloseHarness]::SubmitFileDialog($dialogHandle, $TextFallbackPath)) {
        throw 'The native file dialog did not accept the TXT persistence fixture.'
    }

    $textReopenDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 50
        $restoredTextState = Invoke-JavaScript -Socket $socket -Expression @'
(() => ({
  editorText: document.querySelector('.cm-content')?.textContent ?? '',
  bookmarkCount: document.querySelectorAll('[aria-label="TXT 书签"] article').length
}))()
'@
        if ([DateTime]::UtcNow -gt $textReopenDeadline) {
            throw "Timed out waiting for the persisted TXT bookmark. Current state: $($restoredTextState | ConvertTo-Json -Compress)"
        }
        $restoredTextLoaded = if ($TextStressParagraphs -gt 0) {
            -not [string]::IsNullOrWhiteSpace($restoredTextState.editorText)
        }
        else {
            $restoredTextState.editorText -like "*$fallbackMarker*"
        }
    } while (-not $restoredTextLoaded -or
        $restoredTextState.bookmarkCount -le $initialTextBookmarkCount)

    $restoredTextWindow = $null
    $forwardTextWindows = @()
    if ($TextStressParagraphs -gt 0) {
        $restoredTextWindow = Invoke-JavaScript -Socket $socket -Expression @'
new Promise((resolve) => setTimeout(() => {
  const surface = document.querySelector('.text-reading-surface');
  if (!(surface instanceof HTMLElement)) return resolve(null);
  const bounds = surface.getBoundingClientRect();
  const blocks = [...surface.querySelectorAll('[data-source-start]')];
  const visible = blocks.filter((block) => {
    const rect = block.getBoundingClientRect();
    return rect.bottom > bounds.top && rect.top < bounds.bottom;
  });
  resolve({
    scrollTop: surface.scrollTop,
    scrollHeight: surface.scrollHeight,
    renderedBlocks: blocks.length,
    visibleBlocks: visible.length,
    visibleText: visible.map((block) => block.textContent ?? '').join('\n'),
    firstRenderedSource: Number(blocks[0]?.getAttribute('data-source-start') ?? -1),
    firstVisibleSource: Number(visible[0]?.getAttribute('data-source-start') ?? -1)
  });
}, 1000))
'@
        if ($null -eq $restoredTextWindow -or
            $restoredTextWindow.visibleBlocks -le 0 -or
            [string]::IsNullOrWhiteSpace($restoredTextWindow.visibleText) -or
            $restoredTextWindow.firstVisibleSource -le 0) {
            throw "TXT restored its deep reading position with a blank viewport. Before close: $($deepTextState | ConvertTo-Json -Compress); restored: $($restoredTextWindow | ConvertTo-Json -Compress)"
        }

        $forwardTextWindows = @(Invoke-JavaScript -Socket $socket -Expression @'
new Promise(async (resolve) => {
  const surface = document.querySelector('.text-reading-surface');
  if (!(surface instanceof HTMLElement)) return resolve([]);
  const states = [];
  for (let step = 1; step <= 16; step += 1) {
    surface.scrollTop = Math.min(
      surface.scrollHeight - surface.clientHeight,
      surface.scrollTop + surface.clientHeight * 0.85
    );
    surface.dispatchEvent(new Event('scroll'));
    await new Promise((next) => setTimeout(next, 120));
    const bounds = surface.getBoundingClientRect();
    const blocks = [...surface.querySelectorAll('[data-source-start]')];
    const visible = blocks.filter((block) => {
      const rect = block.getBoundingClientRect();
      return rect.bottom > bounds.top && rect.top < bounds.bottom;
    });
    const firstRect = blocks[0]?.getBoundingClientRect();
    const lastRect = blocks.at(-1)?.getBoundingClientRect();
    const topSpacer = surface.querySelector('.virtual-spacer');
    states.push({
      step,
      scrollTop: surface.scrollTop,
      renderedBlocks: blocks.length,
      visibleBlocks: visible.length,
      visibleTextLength: visible.reduce((total, block) => total + (block.textContent?.trim().length ?? 0), 0),
      firstVisibleSource: Number(visible[0]?.getAttribute('data-source-start') ?? -1),
      firstBlockTop: firstRect ? firstRect.top - bounds.top : null,
      lastBlockBottom: lastRect ? lastRect.bottom - bounds.top : null,
      topSpacerHeight: topSpacer instanceof HTMLElement ? topSpacer.getBoundingClientRect().height : 0
    });
  }
  resolve(states);
})
'@)
        $blankForwardWindow = $forwardTextWindows | Where-Object {
            $_.visibleBlocks -le 0 -or $_.visibleTextLength -le 0
        } | Select-Object -First 1
        if ($null -ne $blankForwardWindow) {
            throw "TXT became blank while reading forward from a restored position. Restored: $($restoredTextWindow | ConvertTo-Json -Compress); failingWindow: $($blankForwardWindow | ConvertTo-Json -Compress)"
        }
    }

    $textBookmarkCleanupStarted = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const articles = [...document.querySelectorAll('[aria-label="TXT 书签"] article')];
  const button = [...(articles.at(-1)?.querySelectorAll('button') ?? [])]
    .find((candidate) => candidate.textContent?.trim() === '删除');
  if (!(button instanceof HTMLButtonElement)) return false;
  window.confirm = () => true;
  button.click();
  return true;
})()
'@
    if (-not $textBookmarkCleanupStarted) {
        throw 'The temporary TXT bookmark could not be removed after verification.'
    }
    $textBookmarkCleanupDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 25
        $remainingTextBookmarkCount = Invoke-JavaScript -Socket $socket -Expression "document.querySelectorAll('[aria-label=`"TXT 书签`"] article').length"
        if ([DateTime]::UtcNow -gt $textBookmarkCleanupDeadline) {
            throw 'Timed out removing the temporary TXT bookmark.'
        }
    } while ($remainingTextBookmarkCount -gt $initialTextBookmarkCount)

    $fallbackScreenshot = Invoke-CdpCommand -Socket $socket -Method 'Page.captureScreenshot' -Parameters @{
        format = 'png'
        captureBeyondViewport = $false
    }
    [System.IO.File]::WriteAllBytes(
        $fallbackScreenshotPath,
        [Convert]::FromBase64String($fallbackScreenshot.data)
    )

    $libraryOpened = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const button = [...document.querySelectorAll('button')]
    .find((candidate) => candidate.textContent?.trim() === '书库');
  if (!(button instanceof HTMLButtonElement)) return false;
  button.click();
  return true;
})()
'@
    if (-not $libraryOpened) {
        throw 'The library navigation action was unavailable.'
    }
    $libraryDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 25
        $libraryState = Invoke-JavaScript -Socket $socket -Expression @'
(() => ({
  visible: document.querySelector('[aria-label="书库"]') !== null,
  bookCount: document.querySelectorAll('.library-grid .book-card').length,
  tabCount: document.querySelectorAll('.tabs-strip .tab').length,
  text: document.querySelector('[aria-label="书库"]')?.textContent ?? ''
}))()
'@
        if ([DateTime]::UtcNow -gt $libraryDeadline) {
            throw "Timed out waiting for the populated library. Current state: $($libraryState | ConvertTo-Json -Compress)"
        }
    } while (-not $libraryState.visible -or $libraryState.bookCount -lt 2 -or
        $libraryState.text -notlike '*阅织阶段三验收书*')

    $libraryFiltered = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const input = document.querySelector('input[aria-label="搜索书库"]');
  if (!(input instanceof HTMLInputElement)) return false;
  input.value = '阅织阶段三验收书';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  return true;
})()
'@
    if (-not $libraryFiltered) {
        throw 'The library search control was unavailable.'
    }
    $libraryFilterDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 25
        $filteredLibraryCount = Invoke-JavaScript -Socket $socket -Expression "document.querySelectorAll('.library-grid .book-card').length"
        if ([DateTime]::UtcNow -gt $libraryFilterDeadline) {
            throw 'Timed out waiting for the filtered library result.'
        }
    } while ($filteredLibraryCount -ne 1)

    $libraryScreenshot = Invoke-CdpCommand -Socket $socket -Method 'Page.captureScreenshot' -Parameters @{
        format = 'png'
        captureBeyondViewport = $false
    }
    [System.IO.File]::WriteAllBytes(
        $libraryScreenshotPath,
        [Convert]::FromBase64String($libraryScreenshot.data)
    )

    $libraryBookOpened = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const button = document.querySelector('button[aria-label="打开 阅织阶段三验收书"]');
  if (!(button instanceof HTMLButtonElement) || button.disabled) return false;
  button.click();
  return true;
})()
'@
    if (-not $libraryBookOpened) {
        throw 'The filtered library book could not be opened.'
    }
    $libraryOpenDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 25
        $libraryReturnState = Invoke-JavaScript -Socket $socket -Expression @'
(() => ({
  returned: document.querySelector('[aria-label="EPUB 阅读器"]') !== null &&
    document.querySelector('[aria-label="书库"]') === null,
  tabCount: document.querySelectorAll('.tabs-strip .tab').length
}))()
'@
        if ([DateTime]::UtcNow -gt $libraryOpenDeadline) {
            throw 'Timed out returning from the library to the already-open EPUB tab.'
        }
    } while (-not $libraryReturnState.returned -or $libraryReturnState.tabCount -ne $libraryState.tabCount)

    $settingsOpened = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const button = [...document.querySelectorAll('button')]
    .find((candidate) => candidate.textContent?.trim() === '设置');
  if (!(button instanceof HTMLButtonElement)) return false;
  button.click();
  return true;
})()
'@
    if (-not $settingsOpened) {
        throw 'The settings navigation action was unavailable.'
    }
    $settingsDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 25
        $settingsState = Invoke-JavaScript -Socket $socket -Expression @'
(() => {
  const view = document.querySelector('[aria-label="设置"]');
  const detail = document.querySelector('.settings-detail');
  return {
    visible: view !== null,
    text: view?.textContent ?? '',
    horizontalOverflow: detail instanceof HTMLElement ? detail.scrollWidth - detail.clientWidth : 999,
    readingPanelCount: document.querySelectorAll('[aria-label="阅读排版设置"]').length
  };
})()
'@
        if ([DateTime]::UtcNow -gt $settingsDeadline) {
            throw "Timed out waiting for the reading typography settings. Current state: $($settingsState | ConvertTo-Json -Compress)"
        }
    } while (-not $settingsState.visible -or
        $settingsState.text -notlike '*字体*字号*字重*字间距*' -or
        $settingsState.text -notlike '*TXT*EPUB*' -or
        $settingsState.readingPanelCount -ne 1)
    $settingsScreenshot = Invoke-CdpCommand -Socket $socket -Method 'Page.captureScreenshot' -Parameters @{
        format = 'png'
        captureBeyondViewport = $false
    }
    [System.IO.File]::WriteAllBytes(
        $settingsScreenshotPath,
        [Convert]::FromBase64String($settingsScreenshot.data)
    )
    if ($settingsState.horizontalOverflow -gt 2) {
        throw "The settings detail has unexpected horizontal overflow: $($settingsState.horizontalOverflow) px."
    }

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
        readerHeight = $epubLayout.readerHeight
        readerBodyHeight = $epubLayout.readerBodyHeight
        readerBottomGap = $epubLayout.bottomGap
        readerBodyBottomGap = $epubLayout.readerBodyBottomGap
        chapter = '2 / 2'
        chapterSwitchMs = [Math]::Round($chapterSwitchStopwatch.Elapsed.TotalMilliseconds, 2)
        secondFrameText = $secondFrame.text
        searchMs = [Math]::Round($searchStopwatch.Elapsed.TotalMilliseconds, 2)
        searchResults = $searchResultCount
        bookmarkRequestSent = $bookmarkAdded
        fallbackFile = $TextFallbackPath
        fallbackOpenedAsText = $fallbackLoaded
        fallbackVisibleTextStable = $txtStableVisible
        fallbackVisibleBlocks = $txtStable.blocks
        fallbackTailVisible = $txtTailVisible
        restoredTextVisible = $TextStressParagraphs -eq 0 -or $restoredTextWindow.visibleBlocks -gt 0
        forwardTextWindowsVisible = $TextStressParagraphs -eq 0 -or $forwardTextWindows.Count -eq 16
        fallbackOpenMs = [Math]::Round($txtOpenStopwatch.Elapsed.TotalMilliseconds, 2)
        fallbackEventLoopResponseMs = [Math]::Round($txtResponsiveStopwatch.Elapsed.TotalMilliseconds, 2)
        fallbackKeptEpubTab = $fallbackState.tabText -like '*阅织阶段三验收书*'
        textSearchResults = $textSearchResultCount
        textSearchResultOpened = $textSearchResultOpened
        textBookmarkPersistedAfterReopen = $restoredTextState.bookmarkCount -gt $initialTextBookmarkCount
        textBookmarkCleanupRestoredCount = $remainingTextBookmarkCount -eq $initialTextBookmarkCount
        libraryBookCount = $libraryState.bookCount
        libraryFilteredResults = $filteredLibraryCount
        libraryReturnedToOpenEpub = $libraryReturnState.returned
        libraryReusedExistingTab = $libraryReturnState.tabCount -eq $libraryState.tabCount
        settingsReadingTypographyVisible = $settingsState.readingPanelCount -eq 1
        settingsHorizontalOverflow = $settingsState.horizontalOverflow
        epubWorkingSetBytes = [int64]$epubWorkingSetBytes
        epubPrivateMemoryBytes = [int64]$epubPrivateMemoryBytes
        exited = $true
        exitCode = $process.ExitCode
        screenshot = $screenshotPath
        layoutScreenshot = $layoutScreenshotPath
        fallbackScreenshot = $fallbackScreenshotPath
        libraryScreenshot = $libraryScreenshotPath
        settingsScreenshot = $settingsScreenshotPath
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
