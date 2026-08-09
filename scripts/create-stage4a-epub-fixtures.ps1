param(
    [string]$OutputDirectory = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\validation')
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

$pngCoverPath = Join-Path $OutputDirectory 'stage4a-replacement.png'
$jpegCoverPath = Join-Path $OutputDirectory 'stage4a-replacement.jpg'
$bitmap = [System.Drawing.Bitmap]::new(120, 180)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.Clear([System.Drawing.Color]::FromArgb(34, 89, 184))
    $graphics.FillRectangle([System.Drawing.Brushes]::Gold, 18, 24, 84, 132)
    $bitmap.Save($pngCoverPath, [System.Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Save($jpegCoverPath, [System.Drawing.Imaging.ImageFormat]::Jpeg)
}
finally {
    $graphics.Dispose()
    $bitmap.Dispose()
}

$storedMimetypeArchive = [Convert]::FromBase64String(
    'UEsDBBQAAAAAAAAACF1vYassFAAAABQAAAAIAAAAbWltZXR5cGVhcHBsaWNhdGlvbi9lcHViK3ppcFBLAQIUABQAAAAAAAAACF1vYassFAAAABQAAAAIAAAAAAAAAAAAAACAAQAAAABtaW1ldHlwZVBLBQYAAAAAAQABADYAAAA6AAAAAAA='
)
$validationFontPath = (& py -3 -c "import matplotlib, pathlib; print(pathlib.Path(matplotlib.get_data_path()) / 'fonts' / 'ttf' / 'DejaVuSans.ttf')").Trim()
if ($LASTEXITCODE -ne 0) {
    throw 'Python could not locate the bundled DejaVu Sans validation font.'
}
if (-not (Test-Path -LiteralPath $validationFontPath -PathType Leaf)) {
    throw "Validation font not found: $validationFontPath"
}
$validationFontBytes = [System.IO.File]::ReadAllBytes($validationFontPath)

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
                        1024,
                        $true
                    )
                    try {
                        $writer.Write([string]$item.Content)
                    }
                    finally {
                        $writer.Dispose()
                    }
                }
            }
            finally {
                $entryStream.Dispose()
            }
        }
    }
    finally {
        $archive.Dispose()
        $stream.Dispose()
    }
}

$epub2Path = Join-Path $OutputDirectory 'stage4a-epub2.epub'
New-EpubFixture -Path $epub2Path -Entries @(
    @{ Name = 'META-INF/container.xml'; Content = @'
<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>
'@ },
    @{ Name = 'OEBPS/content.opf'; Content = @'
<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="book-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="book-id">urn:readloom:validation:stage4a:epub2</dc:identifier>
    <dc:title>阶段 4A EPUB 2 输入</dc:title>
    <dc:creator>EPUB2 原作者</dc:creator>
    <dc:language>zh-CN</dc:language>
    <meta name="cover" content="old-cover"/>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="style" href="styles/book.css" media-type="text/css"/>
    <item id="font" href="fonts/validation.ttf" media-type="font/ttf"/>
    <item id="old-cover" href="images/original-cover.png" media-type="image/png"/>
    <item id="chapter" href="text/chapter.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx"><itemref idref="chapter"/></spine>
</package>
'@ },
    @{ Name = 'OEBPS/toc.ncx'; Content = @'
<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head><meta name="dtb:uid" content="urn:readloom:validation:stage4a:epub2"/></head>
  <docTitle><text>阶段 4A EPUB 2 输入</text></docTitle>
  <navMap><navPoint id="chapter" playOrder="1"><navLabel><text>EPUB 2 章节</text></navLabel><content src="text/chapter.xhtml#start"/></navPoint></navMap>
</ncx>
'@ },
    @{ Name = 'OEBPS/styles/book.css'; Content = '@font-face { font-family: ReadloomValidation; src: url("../fonts/validation.ttf"); } body { color: #25324a; background: #fbfaf7; font-family: ReadloomValidation, sans-serif; } h1 { color: #2359b8; }' },
    @{ Name = 'OEBPS/text/chapter.xhtml'; Content = @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>EPUB 2 章节</title><link rel="stylesheet" href="../styles/book.css"/></head>
<body><h1 id="start">EPUB 2 章节</h1><img src="../images/original-cover.png" alt="原封面"/><p>EPUB 2 Stage 4A chapter readable.</p><span style="font-family: ReadloomValidation">Internal font probe.</span></body></html>
'@ },
    @{ Name = 'OEBPS/images/original-cover.png'; Content = [System.IO.File]::ReadAllBytes($pngCoverPath) },
    @{ Name = 'OEBPS/fonts/validation.ttf'; Content = $validationFontBytes }
)

$noCoverPath = Join-Path $OutputDirectory 'stage4a-no-cover.epub'
New-EpubFixture -Path $noCoverPath -Entries @(
    @{ Name = 'META-INF/container.xml'; Content = @'
<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>
'@ },
    @{ Name = 'EPUB/package.opf'; Content = @'
<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="pub-id">urn:readloom:validation:stage4a:no-cover</dc:identifier>
    <dc:title>阶段 4A 无封面 EPUB</dc:title>
    <dc:creator>Readloom</dc:creator>
    <dc:language>zh-CN</dc:language>
    <meta property="dcterms:modified">2026-08-08T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="style" href="styles/book.css" media-type="text/css"/>
    <item id="chapter" href="text/chapter.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="chapter"/></spine>
</package>
'@ },
    @{ Name = 'EPUB/nav.xhtml'; Content = @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="text/chapter.xhtml#start">无封面章节</a></li></ol></nav></body></html>
'@ },
    @{ Name = 'EPUB/styles/book.css'; Content = 'body { color: #25324a; background: #fbfaf7; } h1 { color: #2359b8; }' },
    @{ Name = 'EPUB/text/chapter.xhtml'; Content = @'
<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>无封面章节</title><link rel="stylesheet" href="../styles/book.css"/></head>
<body><h1 id="start">无封面章节</h1><p>EPUB 3 no-cover Stage 4A chapter readable.</p></body></html>
'@ }
)

@($epub2Path, $noCoverPath, $pngCoverPath, $jpegCoverPath) |
    ForEach-Object { Get-Item -LiteralPath $_ | Select-Object FullName, Length }
