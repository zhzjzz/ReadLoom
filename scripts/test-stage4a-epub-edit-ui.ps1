param(
    [string]$ExecutablePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri\target\release\readloom.exe'),
    [Parameter(Mandatory = $true)]
    [string]$InputPath,
    [Parameter(Mandatory = $true)]
    [string]$OutputPath,
    [Parameter(Mandatory = $true)]
    [string]$OriginalTitle,
    [Parameter(Mandatory = $true)]
    [string]$EditedTitle,
    [string]$EditedCreators = '阶段 4A 作者甲|阶段 4A 作者乙',
    [string]$EditedLanguage = 'zh-Hans-CN',
    [string]$EditedPublisher = 'Readloom 阶段 4A 出版社',
    [string]$EditedDescription = '阶段 4A 安全纯文本简介。',
    [Parameter(Mandatory = $true)]
    [string]$ChapterMarker,
    [string]$GeneratedChapterMarker = '',
    [ValidateSet('2.0', '3.0')]
    [string]$ExpectedVersion = '3.0',
    [ValidateRange(0, 1000)]
    [int]$ExpectedInternalImages = 0,
    [switch]$NoOriginalCover,
    [switch]$ExpectInternalFont,
    [string]$PngCoverPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\stage4a-replacement.png'),
    [string]$JpegCoverPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\stage4a-replacement.jpg'),
    [ValidateRange(1024, 65535)]
    [int]$DebugPort = 9238
)

$ErrorActionPreference = 'Stop'
if (-not $GeneratedChapterMarker) { $GeneratedChapterMarker = $ChapterMarker }

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Threading;
using System.Runtime.InteropServices;

public static class ReadloomStage4AUiHarness
{
    private delegate bool EnumWindowsCallback(IntPtr window, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsCallback callback, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern bool EnumChildWindows(IntPtr parent, EnumWindowsCallback callback, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern int GetDlgCtrlID(IntPtr window);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(IntPtr window, StringBuilder value, int maximumCount);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr window, StringBuilder value, int maximumCount);

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr window);

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
        if (fileName == IntPtr.Zero) fileName = GetDlgItem(dialog, 1001);
        IntPtr primaryButton = GetDlgItem(dialog, 1);
        if (fileName == IntPtr.Zero) fileName = FindDescendantById(dialog, 0x47c);
        if (fileName == IntPtr.Zero) fileName = FindDescendantById(dialog, 0x480);
        if (fileName == IntPtr.Zero) fileName = FindDescendantById(dialog, 1001);
        if (primaryButton == IntPtr.Zero) primaryButton = FindDescendantById(dialog, 1);
        if (fileName == IntPtr.Zero || primaryButton == IntPtr.Zero) return false;
        SetForegroundWindow(dialog);
        SendMessage(fileName, 0x000c, IntPtr.Zero, path);
        Thread.Sleep(150);
        SendMessage(primaryButton, 0x00f5, IntPtr.Zero, IntPtr.Zero);
        return true;
    }

    private static IntPtr FindDescendantById(IntPtr parent, int expectedId)
    {
        IntPtr result = IntPtr.Zero;
        EnumChildWindows(parent, (window, _) => {
            if (GetDlgCtrlID(window) != expectedId) return true;
            result = window;
            return false;
        }, IntPtr.Zero);
        return result;
    }

    public static string DescribeDialog(IntPtr dialog)
    {
        var description = new StringBuilder();
        EnumChildWindows(dialog, (window, _) => {
            var className = new StringBuilder(128);
            var text = new StringBuilder(256);
            GetClassName(window, className, className.Capacity);
            GetWindowText(window, text, text.Capacity);
            description.AppendFormat("id={0} class={1} text={2}; ", GetDlgCtrlID(window), className, text);
            return true;
        }, IntPtr.Zero);
        return description.ToString();
    }
}
'@

foreach ($requiredPath in @($ExecutablePath, $InputPath, $PngCoverPath, $JpegCoverPath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Required Stage 4A validation file not found: $requiredPath"
    }
}

$ExecutablePath = (Resolve-Path -LiteralPath $ExecutablePath).Path
$InputPath = (Resolve-Path -LiteralPath $InputPath).Path
$PngCoverPath = (Resolve-Path -LiteralPath $PngCoverPath).Path
$JpegCoverPath = (Resolve-Path -LiteralPath $JpegCoverPath).Path
$OutputPath = [System.IO.Path]::GetFullPath($OutputPath)

$outputDirectory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
if (Test-Path -LiteralPath $OutputPath) {
    Remove-Item -LiteralPath $OutputPath -Force
}
$sourceHashBefore = (Get-FileHash -Algorithm SHA256 -LiteralPath $InputPath).Hash
$screenshotPath = [System.IO.Path]::ChangeExtension($OutputPath, '.png')

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
    $payload = @{ id = $commandId; method = $Method; params = $Parameters } |
        ConvertTo-Json -Depth 20 -Compress
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

function Invoke-JavaScript {
    param([string]$Expression)

    $result = Invoke-CdpCommand -Socket $socket -Method 'Runtime.evaluate' -Parameters @{
        expression = $Expression
        returnByValue = $true
        awaitPromise = $true
    }
    if ($null -ne $result.exceptionDetails) {
        throw "JavaScript evaluation failed: $($result.exceptionDetails.text)"
    }
    return $result.result.value
}

function Wait-JavaScript {
    param(
        [string]$Expression,
        [string]$Description,
        [int]$TimeoutSeconds = 20
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        Start-Sleep -Milliseconds 100
        $value = Invoke-JavaScript -Expression $Expression
        if ($value) { return $value }
        if ([DateTime]::UtcNow -gt $deadline) {
            $body = Invoke-JavaScript -Expression 'document.body.innerText'
            throw "Timed out waiting for $Description. Current text: $body"
        }
    } while ($true)
}

function Submit-NativeSelection {
    param([string]$Path, [string]$Description)

    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $dialog = [ReadloomStage4AUiHarness]::FindFileDialog([uint32]$process.Id)
        if ([DateTime]::UtcNow -gt $deadline) {
            throw "Timed out waiting for the native $Description dialog."
        }
    } while ($dialog -eq [IntPtr]::Zero)
    if (-not [ReadloomStage4AUiHarness]::SubmitFileDialog($dialog, $Path)) {
        $controls = [ReadloomStage4AUiHarness]::DescribeDialog($dialog)
        throw "The native $Description dialog did not accept: $Path. Controls: $controls"
    }
}

function Click-Button {
    param([string]$Text)

    $textJson = $Text | ConvertTo-Json -Compress
    $clicked = Invoke-JavaScript -Expression @"
(() => {
  const text = $textJson;
  const button = [...document.querySelectorAll('button')]
    .find((candidate) => candidate.textContent?.trim() === text && !candidate.disabled);
  if (!button) return false;
  button.click();
  return true;
})()
"@
    if (-not $clicked) { throw "Enabled button not found: $Text" }
}

function Open-AppFile {
    param([string]$Path)

    Click-Button -Text '打开文件'
    Submit-NativeSelection -Path $Path -Description 'open file'
}

function Test-ChapterMarker {
    param([string]$Text, [string]$Markers)

    foreach ($marker in $Markers -split '\|') {
        if ($marker -and $Text.Contains($marker)) { return $true }
    }
    return $false
}

function Get-ProcessTreeMemory {
    $all = @(Get-CimInstance Win32_Process)
    $ids = [System.Collections.Generic.HashSet[int]]::new()
    $pending = [System.Collections.Generic.Queue[int]]::new()
    $ids.Add($process.Id) | Out-Null
    $pending.Enqueue($process.Id)
    while ($pending.Count -gt 0) {
        $parent = $pending.Dequeue()
        foreach ($candidate in $all) {
            if ($candidate.ParentProcessId -eq $parent -and $ids.Add([int]$candidate.ProcessId)) {
                $pending.Enqueue([int]$candidate.ProcessId)
            }
        }
    }
    $tree = @($ids | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
    return [pscustomobject]@{
        workingSetBytes = [int64](($tree | Measure-Object -Property WorkingSet64 -Sum).Sum)
        peakWorkingSetBytes = [int64](($tree | Measure-Object -Property PeakWorkingSet64 -Sum).Sum)
        privateMemoryBytes = [int64](($tree | Measure-Object -Property PrivateMemorySize64 -Sum).Sum)
        processCount = $tree.Count
    }
}

function Get-EpubFrameState {
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $tree = Invoke-CdpCommand -Socket $socket -Method 'Page.getFrameTree'
        $child = @($tree.frameTree.childFrames) |
            Where-Object { $_.frame.url -like 'http://readloom-epub.localhost/*' } |
            Select-Object -First 1
        if ($null -ne $child) {
            $world = Invoke-CdpCommand -Socket $socket -Method 'Page.createIsolatedWorld' -Parameters @{
                frameId = $child.frame.id
                worldName = 'readloom-stage4a-validation'
                grantUniveralAccess = $false
            }
            $evaluated = Invoke-CdpCommand -Socket $socket -Method 'Runtime.evaluate' -Parameters @{
                expression = '({ text: document.body?.innerText ?? "", imageCount: document.images.length, internalImageCount: [...document.images].filter((image) => image.currentSrc.startsWith("http://readloom-epub.localhost/") && image.complete && image.naturalWidth > 0).length, stylesheetCount: document.styleSheets.length, stylesheetHrefs: [...document.styleSheets].map((sheet) => sheet.href), headingColor: getComputedStyle(document.querySelector("h1") ?? document.body).color, fontLoaded: document.fonts?.check("16px ReadloomValidation") ?? false, fontStatus: document.fonts?.status ?? "unavailable", fontProbeFamily: getComputedStyle([...document.querySelectorAll("span")].find((node) => node.textContent?.includes("Internal font probe")) ?? document.body).fontFamily, fontFaces: [...(document.fonts ?? [])].map((face) => ({ family: face.family, status: face.status })), fontRules: [...document.styleSheets].flatMap((sheet) => { try { return [...sheet.cssRules].map((rule) => rule.cssText); } catch (error) { return ["blocked:" + error]; } }).filter((rule) => /font-face|validation\.ttf|blocked:/i.test(rule)), fontResources: performance.getEntriesByType("resource").filter((entry) => /font|\.ttf/i.test(entry.name)).map((entry) => entry.name) })'
                contextId = $world.executionContextId
                returnByValue = $true
            }
            $state = [pscustomobject]@{
                url = $child.frame.url
                text = $evaluated.result.value.text
                imageCount = $evaluated.result.value.imageCount
                internalImageCount = $evaluated.result.value.internalImageCount
                stylesheetCount = $evaluated.result.value.stylesheetCount
                headingColor = $evaluated.result.value.headingColor
                fontLoaded = $evaluated.result.value.fontLoaded
                fontStatus = $evaluated.result.value.fontStatus
                fontProbeFamily = $evaluated.result.value.fontProbeFamily
                fontFaces = $evaluated.result.value.fontFaces
                fontRules = $evaluated.result.value.fontRules
                fontResources = $evaluated.result.value.fontResources
                stylesheetHrefs = $evaluated.result.value.stylesheetHrefs
            }
            if ($state.text) { return $state }
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
                $frameWebSocketUrl = @($iframeTarget.webSocketDebuggerUrl)[0]
                $null = $frameSocket.ConnectAsync(
                    [Uri]$frameWebSocketUrl,
                    [Threading.CancellationToken]::None
                ).GetAwaiter().GetResult()
                Invoke-CdpCommand -Socket $frameSocket -Method 'Runtime.enable' | Out-Null
                $evaluated = Invoke-CdpCommand -Socket $frameSocket -Method 'Runtime.evaluate' -Parameters @{
                    expression = '({ text: document.body?.innerText ?? "", imageCount: document.images.length, internalImageCount: [...document.images].filter((image) => image.currentSrc.startsWith("http://readloom-epub.localhost/") && image.complete && image.naturalWidth > 0).length, stylesheetCount: document.styleSheets.length, stylesheetHrefs: [...document.styleSheets].map((sheet) => sheet.href), headingColor: getComputedStyle(document.querySelector("h1") ?? document.body).color, fontLoaded: document.fonts?.check("16px ReadloomValidation") ?? false, fontStatus: document.fonts?.status ?? "unavailable", fontProbeFamily: getComputedStyle([...document.querySelectorAll("span")].find((node) => node.textContent?.includes("Internal font probe")) ?? document.body).fontFamily, fontFaces: [...(document.fonts ?? [])].map((face) => ({ family: face.family, status: face.status })), fontRules: [...document.styleSheets].flatMap((sheet) => { try { return [...sheet.cssRules].map((rule) => rule.cssText); } catch (error) { return ["blocked:" + error]; } }).filter((rule) => /font-face|validation\.ttf|blocked:/i.test(rule)), fontResources: performance.getEntriesByType("resource").filter((entry) => /font|\.ttf/i.test(entry.name)).map((entry) => entry.name) })'
                    returnByValue = $true
                }
                $state = [pscustomobject]@{
                    url = $iframeTarget.url
                    text = $evaluated.result.value.text
                    imageCount = $evaluated.result.value.imageCount
                    internalImageCount = $evaluated.result.value.internalImageCount
                    stylesheetCount = $evaluated.result.value.stylesheetCount
                    headingColor = $evaluated.result.value.headingColor
                    fontLoaded = $evaluated.result.value.fontLoaded
                    fontStatus = $evaluated.result.value.fontStatus
                    fontProbeFamily = $evaluated.result.value.fontProbeFamily
                    fontFaces = $evaluated.result.value.fontFaces
                    fontRules = $evaluated.result.value.fontRules
                    fontResources = $evaluated.result.value.fontResources
                    stylesheetHrefs = $evaluated.result.value.stylesheetHrefs
                }
                if ($state.text) { return $state }
            }
            finally {
                $frameSocket.Dispose()
            }
        }
        if ([DateTime]::UtcNow -gt $deadline) {
            throw 'The active EPUB iframe was not available.'
        }
        Start-Sleep -Milliseconds 100
    } while ($true)
}

try {
    $endpoint = "http://127.0.0.1:$DebugPort/json/list"
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        try { $targets = @(Invoke-RestMethod -Uri $endpoint -TimeoutSec 1) }
        catch { $targets = @() }
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

    Open-AppFile -Path $InputPath
    $originalTitleJson = $OriginalTitle | ConvertTo-Json -Compress
    Wait-JavaScript -Description 'the original EPUB reader' -Expression @"
(() => document.body.innerText.includes($originalTitleJson) && document.querySelectorAll('iframe[sandbox="allow-scripts"]').length === 1)()
"@ | Out-Null
    Write-Host '[stage4a-ui] source EPUB opened'

    $originalFrame = Get-EpubFrameState
    if (-not (Test-ChapterMarker -Text $originalFrame.text -Markers $ChapterMarker)) {
        throw "Original chapter did not render: $($originalFrame | ConvertTo-Json -Compress)"
    }

    Click-Button -Text '编辑书籍信息'
    $initialEditState = Wait-JavaScript -Description 'the lazy edit draft' -Expression @'
(() => {
  const panel = document.querySelector('aside[aria-label="编辑书籍信息"]');
  const save = [...(panel?.querySelectorAll('button') ?? [])].find((button) => button.textContent?.trim() === '另存为 EPUB');
  return panel && save?.disabled ? { text: panel.innerText, saveDisabled: true } : false;
})()
'@
    if ($NoOriginalCover -and $initialEditState.text -notlike '*出版物没有显式封面*') {
        throw 'The no-cover publication was not represented as having no explicit cover.'
    }
    Write-Host '[stage4a-ui] clean lazy draft opened'

    Click-Button -Text '替换封面'
    Submit-NativeSelection -Path $PngCoverPath -Description 'PNG cover'
    Wait-JavaScript -Description 'the PNG cover preview' -Expression @'
(() => document.querySelector('aside[aria-label="编辑书籍信息"]')?.innerText.includes('image/png · 120 × 180') ?? false)()
'@ | Out-Null
    $pngPreviewLoaded = Wait-JavaScript -Description 'the isolated PNG preview resource' -Expression @'
(() => {
  const image = document.querySelector('img[alt="当前 EPUB 封面预览"]');
  return image?.complete && image.naturalWidth > 0 && image.currentSrc.includes('__readloom_edit/');
})()
'@
    Write-Host '[stage4a-ui] PNG cover preview loaded'

    Click-Button -Text '替换封面'
    Submit-NativeSelection -Path $JpegCoverPath -Description 'JPEG cover'
    Wait-JavaScript -Description 'the JPEG cover preview' -Expression @'
(() => document.querySelector('aside[aria-label="编辑书籍信息"]')?.innerText.includes('image/jpeg · 120 × 180') ?? false)()
'@ | Out-Null
    $jpegPreviewLoaded = Wait-JavaScript -Description 'the isolated JPEG preview resource' -Expression @'
(() => {
  const image = document.querySelector('img[alt="当前 EPUB 封面预览"]');
  return image?.complete && image.naturalWidth > 0 && image.currentSrc.includes('__readloom_edit/');
})()
'@
    Write-Host '[stage4a-ui] JPEG cover preview loaded'
    $coverMemory = Get-ProcessTreeMemory

    $titleJson = $EditedTitle | ConvertTo-Json -Compress
    $creatorsJson = (($EditedCreators -split '\|' | ForEach-Object { $_.Trim() }) -join "`n") | ConvertTo-Json -Compress
    $languageJson = $EditedLanguage | ConvertTo-Json -Compress
    $publisherJson = $EditedPublisher | ConvertTo-Json -Compress
    $descriptionJson = $EditedDescription | ConvertTo-Json -Compress
    $metadataApplied = Invoke-JavaScript -Expression @"
(() => {
  const set = (label, value) => {
    const element = document.querySelector('[aria-label="' + label + '"]');
    if (!(element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement)) return false;
    element.value = value;
    element.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  if (!set('书名', $titleJson) || !set('作者列表', $creatorsJson) ||
      !set('语言', $languageJson) || !set('出版社', $publisherJson) ||
      !set('简介', $descriptionJson)) return false;
  const apply = [...document.querySelectorAll('button')].find((button) => button.textContent?.trim() === '应用元数据');
  if (!apply) return false;
  apply.click();
  return true;
})()
"@
    if (-not $metadataApplied) { throw 'The metadata form could not be applied.' }

    Wait-JavaScript -Description 'structured dirty metadata' -Expression @"
(() => {
  const panel = document.querySelector('aside[aria-label="编辑书籍信息"]');
  const save = [...(panel?.querySelectorAll('button') ?? [])].find((button) => button.textContent?.trim() === '另存为 EPUB');
  return panel?.innerText.includes('有未保存修改') && !save?.disabled && document.querySelector('[aria-label="书名"]')?.value === $titleJson;
})()
"@ | Out-Null
    Write-Host '[stage4a-ui] metadata applied and draft dirty'

    Invoke-JavaScript -Expression 'window.confirm = () => false; true' | Out-Null
    Click-Button -Text '另存为 EPUB'
    $saveStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    Submit-NativeSelection -Path $OutputPath -Description 'Save As'
    $savedState = Wait-JavaScript -Description 'the safe Save As completion' -TimeoutSeconds 60 -Expression @'
(() => {
  const panel = document.querySelector('aside[aria-label="编辑书籍信息"]');
  const save = [...(panel?.querySelectorAll('button') ?? [])].find((button) => button.textContent?.trim() === '另存为 EPUB');
  return panel?.innerText.includes('没有未保存修改') && save?.disabled ? true : false;
})()
'@
    $saveStopwatch.Stop()
    if (-not $savedState -or -not (Test-Path -LiteralPath $OutputPath -PathType Leaf)) {
        throw 'The generated EPUB was not committed to the selected path.'
    }
    $saveMemory = Get-ProcessTreeMemory
    $sourceHashAfter = (Get-FileHash -Algorithm SHA256 -LiteralPath $InputPath).Hash
    if ($sourceHashAfter -ne $sourceHashBefore) {
        throw 'The original EPUB changed during Save As.'
    }
    Write-Host '[stage4a-ui] Save As completed; source unchanged'

    Open-AppFile -Path $OutputPath
    Wait-JavaScript -Description 'the generated EPUB in a new tab' -Expression @"
(() => document.body.innerText.includes($titleJson) && document.querySelectorAll('iframe[sandbox="allow-scripts"]').length === 1)()
"@ | Out-Null
    $generatedFrame = Get-EpubFrameState
    if ($ExpectInternalFont) {
        $fontDeadline = [DateTime]::UtcNow.AddSeconds(10)
        while (-not $generatedFrame.fontLoaded -and [DateTime]::UtcNow -lt $fontDeadline) {
            Start-Sleep -Milliseconds 100
            $generatedFrame = Get-EpubFrameState
        }
    }
    if (-not (Test-ChapterMarker -Text $generatedFrame.text -Markers $GeneratedChapterMarker) -or
        $generatedFrame.stylesheetCount -lt 1) {
        throw "Generated EPUB chapter/CSS did not render: $($generatedFrame | ConvertTo-Json -Compress)"
    }
    if ($ExpectInternalFont -and -not $generatedFrame.fontLoaded) {
        throw "Generated EPUB internal font did not load: $($generatedFrame | ConvertTo-Json -Compress)"
    }
    if ($generatedFrame.internalImageCount -ne $ExpectedInternalImages) {
        throw "Generated EPUB internal images did not load: $($generatedFrame | ConvertTo-Json -Compress)"
    }
    Write-Host '[stage4a-ui] generated EPUB reopened; chapter and CSS loaded'

    Click-Button -Text '书籍信息'
    $versionJson = "EPUB $ExpectedVersion" | ConvertTo-Json -Compress
    $readerMetadata = Wait-JavaScript -Description 'the generated metadata and cover in the reader' -Expression @"
(() => {
  const pane = document.querySelector('aside[aria-label="EPUB 元数据"]');
  const image = pane?.querySelector('img');
  if (!pane || !image?.complete || image.naturalWidth <= 0) return false;
  return {
    text: pane.innerText,
    coverSource: image.currentSrc,
    title: pane.querySelector('h2')?.textContent ?? '',
    versionOk: pane.innerText.includes($versionJson),
  };
})()
"@
    if ($readerMetadata.title -ne $EditedTitle -or
        -not $readerMetadata.versionOk -or
        $readerMetadata.text -notlike "*$EditedPublisher*" -or
        $readerMetadata.coverSource -notlike '*readloom-assets/cover-*.jpg*') {
        throw "Generated metadata/cover mismatch: $($readerMetadata | ConvertTo-Json -Compress)"
    }
    Write-Host '[stage4a-ui] generated metadata and cover visible in reader'

    $screenshot = Invoke-CdpCommand -Socket $socket -Method 'Page.captureScreenshot' -Parameters @{
        format = 'png'
        captureBeyondViewport = $false
    }
    [System.IO.File]::WriteAllBytes(
        $screenshotPath,
        [Convert]::FromBase64String($screenshot.data)
    )

    Click-Button -Text '编辑书籍信息'
    $reopenedDraft = Wait-JavaScript -Description 'the generated EPUB metadata draft' -Expression @"
(() => {
  const panel = document.querySelector('aside[aria-label="编辑书籍信息"]');
  if (!panel || !panel.innerText.includes('没有未保存修改')) return false;
  return {
    language: document.querySelector('[aria-label="语言"]')?.value ?? '',
    publisher: document.querySelector('[aria-label="出版社"]')?.value ?? '',
    description: document.querySelector('[aria-label="简介"]')?.value ?? '',
    creators: document.querySelector('[aria-label="作者列表"]')?.value ?? '',
  };
})()
"@
    if ($reopenedDraft.language -ne $EditedLanguage -or
        $reopenedDraft.publisher -ne $EditedPublisher -or
        $reopenedDraft.description -ne $EditedDescription) {
        throw "Generated metadata fields did not round trip: $($reopenedDraft | ConvertTo-Json -Compress)"
    }

    $process.Refresh()
    if (-not [ReadloomStage4AUiHarness]::PostMessage(
        $process.MainWindowHandle,
        0x0010,
        [IntPtr]::Zero,
        [IntPtr]::Zero
    )) {
        throw 'WM_CLOSE could not be delivered after Stage 4A validation.'
    }
    if (-not $process.WaitForExit(5000)) {
        throw 'Readloom did not exit after closing clean Stage 4A sessions.'
    }

    [pscustomobject]@{
        input = $InputPath
        output = $OutputPath
        version = $ExpectedVersion
        noOriginalCover = [bool]$NoOriginalCover
        originalSourceUnchanged = $sourceHashBefore -eq $sourceHashAfter
        initialDraftClean = [bool]$initialEditState.saveDisabled
        pngPreviewLoaded = [bool]$pngPreviewLoaded
        jpegPreviewLoaded = [bool]$jpegPreviewLoaded
        saveAsMs = [Math]::Round($saveStopwatch.Elapsed.TotalMilliseconds, 2)
        outputBytes = (Get-Item -LiteralPath $OutputPath).Length
        generatedTitle = $readerMetadata.title
        generatedCover = $readerMetadata.coverSource
        chapterReadable = Test-ChapterMarker -Text $generatedFrame.text -Markers $GeneratedChapterMarker
        cssLoaded = $generatedFrame.stylesheetCount -ge 1
        imagesLoaded = $generatedFrame.internalImageCount -eq $ExpectedInternalImages
        fontLoaded = [bool]$generatedFrame.fontLoaded
        coverStageMemory = $coverMemory
        saveStageMemory = $saveMemory
        exited = $true
        exitCode = $process.ExitCode
        screenshot = $screenshotPath
    }
}
finally {
    if ($null -ne $socket) { $socket.Dispose() }
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousBrowserArguments
    if (-not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $process.WaitForExit(5000) | Out-Null
    }
}
