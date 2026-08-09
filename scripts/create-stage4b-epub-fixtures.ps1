param(
    [string]$OutputDirectory = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation')
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.Drawing

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

$storedMimetypeArchive = [Convert]::FromBase64String(
    'UEsDBBQAAAAAAAAACF1vYassFAAAABQAAAAIAAAAbWltZXR5cGVhcHBsaWNhdGlvbi9lcHViK3ppcFBLAQIUABQAAAAAAAAACF1vYassFAAAABQAAAAIAAAAAAAAAAAAAACAAQAAAABtaW1ldHlwZVBLBQYAAAAAAQABADYAAAA6AAAAAAA='
)
$pictureBytes = [Convert]::FromBase64String(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII='
)
$picturePath = Join-Path $OutputDirectory 'stage4b-import.png'
$bitmap = [System.Drawing.Bitmap]::new(120, 180)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.Clear([System.Drawing.Color]::FromArgb(36, 84, 166))
    $graphics.FillRectangle([System.Drawing.Brushes]::Gold, 20, 28, 80, 124)
    $bitmap.Save($picturePath, [System.Drawing.Imaging.ImageFormat]::Png)
}
finally {
    $graphics.Dispose()
    $bitmap.Dispose()
}

function New-EpubFixture {
    param(
        [string]$Path,
        [object[]]$Entries
    )

    if (Test-Path -LiteralPath $Path) {
        Remove-Item -LiteralPath $Path -Force
    }
    [System.IO.File]::WriteAllBytes($Path, $storedMimetypeArchive)
    $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open)
    $archive = [System.IO.Compression.ZipArchive]::new(
        $stream,
        [System.IO.Compression.ZipArchiveMode]::Update,
        $false
    )
    try {
        foreach ($item in $Entries) {
            $entry = $archive.CreateEntry(
                [string]$item.Name,
                [System.IO.Compression.CompressionLevel]::Optimal
            )
            $entryStream = $entry.Open()
            try {
                if ($item.Content -is [byte[]]) {
                    $entryStream.Write($item.Content, 0, $item.Content.Length)
                }
                else {
                    $writer = [System.IO.StreamWriter]::new(
                        $entryStream,
                        [System.Text.UTF8Encoding]::new($false),
                        4096,
                        $true
                    )
                    try { $writer.Write([string]$item.Content) }
                    finally { $writer.Dispose() }
                }
            }
            finally { $entryStream.Dispose() }
        }
    }
    finally {
        $archive.Dispose()
        $stream.Dispose()
    }
}

$container = @'
<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>
'@
$normalOpf = @'
<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="pub-id">urn:readloom:validation:stage4b</dc:identifier><dc:title>阶段 4B 正常编辑书</dc:title><dc:creator>Readloom</dc:creator><dc:language>zh-CN</dc:language><meta property="dcterms:modified">2026-08-09T00:00:00Z</meta></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="style" href="styles/book.css" media-type="text/css"/><item id="picture" href="images/original.png" media-type="image/png"/><item id="one" href="text/one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="text/two.xhtml" media-type="application/xhtml+xml"/><item id="three" href="text/three.xhtml" media-type="application/xhtml+xml"/><item id="four" href="text/four.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/><itemref idref="three"/><itemref idref="four"/></spine></package>
'@
$normalNav = @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>目录</title></head><body><nav epub:type="toc"><ol><li><a href="text/one.xhtml#one">第一章 可视化编辑</a></li><li><a href="text/two.xhtml#two">第二章 多草稿</a></li><li><a href="text/three.xhtml#three">第三章 缓存淘汰</a></li><li><a href="text/four.xhtml#four">第四章 草稿恢复</a></li></ol></nav></body></html>
'@
$normalOne = @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>第一章 可视化编辑</title><link rel="stylesheet" href="../styles/book.css"/></head><body epub:type="bodymatter"><h1 id="one">第一章 可视化编辑</h1><p>原始正文甲：中文、emoji 😀 与组合字符 é。</p><p><strong>粗体</strong>、<em>斜体</em>、<a href="two.xhtml#two">内部链接</a>。</p><img src="../images/original.png" alt="原始插图" width="1" height="1"/></body></html>
'@
$normalTwo = @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>第二章 多草稿</title><link rel="stylesheet" href="../styles/book.css"/></head><body><h1 id="two">第二章 多草稿</h1><p>原始正文乙，用于切章和独立撤销。</p><ol start="2"><li><p>第二项</p></li></ol></body></html>
'@
$normalThree = @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>第三章 缓存淘汰</title><link rel="stylesheet" href="../styles/book.css"/></head><body><h1 id="three">第三章 缓存淘汰</h1><p>第三章缓存淘汰验证。</p></body></html>
'@
$normalFour = @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>第四章 草稿恢复</title><link rel="stylesheet" href="../styles/book.css"/></head><body><h1 id="four">第四章 草稿恢复</h1><p>第四章触发三章缓存上限。</p></body></html>
'@
$normalPath = Join-Path $OutputDirectory 'stage4b-normal.epub'
New-EpubFixture -Path $normalPath -Entries @(
    @{ Name = 'META-INF/container.xml'; Content = $container },
    @{ Name = 'EPUB/package.opf'; Content = $normalOpf },
    @{ Name = 'EPUB/nav.xhtml'; Content = $normalNav },
    @{ Name = 'EPUB/styles/book.css'; Content = 'body { color:#25324a; background:#fbfaf7; line-height:1.8 } h1 { color:#2359b8 }' },
    @{ Name = 'EPUB/images/original.png'; Content = $pictureBytes },
    @{ Name = 'EPUB/text/one.xhtml'; Content = $normalOne },
    @{ Name = 'EPUB/text/two.xhtml'; Content = $normalTwo },
    @{ Name = 'EPUB/text/three.xhtml'; Content = $normalThree },
    @{ Name = 'EPUB/text/four.xhtml'; Content = $normalFour }
)

$unsafeOpf = @'
<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">urn:readloom:validation:stage4b:readonly</dc:identifier><dc:title>阶段 4B 只读降级</dc:title><dc:language>zh-CN</dc:language></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>
'@
$unsafeChapter = @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>危险结构</title></head><body><h1>危险结构</h1><script>window.bad=true</script><table><tr><td>表格保持只读</td></tr></table><p onclick="alert(1)">不能静默改写。</p></body></html>
'@
$unsafePath = Join-Path $OutputDirectory 'stage4b-read-only.epub'
New-EpubFixture -Path $unsafePath -Entries @(
    @{ Name = 'META-INF/container.xml'; Content = $container },
    @{ Name = 'EPUB/package.opf'; Content = $unsafeOpf },
    @{ Name = 'EPUB/chapter.xhtml'; Content = $unsafeChapter }
)

$largeText = '大章节正文：中文 emoji 😀 é。' * 40000
$largeOpf = @'
<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">urn:readloom:validation:stage4b:large</dc:identifier><dc:title>阶段 4B 大章节</dc:title><dc:language>zh-CN</dc:language></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>
'@
$largePath = Join-Path $OutputDirectory 'stage4b-large.epub'
New-EpubFixture -Path $largePath -Entries @(
    @{ Name = 'META-INF/container.xml'; Content = $container },
    @{ Name = 'EPUB/package.opf'; Content = $largeOpf },
    @{ Name = 'EPUB/chapter.xhtml'; Content = "<?xml version=`"1.0`" encoding=`"UTF-8`"?><html xmlns=`"http://www.w3.org/1999/xhtml`"><head><title>大章节</title></head><body><h1>大章节</h1><p>$largeText</p></body></html>" }
)

@($normalPath, $unsafePath, $largePath, $picturePath) |
    ForEach-Object { Get-Item -LiteralPath $_ | Select-Object FullName, Length }
