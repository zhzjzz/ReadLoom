param(
    [string]$ExecutablePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri\target\release\readloom.exe'),
    [string]$InputPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\stage4b-normal.epub'),
    [string]$OutputPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\stage4b-normal-edited.epub'),
    [string]$ImagePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\stage4b-import.png'),
    [string]$ReadOnlyPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\stage4b-read-only.epub'),
    [string]$LargePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\stage4b-large.epub'),
    [switch]$CompatibilityProbeOnly,
    [string]$ExpectedTitle = '为美好的世界献上祝福',
    [ValidateRange(1, 10000)]
    [int]$ExpectedChapterPosition = 5,
    [ValidateRange(1024, 65535)]
    [int]$DebugPort = 9242
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Threading;
using System.Runtime.InteropServices;

public static class ReadloomStage4BUiHarness
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
}
'@

foreach ($requiredPath in @($ExecutablePath, $InputPath, $ImagePath, $ReadOnlyPath, $LargePath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Stage 4B validation file not found: $requiredPath"
    }
}

$ExecutablePath = (Resolve-Path -LiteralPath $ExecutablePath).Path
$InputPath = (Resolve-Path -LiteralPath $InputPath).Path
$ImagePath = (Resolve-Path -LiteralPath $ImagePath).Path
$ReadOnlyPath = (Resolve-Path -LiteralPath $ReadOnlyPath).Path
$LargePath = (Resolve-Path -LiteralPath $LargePath).Path
$OutputPath = [System.IO.Path]::GetFullPath($OutputPath)
$outputDirectory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
if (Test-Path -LiteralPath $OutputPath) { Remove-Item -LiteralPath $OutputPath -Force }
$screenshotPath = [System.IO.Path]::ChangeExtension($OutputPath, '.png')
$sourceHashBefore = (Get-FileHash -Algorithm SHA256 -LiteralPath $InputPath).Hash
$webViewProcessIdsBefore = [System.Collections.Generic.HashSet[int]]::new()
foreach ($webViewProcess in @(Get-Process -Name msedgewebview2 -ErrorAction SilentlyContinue)) {
    $webViewProcessIdsBefore.Add($webViewProcess.Id) | Out-Null
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
    $payload = @{ id = $commandId; method = $Method; params = $Parameters } |
        ConvertTo-Json -Depth 20 -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($payload)
    $null = $Socket.SendAsync(
        [ArraySegment[byte]]::new($bytes),
        [System.Net.WebSockets.WebSocketMessageType]::Text,
        $true,
        [Threading.CancellationToken]::None
    ).GetAwaiter().GetResult()
    $receiveTimeout = [Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(15))
    try {
    while ($true) {
        $buffer = [byte[]]::new(65536)
        $message = [System.IO.MemoryStream]::new()
        do {
            $received = $Socket.ReceiveAsync(
                [ArraySegment[byte]]::new($buffer),
                $receiveTimeout.Token
            ).GetAwaiter().GetResult()
            $message.Write($buffer, 0, $received.Count)
        } while (-not $received.EndOfMessage)
        $json = [System.Text.Encoding]::UTF8.GetString($message.ToArray()) | ConvertFrom-Json
        $message.Dispose()
        if ($json.id -eq $commandId) {
            if ($null -ne $json.error) { throw "CDP $Method failed: $($json.error.message)" }
            return $json.result
        }
    }
    }
    catch {
        throw "CDP $Method timed out or disconnected: $($_.Exception.Message)"
    }
    finally { $receiveTimeout.Dispose() }
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
    param([string]$Expression, [string]$Description, [int]$TimeoutSeconds = 30)
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

function Click-Button {
    param([string]$Text)
    $textJson = $Text | ConvertTo-Json -Compress
    $clicked = Invoke-JavaScript -Expression @"
(() => {
  const text = $textJson;
  const button = [...document.querySelectorAll('button')]
    .find((candidate) => candidate.textContent?.trim() === text && !candidate.disabled);
  if (!button) return false;
  if (text === '导入图片' || text === '另存为') setTimeout(() => button.click(), 500);
  else button.click();
  return true;
})()
"@
    if (-not $clicked) { throw "Enabled button not found: $Text" }
}

function Click-LabelledButton {
    param([string]$Label)
    $labelJson = $Label | ConvertTo-Json -Compress
    $clicked = Invoke-JavaScript -Expression @"
(() => {
  const button = [...document.querySelectorAll('button')]
    .find((candidate) => candidate.getAttribute('aria-label') === $labelJson && !candidate.disabled);
  if (!button) return false;
  button.click();
  return true;
})()
"@
    if (-not $clicked) { throw "Enabled labelled button not found: $Label" }
}

function Submit-NativeSelection {
    param([string]$Path, [string]$Description)
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $dialog = [ReadloomStage4BUiHarness]::FindFileDialog([uint32]$process.Id)
        if ([DateTime]::UtcNow -gt $deadline) {
            throw "Timed out waiting for the native $Description dialog."
        }
    } while ($dialog -eq [IntPtr]::Zero)
    if (-not [ReadloomStage4BUiHarness]::SubmitFileDialog($dialog, $Path)) {
        throw "The native $Description dialog did not accept: $Path"
    }
    $closeDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 250
        $remainingDialog = [ReadloomStage4BUiHarness]::FindFileDialog([uint32]$process.Id)
        if ($remainingDialog -eq [IntPtr]::Zero) { break }
        if ([DateTime]::UtcNow -gt $closeDeadline) {
            throw "The native $Description dialog did not close after accepting: $Path"
        }
    } while ($true)
    Start-Sleep -Milliseconds 1000
}

function Open-AppFile {
    param([string]$Path)
    $opened = Invoke-JavaScript -Expression @'
(() => {
  const button = [...document.querySelectorAll('button')]
    .find((candidate) => ['打开文件', '打开'].includes(candidate.textContent?.trim()) && !candidate.disabled);
  if (!button) return false;
  button.click();
  return true;
})()
'@
    if (-not $opened) { throw 'No enabled open-file button was available.' }
    Submit-NativeSelection -Path $Path -Description 'open file'
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

function Read-ZipText {
    param([string]$ArchivePath, [string]$EntryName)
    $stream = [System.IO.File]::OpenRead($ArchivePath)
    $archive = [System.IO.Compression.ZipArchive]::new($stream, [System.IO.Compression.ZipArchiveMode]::Read)
    try {
        $entry = $archive.GetEntry($EntryName)
        if ($null -eq $entry) { throw "Missing EPUB entry: $EntryName" }
        $reader = [System.IO.StreamReader]::new($entry.Open(), [System.Text.Encoding]::UTF8)
        try { return $reader.ReadToEnd() }
        finally { $reader.Dispose() }
    }
    finally {
        $archive.Dispose()
        $stream.Dispose()
    }
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
            if ($process.HasExited) { throw "Readloom exited during startup ($($process.ExitCode))." }
            if ([DateTime]::UtcNow -gt $deadline) { throw 'Release WebView2 target did not start.' }
            Start-Sleep -Milliseconds 100
            $process.Refresh()
        }
    } while ($null -eq $page)

    $socket = [System.Net.WebSockets.ClientWebSocket]::new()
    $null = $socket.ConnectAsync([Uri]$page.webSocketDebuggerUrl, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
    Invoke-CdpCommand -Socket $socket -Method 'Page.enable' | Out-Null
    Invoke-CdpCommand -Socket $socket -Method 'Runtime.enable' | Out-Null

    $initialAssets = @(Invoke-JavaScript -Expression 'performance.getEntriesByType("resource").map((entry) => entry.name)')
    if ($initialAssets -match 'EpubChapterEditor|tiptap|prosemirror' -or
        (Invoke-JavaScript -Expression 'document.querySelectorAll(".ProseMirror").length') -ne 0) {
        throw 'Chapter editor code or DOM loaded before the explicit editing action.'
    }
    Write-Host '[stage4b-ui] initial reader has no editor assets or view'

    Open-AppFile -Path $InputPath
    if ($CompatibilityProbeOnly) {
        $titleJson = $ExpectedTitle | ConvertTo-Json -Compress
        Wait-JavaScript -Description 'compatibility probe EPUB reader' -TimeoutSeconds 60 -Expression @"
(() => document.body.innerText.includes($titleJson) && document.querySelectorAll('iframe[sandbox="allow-scripts"]').length === 1)()
"@ | Out-Null
        for ($attempt = 0; $attempt -lt 100; $attempt += 1) {
            $position = Invoke-JavaScript -Expression @'
(() => {
  const match = document.body.innerText.match(/(\d+)\s*\/\s*(\d+)\s*·/);
  return match ? { current: Number(match[1]), total: Number(match[2]) } : null;
})()
'@
            if ($null -ne $position -and $position.current -eq $ExpectedChapterPosition) { break }
            if ($null -eq $position) { throw 'Could not read the current EPUB chapter position.' }
            $label = if ($position.current -lt $ExpectedChapterPosition) { '下一章' } else { '上一章' }
            Click-LabelledButton -Label $label
            Start-Sleep -Milliseconds 250
        }
        if ($position.current -ne $ExpectedChapterPosition) {
            throw "Could not navigate to EPUB chapter position $ExpectedChapterPosition."
        }
        Click-Button -Text '编辑当前章节'
        $compatibilityState = Wait-JavaScript -Description 'publisher XHTML editable chapter' -TimeoutSeconds 60 -Expression @'
(() => {
  const editor = document.querySelector('.ProseMirror[contenteditable="true"]');
  if (!editor || document.querySelectorAll('.ProseMirror').length !== 1) return false;
  return {
    publisherSpan: Boolean(editor.querySelector('span[role="heading"]')),
    imageIdentity: Boolean(editor.querySelector('img#image-26.s2.s3')),
    readOnlyWarning: document.body.innerText.includes('章节包含无法可靠保留的元素属性'),
  };
})()
'@
        if (-not $compatibilityState.publisherSpan -or -not $compatibilityState.imageIdentity -or
            $compatibilityState.readOnlyWarning) {
            throw "Publisher XHTML attributes were not preserved in the editable DOM: $($compatibilityState | ConvertTo-Json -Compress)"
        }
        $editingAssets = @(Invoke-JavaScript -Expression 'performance.getEntriesByType("resource").map((entry) => entry.name)')
        if (-not ($editingAssets -match 'EpubChapterEditor')) {
            throw 'The editor dynamic component was not observed during the compatibility probe.'
        }
        $screenshot = Invoke-CdpCommand -Socket $socket -Method 'Page.captureScreenshot' -Parameters @{
            format = 'png'
            captureBeyondViewport = $false
        }
        [System.IO.File]::WriteAllBytes($screenshotPath, [Convert]::FromBase64String($screenshot.data))
        return [pscustomobject]@{
            input = $InputPath
            sourceUnchanged = ((Get-FileHash -Algorithm SHA256 -LiteralPath $InputPath).Hash -eq $sourceHashBefore)
            chapterPosition = $position.current
            spineLength = $position.total
            editable = $true
            publisherSpanPreserved = [bool]$compatibilityState.publisherSpan
            imageIdentityPreserved = [bool]$compatibilityState.imageIdentity
            readOnlyWarning = [bool]$compatibilityState.readOnlyWarning
            screenshot = $screenshotPath
        }
    }
    Wait-JavaScript -Description 'normal EPUB reader' -Expression @'
(() => document.body.innerText.includes('阶段 4B 正常编辑书') && document.querySelectorAll('iframe[sandbox="allow-scripts"]').length === 1)()
'@ | Out-Null
    Write-Host '[stage4b-ui] normal EPUB opened in isolated reader'
    Click-Button -Text '编辑当前章节'
    $editorLoad = [System.Diagnostics.Stopwatch]::StartNew()
    $firstEditor = Wait-JavaScript -Description 'first editable ProseMirror chapter' -Expression @'
(() => {
  const editor = document.querySelector('.ProseMirror[contenteditable="true"]');
  return editor && document.querySelectorAll('.ProseMirror').length === 1 &&
    document.body.innerText.includes('第一章 可视化编辑') ? { text: editor.innerText } : false;
})()
'@
    $editorLoad.Stop()
    $editingAssets = @(Invoke-JavaScript -Expression 'performance.getEntriesByType("resource").map((entry) => entry.name)')
    if (-not ($editingAssets -match 'EpubChapterEditor')) {
        throw 'The editor dynamic component was not observed after entering edit mode.'
    }
    Write-Host '[stage4b-ui] editor dynamically loaded with one EditorView'

    Invoke-JavaScript -Expression @'
(() => {
  const editor = document.querySelector('.ProseMirror');
  editor.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: '拼' }));
  return true;
})()
'@ | Out-Null
    Write-Host '[stage4b-ui] IME composition gate observed'
    $compositionGate = Wait-JavaScript -Description 'IME local-only composition state' -Expression @'
(() => document.querySelector('.sync-status')?.dataset.status === 'typing')()
'@
    Invoke-JavaScript -Expression @'
(() => {
  document.querySelector('.ProseMirror').dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '拼音' }));
  return true;
})()
'@ | Out-Null

    $insertedFirst = Invoke-JavaScript -Expression @'
(() => {
  const editor = document.querySelector('.ProseMirror');
  editor.focus();
  const selection = window.getSelection();
  const range = document.createRange();
  range.selectNodeContents(editor);
  range.collapse(false);
  selection.removeAllRanges();
  selection.addRange(range);
  document.execCommand('insertParagraph');
  return document.execCommand('insertText', false, '第一章实时修改：中文 😀 é');
})()
'@
    if (-not $insertedFirst) { throw 'Could not insert first-chapter text through contenteditable.' }
    Wait-JavaScript -Description 'first chapter Rust synchronization' -Expression @'
(() => document.querySelector('.sync-status')?.dataset.status === 'synced' && document.body.innerText.includes('本章有修改'))()
'@ | Out-Null
    Write-Host '[stage4b-ui] first chapter synchronized'

    $selectedMarker = Invoke-JavaScript -Expression @'
(() => {
  const editor = document.querySelector('.ProseMirror');
  const walker = document.createTreeWalker(editor, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) {
    const node = walker.currentNode;
    const start = node.data.indexOf('第一章实时修改');
    if (start < 0) continue;
    const range = document.createRange();
    range.setStart(node, start);
    range.setEnd(node, start + '第一章实时修改'.length);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
    return true;
  }
  return false;
})()
'@
    if (-not $selectedMarker) { throw 'Could not select the formatting marker.' }
    Click-Button -Text 'B'
    Wait-JavaScript -Description 'bold toolbar command' -Expression @'
(() => [...document.querySelectorAll('.ProseMirror strong')].some((node) => node.textContent.includes('第一章实时修改')))()
'@ | Out-Null
    Wait-JavaScript -Description 'enabled chapter undo command' -Expression @'
(() => {
  const button = [...document.querySelectorAll('button')].find((candidate) => candidate.getAttribute('aria-label') === '撤销');
  return Boolean(button && !button.disabled);
})()
'@ | Out-Null
    Click-LabelledButton -Label '撤销'
    Wait-JavaScript -Description 'chapter undo' -Expression @'
(() => ![...document.querySelectorAll('.ProseMirror strong')].some((node) => node.textContent.includes('第一章实时修改')))()
'@ | Out-Null
    Wait-JavaScript -Description 'enabled chapter redo command' -Expression @'
(() => {
  const button = [...document.querySelectorAll('button')].find((candidate) => candidate.getAttribute('aria-label') === '重做');
  return Boolean(button && !button.disabled);
})()
'@ | Out-Null
    Click-LabelledButton -Label '重做'
    Wait-JavaScript -Description 'chapter redo' -Expression @'
(() => [...document.querySelectorAll('.ProseMirror strong')].some((node) => node.textContent.includes('第一章实时修改')))()
'@ | Out-Null
    Write-Host '[stage4b-ui] formatting toolbar and undo/redo verified'

    Click-Button -Text '关闭'
    Wait-JavaScript -Description 'dirty close confirmation' -Expression @'
(() => document.querySelector('[role="dialog"]')?.innerText.includes('保存对') ?? false)()
'@ | Out-Null
    Click-Button -Text '取消'
    Wait-JavaScript -Description 'cancelled dirty close' -Expression @'
(() => !document.querySelector('[role="dialog"]') && document.querySelectorAll('.ProseMirror').length === 1)()
'@ | Out-Null
    Write-Host '[stage4b-ui] dirty close confirmation preserved the draft'

    $pasted = Invoke-JavaScript -Expression @'
(() => {
  const editor = document.querySelector('.ProseMirror');
  const data = new DataTransfer();
  data.setData('text/html', '<p>安全粘贴文本<script>bad()</script><img src="https://tracker.invalid/pixel.png"/></p>');
  editor.focus();
  const selection = window.getSelection();
  const range = document.createRange();
  range.selectNodeContents(editor);
  range.collapse(false);
  selection.removeAllRanges();
  selection.addRange(range);
  return editor.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: data }));
})()
'@
    Start-Sleep -Milliseconds 800
    $pasteState = Invoke-JavaScript -Expression @'
(() => ({
  textPresent: document.querySelector('.ProseMirror')?.innerText.includes('安全粘贴文本') ?? false,
  scriptCount: document.querySelectorAll('.ProseMirror script').length,
  externalImages: [...document.querySelectorAll('.ProseMirror img')].filter((image) => image.src.includes('tracker.invalid')).length,
}))()
'@
    if (-not $pasteState.textPresent -or $pasteState.scriptCount -ne 0 -or $pasteState.externalImages -ne 0) {
        throw "Unsafe paste sanitization failed: $($pasteState | ConvertTo-Json -Compress)"
    }
    Write-Host '[stage4b-ui] unsafe paste sanitized'

    Invoke-JavaScript -Expression @'
(() => {
  window.prompt = () => '阶段4B导入图';
  return true;
})()
'@ | Out-Null
    Click-Button -Text '导入图片'
    Submit-NativeSelection -Path $ImagePath -Description 'chapter image'
    Wait-JavaScript -Description 'imported chapter image synchronization' -Expression @'
(() => {
  const image = [...document.querySelectorAll('.ProseMirror img')]
    .find((candidate) => candidate.src.includes('/readloom-'));
  return image && document.querySelector('.sync-status')?.dataset.status === 'synced';
})()
'@ | Out-Null
    Write-Host '[stage4b-ui] chapter image imported and alt updated'

    Click-Button -Text '编辑 + 预览'
    $previewState = Wait-JavaScript -Description 'accepted draft split preview' -Expression @'
(() => {
  const frame = document.querySelector('iframe[title^="Rust 已接受草稿预览"]');
  return frame && frame.getAttribute('sandbox') === 'allow-scripts' &&
    !frame.getAttribute('sandbox').includes('allow-same-origin');
})()
'@
    $screenshot = Invoke-CdpCommand -Socket $socket -Method 'Page.captureScreenshot' -Parameters @{
        format = 'png'
        captureBeyondViewport = $false
    }
    [System.IO.File]::WriteAllBytes($screenshotPath, [Convert]::FromBase64String($screenshot.data))
    Write-Host '[stage4b-ui] split preview isolated and screenshot captured'

    Click-LabelledButton -Label '下一章'
    Wait-JavaScript -Description 'second independent chapter draft' -Expression @'
(() => document.querySelector('.ProseMirror')?.innerText.includes('原始正文乙') && document.querySelectorAll('.ProseMirror').length === 1)()
'@ | Out-Null
    Write-Host '[stage4b-ui] two independent chapter drafts restored across switches'
    $insertedSecond = Invoke-JavaScript -Expression @'
(() => {
  const editor = document.querySelector('.ProseMirror');
  editor.focus();
  const selection = window.getSelection();
  const range = document.createRange();
  range.selectNodeContents(editor);
  range.collapse(false);
  selection.removeAllRanges();
  selection.addRange(range);
  document.execCommand('insertParagraph');
  return document.execCommand('insertText', false, '第二章独立修改');
})()
'@
    if (-not $insertedSecond) { throw 'Could not insert second-chapter text.' }
    Wait-JavaScript -Description 'second chapter synchronization' -Expression @'
(() => document.querySelector('.sync-status')?.dataset.status === 'synced')()
'@ | Out-Null
    Click-LabelledButton -Label '下一章'
    Wait-JavaScript -Description 'third chapter with one EditorView' -Expression @'
(() => document.querySelector('.ProseMirror')?.innerText.includes('第三章缓存淘汰') && document.querySelectorAll('.ProseMirror').length === 1)()
'@ | Out-Null
    Click-LabelledButton -Label '下一章'
    Wait-JavaScript -Description 'fourth chapter crossing the LRU limit' -Expression @'
(() => document.querySelector('.ProseMirror')?.innerText.includes('第四章触发三章缓存上限') && document.querySelectorAll('.ProseMirror').length === 1)()
'@ | Out-Null
    Click-LabelledButton -Label '上一章'
    Wait-JavaScript -Description 'third chapter after LRU crossing' -Expression @'
(() => document.querySelector('.ProseMirror')?.innerText.includes('第三章缓存淘汰') && document.querySelectorAll('.ProseMirror').length === 1)()
'@ | Out-Null
    Click-LabelledButton -Label '上一章'
    Wait-JavaScript -Description 'restored second chapter after LRU crossing' -Expression @'
(() => document.querySelector('.ProseMirror')?.innerText.includes('第二章独立修改') && document.querySelectorAll('.ProseMirror').length === 1)()
'@ | Out-Null
    Write-Host '[stage4b-ui] four chapter LRU traversal kept one EditorView and restored drafts'
    Click-LabelledButton -Label '上一章'
    Wait-JavaScript -Description 'restored first chapter editor state' -Expression @'
(() => document.querySelector('.ProseMirror')?.innerText.includes('安全粘贴文本') && document.querySelectorAll('.ProseMirror').length === 1)()
'@ | Out-Null

    $editorMemory = Get-ProcessTreeMemory
    Write-Host '[stage4b-ui] exited edit mode with modified chapter markers'
    Click-Button -Text '退出章节编辑'
    Wait-JavaScript -Description 'reader with modified chapter markers' -Expression @'
(() => document.querySelectorAll('.ProseMirror').length === 0 && document.body.innerText.includes('已修改'))()
'@ | Out-Null

    Click-Button -Text '另存为'
    $saveTimer = [System.Diagnostics.Stopwatch]::StartNew()
    Submit-NativeSelection -Path $OutputPath -Description 'Save As'
    Wait-JavaScript -Description 'chapter Save As completion' -TimeoutSeconds 60 -Expression @'
(() => {
  const save = [...document.querySelectorAll('button')].find((button) => button.textContent?.trim() === '另存为');
  return save?.disabled && !document.body.innerText.includes('有未保存修改');
})()
'@ | Out-Null
    $saveTimer.Stop()
    if (-not (Test-Path -LiteralPath $OutputPath -PathType Leaf)) {
        throw 'The chapter-edited EPUB was not committed.'
    }
    $sourceHashAfter = (Get-FileHash -Algorithm SHA256 -LiteralPath $InputPath).Hash
    if ($sourceHashAfter -ne $sourceHashBefore) { throw 'The source EPUB changed during chapter Save As.' }
    Write-Host '[stage4b-ui] Save As completed and source hash stayed unchanged'

    $savedOne = Read-ZipText -ArchivePath $OutputPath -EntryName 'EPUB/text/one.xhtml'
    $savedTwo = Read-ZipText -ArchivePath $OutputPath -EntryName 'EPUB/text/two.xhtml'
    $savedOpf = Read-ZipText -ArchivePath $OutputPath -EntryName 'EPUB/package.opf'
    if (-not $savedOne.Contains('安全粘贴文本') -or
        -not $savedOne.Contains('alt="阶段4B导入图"') -or $savedOne.Contains('tracker.invalid') -or
        -not $savedTwo.Contains('第二章独立修改') -or -not $savedOpf.Contains('readloom-image-')) {
        throw 'Saved XHTML or OPF did not contain the expected safe multi-chapter changes.'
    }
    Write-Host '[stage4b-ui] generated XHTML, image manifest, and both chapters verified'

    Open-AppFile -Path $OutputPath
    Wait-JavaScript -Description 'reopened generated EPUB reader' -Expression @'
(() => document.body.innerText.includes('阶段 4B 正常编辑书') && document.querySelectorAll('iframe[sandbox="allow-scripts"]').length === 1)()
'@ | Out-Null
    Click-Button -Text '编辑当前章节'
    Wait-JavaScript -Description 'reopened generated chapter draft' -Expression @'
(() => document.querySelector('.ProseMirror')?.innerText.includes('安全粘贴文本'))()
'@ | Out-Null
    Click-Button -Text '退出章节编辑'
    Write-Host '[stage4b-ui] generated EPUB reopened in editor'

    Open-AppFile -Path $ReadOnlyPath
    Wait-JavaScript -Description 'read-only structure reader' -Expression @'
(() => document.body.innerText.includes('阶段 4B 只读降级'))()
'@ | Out-Null
    Click-Button -Text '编辑当前章节'
    $readOnlyState = Wait-JavaScript -Description 'explicit read-only chapter degradation' -Expression @'
(() => document.body.innerText.includes('安全阅读模式') && document.querySelectorAll('.ProseMirror').length === 0)()
'@
    Write-Host '[stage4b-ui] unsupported structures degraded to read-only'

    Open-AppFile -Path $LargePath
    Wait-JavaScript -Description 'large chapter reader' -Expression @'
(() => document.body.innerText.includes('阶段 4B 大章节'))()
'@ | Out-Null
    $largeTimer = [System.Diagnostics.Stopwatch]::StartNew()
    Click-Button -Text '编辑当前章节'
    Wait-JavaScript -Description 'large chapter editor' -TimeoutSeconds 60 -Expression @'
(() => document.querySelector('.ProseMirror')?.innerText.includes('大章节正文'))()
'@ | Out-Null
    $largeTimer.Stop()
    $largeMemory = Get-ProcessTreeMemory
    Click-Button -Text '退出章节编辑'
    Write-Host '[stage4b-ui] large chapter opened and exited cleanly'

    $process.Refresh()
    if (-not [ReadloomStage4BUiHarness]::PostMessage(
        $process.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero
    )) { throw 'WM_CLOSE could not be delivered.' }
    if (-not $process.WaitForExit(5000)) { throw 'Readloom did not exit after clean validation.' }

    [pscustomobject]@{
        input = $InputPath
        output = $OutputPath
        sourceUnchanged = $sourceHashBefore -eq $sourceHashAfter
        editorLazyLoaded = -not ($initialAssets -match 'EpubChapterEditor') -and ($editingAssets -match 'EpubChapterEditor')
        oneEditorView = $true
        firstEditorLoadMs = [Math]::Round($editorLoad.Elapsed.TotalMilliseconds, 2)
        compositionGate = [bool]$compositionGate
        pasteSanitized = [bool]$pasteState.textPresent
        imageImportedAndAltEdited = $savedOne.Contains('alt="阶段4B导入图"')
        splitPreviewIsolated = [bool]$previewState
        multiChapterRoundTrip = $savedOne.Contains('安全粘贴文本') -and $savedTwo.Contains('第二章独立修改')
        saveAsMs = [Math]::Round($saveTimer.Elapsed.TotalMilliseconds, 2)
        outputBytes = (Get-Item -LiteralPath $OutputPath).Length
        generatedReopened = $true
        readOnlyDegradation = [bool]$readOnlyState
        largeChapterEditorLoadMs = [Math]::Round($largeTimer.Elapsed.TotalMilliseconds, 2)
        editorMemory = $editorMemory
        largeChapterMemory = $largeMemory
        screenshot = $screenshotPath
        exited = $true
        exitCode = $process.ExitCode
    }
}
finally {
    if ($null -ne $socket) { $socket.Dispose() }
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousBrowserArguments
    if (-not $process.HasExited) {
        & taskkill.exe /PID $process.Id /T /F 2>$null | Out-Null
        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
        $process.WaitForExit(5000) | Out-Null
    }
    Start-Sleep -Milliseconds 500
    for ($cleanupAttempt = 0; $cleanupAttempt -lt 3; $cleanupAttempt++) {
        foreach ($webViewProcess in @(Get-Process -Name msedgewebview2 -ErrorAction SilentlyContinue)) {
            if (-not $webViewProcessIdsBefore.Contains($webViewProcess.Id)) {
                Stop-Process -Id $webViewProcess.Id -Force -ErrorAction SilentlyContinue
            }
        }
        Start-Sleep -Milliseconds 250
    }
}
