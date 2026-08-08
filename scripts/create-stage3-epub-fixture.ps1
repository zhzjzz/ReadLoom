param(
    [string]$OutputPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation\readloom-stage3.epub')
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$outputDirectory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
if (Test-Path -LiteralPath $OutputPath) {
    Remove-Item -LiteralPath $OutputPath -Force
}

$stream = [System.IO.File]::Open($OutputPath, [System.IO.FileMode]::CreateNew)
$archive = [System.IO.Compression.ZipArchive]::new(
    $stream,
    [System.IO.Compression.ZipArchiveMode]::Create,
    $false
)

function Add-TextEntry {
    param(
        [string]$Name,
        [string]$Content,
        [System.IO.Compression.CompressionLevel]$Compression = [System.IO.Compression.CompressionLevel]::Optimal
    )

    $entry = $archive.CreateEntry($Name, $Compression)
    $entryStream = $entry.Open()
    $writer = [System.IO.StreamWriter]::new(
        $entryStream,
        [System.Text.UTF8Encoding]::new($false)
    )
    try {
        $writer.Write($Content)
    }
    finally {
        $writer.Dispose()
    }
}

function Add-BinaryEntry {
    param(
        [string]$Name,
        [byte[]]$Content
    )

    $entry = $archive.CreateEntry($Name, [System.IO.Compression.CompressionLevel]::Optimal)
    $entryStream = $entry.Open()
    try {
        $entryStream.Write($Content, 0, $Content.Length)
    }
    finally {
        $entryStream.Dispose()
    }
}

try {
    Add-TextEntry -Name 'mimetype' -Content 'application/epub+zip' -Compression ([System.IO.Compression.CompressionLevel]::NoCompression)
    Add-TextEntry -Name 'META-INF/container.xml' -Content @'
<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>
'@
    Add-TextEntry -Name 'EPUB/package.opf' -Content @'
<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="pub-id">urn:readloom:validation:stage3</dc:identifier>
    <dc:title>阅织阶段三验收书</dc:title>
    <dc:creator>Readloom</dc:creator>
    <dc:language>zh-CN</dc:language>
    <meta property="dcterms:modified">2026-08-08T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="style" href="styles/book.css" media-type="text/css"/>
    <item id="cover" href="images/cover.png" media-type="image/png" properties="cover-image"/>
    <item id="chapter-1" href="text/chapter-1.xhtml" media-type="application/xhtml+xml"/>
    <item id="chapter-2" href="text/chapter-2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="chapter-1"/>
    <itemref idref="chapter-2"/>
  </spine>
</package>
'@
    Add-TextEntry -Name 'EPUB/nav.xhtml' -Content @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <head><title>目录</title></head>
  <body><nav epub:type="toc"><ol>
    <li><a href="text/chapter-1.xhtml#opening">第一章 安全打开</a></li>
    <li><a href="text/chapter-2.xhtml#continue">第二章 继续阅读</a></li>
  </ol></nav></body>
</html>
'@
    Add-TextEntry -Name 'EPUB/styles/book.css' -Content @'
body { color: #25324a; background: #fbfaf7; line-height: 1.8; }
h1 { color: #2359b8; }
.cover { width: 96px; height: 96px; image-rendering: pixelated; }
'@
    Add-TextEntry -Name 'EPUB/text/chapter-1.xhtml' -Content @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head>
  <title>第一章 安全打开</title><link rel="stylesheet" href="../styles/book.css"/>
</head><body><h1 id="opening">第一章 安全打开</h1>
<script>window.publisherScriptExecuted = true</script>
<img class="cover" src="../images/cover.png" alt="蓝色验收图"/>
<img src="https://tracker.invalid/pixel.png" alt="该外部图片必须被移除"/>
<p>这是 Readloom 阶段三的自有 EPUB 验收内容。中文排版、内部资源和阅读进度应当正常。</p>
<p><a href="https://example.com/read?q=1">复制外部链接</a></p>
<p><a href="chapter-2.xhtml#continue">前往第二章</a></p>
</body></html>
'@
    Add-TextEntry -Name 'EPUB/text/chapter-2.xhtml' -Content @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head>
  <title>第二章 继续阅读</title><link rel="stylesheet" href="../styles/book.css"/>
</head><body><h1 id="continue">第二章 继续阅读</h1>
<p>第二章用于验证目录跳转、上一章、搜索、书签以及关闭后恢复。</p>
<p><a href="chapter-1.xhtml#opening">返回第一章</a></p>
</body></html>
'@
    Add-BinaryEntry -Name 'EPUB/images/cover.png' -Content ([Convert]::FromBase64String(
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII='
    ))
}
finally {
    $archive.Dispose()
    $stream.Dispose()
}

Get-Item -LiteralPath $OutputPath | Select-Object FullName, Length
