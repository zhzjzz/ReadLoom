use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use tempfile::TempDir;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

pub(crate) struct EpubFixture {
    _directory: TempDir,
    path: PathBuf,
}

impl EpubFixture {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) fn minimal_epub3() -> EpubFixture {
    epub3_with_version("3.0")
}

pub(crate) fn minimal_epub3_two_chapters() -> EpubFixture {
    write_epub(&[
        (
            "mimetype",
            "application/epub+zip",
            CompressionMethod::Stored,
        ),
        (
            "META-INF/container.xml",
            r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            CompressionMethod::Deflated,
        ),
        (
            "EPUB/package.opf",
            r#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="pub-id">urn:readloom:test:two</dc:identifier><dc:title>两章测试</dc:title><dc:creator>测试作者</dc:creator><dc:language>zh-CN</dc:language><meta property="dcterms:modified">2026-08-08T00:00:00Z</meta></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#,
            CompressionMethod::Deflated,
        ),
        (
            "EPUB/nav.xhtml",
            r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>目录</title></head><body><nav epub:type="toc"><ol><li><a href="one.xhtml">第一章</a></li><li><a href="two.xhtml">第二章</a></li></ol></nav></body></html>"#,
            CompressionMethod::Deflated,
        ),
        (
            "EPUB/one.xhtml",
            r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>第一章</title></head><body><h1>第一章</h1><p>原始甲。</p></body></html>"#,
            CompressionMethod::Deflated,
        ),
        (
            "EPUB/two.xhtml",
            r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>第二章</title></head><body><h1>第二章</h1><p>原始乙。</p></body></html>"#,
            CompressionMethod::Deflated,
        ),
    ])
}

pub(crate) fn epub3_with_padding(padding_bytes: u64) -> EpubFixture {
    let source = minimal_epub3();
    let directory = tempfile::tempdir().expect("temporary large EPUB fixture directory");
    let path = directory.path().join("large-fixture.epub");
    let mut source_archive =
        zip::ZipArchive::new(File::open(source.path()).expect("open base EPUB fixture"))
            .expect("read base EPUB fixture");
    let mut writer = ZipWriter::new(File::create(&path).expect("create large EPUB fixture"));
    for index in 0..source_archive.len() {
        let mut entry = source_archive
            .by_index(index)
            .expect("read base EPUB entry");
        writer
            .start_file(
                entry.name(),
                SimpleFileOptions::default().compression_method(entry.compression()),
            )
            .expect("copy base EPUB entry header");
        std::io::copy(&mut entry, &mut writer).expect("copy base EPUB entry");
    }
    writer
        .start_file(
            "EPUB/assets/large-unknown-resource.bin",
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .expect("start large EPUB padding entry");
    std::io::copy(&mut std::io::repeat(0x5a).take(padding_bytes), &mut writer)
        .expect("stream large EPUB padding entry");
    writer.finish().expect("finish large EPUB fixture");
    EpubFixture {
        _directory: directory,
        path,
    }
}

pub(crate) fn unsupported_epub_version() -> EpubFixture {
    epub3_with_version("4.0")
}

pub(crate) fn epub_without_container() -> EpubFixture {
    write_epub(&[(
        "mimetype",
        "application/epub+zip",
        CompressionMethod::Stored,
    )])
}

fn epub3_with_version(version: &str) -> EpubFixture {
    let package = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="{version}" unique-identifier="pub-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="pub-id">urn:readloom:test:epub3</dc:identifier>
    <dc:title>阅织 EPUB 3 测试</dc:title>
    <dc:creator>Readloom 测试作者</dc:creator>
    <dc:language>zh-CN</dc:language>
    <meta property="dcterms:modified">2026-08-08T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="chapter"/></spine>
</package>"#,
    );
    write_epub(&[
        (
            "mimetype",
            "application/epub+zip",
            CompressionMethod::Stored,
        ),
        (
            "META-INF/container.xml",
            r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
            CompressionMethod::Deflated,
        ),
        (
            "EPUB/package.opf",
            package.as_str(),
            CompressionMethod::Deflated,
        ),
        (
            "EPUB/nav.xhtml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>目录</title></head>
<body><nav epub:type="toc"><h1>目录</h1><ol><li><a href="chapter.xhtml#start">第一章</a></li></ol></nav></body></html>"#,
            CompressionMethod::Deflated,
        ),
        (
            "EPUB/chapter.xhtml",
            r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>第一章</title></head><body><h1 id="start">第一章</h1><p>你好，Readloom。</p></body></html>"#,
            CompressionMethod::Deflated,
        ),
    ])
}

pub(crate) fn minimal_epub2() -> EpubFixture {
    write_epub(&[
        (
            "mimetype",
            "application/epub+zip",
            CompressionMethod::Stored,
        ),
        (
            "META-INF/container.xml",
            r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
            CompressionMethod::Deflated,
        ),
        (
            "OEBPS/content.opf",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="book-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="book-id">urn:readloom:test:epub2</dc:identifier>
    <dc:title>阅织 EPUB 2 测试</dc:title>
    <dc:creator>第二版作者</dc:creator>
    <dc:language>zh-CN</dc:language>
    <meta name="cover" content="cover"/>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="cover" href="images/cover.png" media-type="image/png"/>
    <item id="chapter" href="text/chapter.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx"><itemref idref="chapter"/></spine>
</package>"#,
            CompressionMethod::Deflated,
        ),
        (
            "OEBPS/toc.ncx",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head><meta name="dtb:uid" content="urn:readloom:test:epub2"/></head>
  <docTitle><text>阅织 EPUB 2 测试</text></docTitle>
  <navMap><navPoint id="chapter-1" playOrder="1"><navLabel><text>旧版第一章</text></navLabel><content src="text/chapter.xhtml#start"/></navPoint></navMap>
</ncx>"#,
            CompressionMethod::Deflated,
        ),
        (
            "OEBPS/text/chapter.xhtml",
            r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>旧版第一章</title></head><body><h1 id="start">旧版第一章</h1><p>EPUB 2 正文。</p></body></html>"#,
            CompressionMethod::Deflated,
        ),
        (
            "OEBPS/images/cover.png",
            "\u{89}PNG\r\n\u{1a}\nfixture",
            CompressionMethod::Deflated,
        ),
    ])
}

fn write_epub(entries: &[(&str, &str, CompressionMethod)]) -> EpubFixture {
    let directory = tempfile::tempdir().expect("temporary EPUB fixture directory");
    let path = directory.path().join("fixture.epub");
    let file = File::create(&path).expect("create EPUB fixture");
    let mut writer = ZipWriter::new(file);
    for (name, content, compression) in entries {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(*compression),
            )
            .expect("start EPUB entry");
        writer
            .write_all(content.as_bytes())
            .expect("write EPUB entry");
    }
    writer.finish().expect("finish EPUB fixture");
    EpubFixture {
        _directory: directory,
        path,
    }
}
