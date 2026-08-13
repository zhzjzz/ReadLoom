use std::{
    fs::File,
    io::{Read, Write},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use quick_xml::{
    Reader, XmlVersion,
    encoding::Decoder,
    escape::{escape, resolve_predefined_entity, unescape},
    events::{BytesRef, BytesStart, Event},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    BlockId, ChapterKey, CoreError, EditError, EditableBlock, EpubDocument, ParagraphKind,
};

#[derive(Debug, Clone)]
struct EpubSourceBlock {
    original_text: String,
    text_spans: Vec<Range<usize>>,
    insertion_offset: usize,
}

#[derive(Debug, Clone)]
pub struct EpubDraft {
    source_path: PathBuf,
    base_fingerprint: String,
    chapter_index: usize,
    resource_path: String,
    source: Arc<str>,
    blocks: Vec<EditableBlock>,
    source_blocks: Vec<EpubSourceBlock>,
}

impl EpubDraft {
    pub fn from_document(document: &EpubDocument) -> Result<Self, CoreError> {
        let resource_path = document
            .active_chapter_resource_path()
            .ok_or_else(|| invalid_epub_edit("EPUB 当前章节资源已经不存在。"))?
            .to_owned();
        let mut archive = ZipArchive::new(File::open(document.path())?)
            .map_err(|_| invalid_epub_edit("无法重新打开 EPUB 容器。"))?;
        let mut entry = archive
            .by_name(&resource_path)
            .map_err(|_| invalid_epub_edit("EPUB 当前章节条目已经不存在。"))?;
        let mut bytes = Vec::with_capacity(entry.size().min(usize::MAX as u64) as usize);
        entry.read_to_end(&mut bytes)?;
        let source = String::from_utf8(bytes).map_err(|_| {
            invalid_epub_edit("当前章节不是 UTF-8 XHTML；为避免破坏原始编码，已设为只读。")
        })?;
        let (blocks, source_blocks) = parse_xhtml_blocks(&resource_path, &source)?;
        if blocks.iter().all(|block| !block.editable) {
            return Err(invalid_epub_edit("当前章节没有受支持的正文文本块。"));
        }
        Ok(Self {
            source_path: document.path().to_owned(),
            base_fingerprint: document.fingerprint().to_owned(),
            chapter_index: document.active_chapter_index(),
            resource_path,
            source: Arc::from(source),
            blocks,
            source_blocks,
        })
    }

    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub fn base_fingerprint(&self) -> &str {
        &self.base_fingerprint
    }

    pub fn resource_path(&self) -> &str {
        &self.resource_path
    }

    pub(crate) fn rebase(&mut self, source_path: PathBuf, base_fingerprint: String) {
        self.source_path = source_path;
        self.base_fingerprint = base_fingerprint;
    }

    pub fn blocks(&self) -> &[EditableBlock] {
        &self.blocks
    }

    pub fn replace_block_text(&mut self, id: &BlockId, text: String) -> Result<String, EditError> {
        let block = self
            .blocks
            .iter_mut()
            .find(|block| block.id == *id)
            .ok_or(EditError::MissingBlock)?;
        if !block.editable {
            return Err(EditError::ReadOnlyBlock);
        }
        Ok(std::mem::replace(&mut block.text, text))
    }

    pub fn render_xhtml(&self) -> String {
        struct Replacement<'a> {
            range: Range<usize>,
            text: &'a str,
            escaped: Option<String>,
        }

        let mut replacements = Vec::new();
        for (block, source) in self.blocks.iter().zip(&self.source_blocks) {
            if !block.editable || block.text == source.original_text {
                continue;
            }
            let escaped = escape(&block.text).into_owned();
            if let Some(first) = source.text_spans.first() {
                replacements.push(Replacement {
                    range: first.clone(),
                    text: "",
                    escaped: Some(escaped),
                });
                for span in source.text_spans.iter().skip(1) {
                    replacements.push(Replacement {
                        range: span.clone(),
                        text: "",
                        escaped: None,
                    });
                }
            } else {
                replacements.push(Replacement {
                    range: source.insertion_offset..source.insertion_offset,
                    text: "",
                    escaped: Some(escaped),
                });
            }
        }
        if replacements.is_empty() {
            return self.source.to_string();
        }
        replacements.sort_by_key(|replacement| replacement.range.start);
        let mut output = String::with_capacity(self.source.len());
        let mut cursor = 0usize;
        for replacement in replacements {
            debug_assert!(replacement.range.start >= cursor);
            output.push_str(&self.source[cursor..replacement.range.start]);
            if let Some(escaped) = replacement.escaped {
                output.push_str(&escaped);
            } else {
                output.push_str(replacement.text);
            }
            cursor = replacement.range.end;
        }
        output.push_str(&self.source[cursor..]);
        output
    }
}

pub(crate) fn save_epub_draft(
    draft: &EpubDraft,
    requested_target: &Path,
) -> Result<EpubDocument, CoreError> {
    if requested_target
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("epub"))
    {
        return Err(invalid_epub_edit("EPUB 另存为路径必须使用 .epub 扩展名。"));
    }
    let parent = requested_target
        .parent()
        .ok_or_else(|| invalid_epub_edit("EPUB 保存路径没有有效的父目录。"))?;
    let parent = std::fs::canonicalize(parent)?;
    let file_name = requested_target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid_epub_edit("EPUB 保存文件名无效。"))?;
    let target = parent.join(file_name);
    let replacing_source = target == draft.source_path;
    let source_changed =
        super::epub::fingerprint_file(&draft.source_path)? != draft.base_fingerprint;
    if replacing_source && source_changed {
        return Err(invalid_epub_edit(
            "EPUB 已被其他程序修改；为避免覆盖，Readloom 已取消保存。",
        ));
    }
    super::epub::validate_archive(&draft.source_path)?;
    let expected_target = if replacing_source {
        Some(draft.base_fingerprint.clone())
    } else if target.exists() {
        Some(super::epub::fingerprint_file(&target)?)
    } else {
        None
    };
    let (temporary, temporary_path) =
        super::core::create_save_artifact(&parent, file_name, "epub")?;
    if let Err(error) = write_repacked_epub(draft, temporary) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error);
    }
    if let Err(error) = validate_repacked_epub(draft, &temporary_path) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error);
    }
    if replacing_source
        && super::epub::fingerprint_file(&draft.source_path)? != draft.base_fingerprint
    {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(invalid_epub_edit(
            "EPUB 在保存过程中被其他程序修改，已取消覆盖。",
        ));
    }
    super::core::install_validated_file(
        &target,
        &temporary_path,
        expected_target.as_deref(),
        "EPUB",
    )?;
    let reopened = EpubDocument::open(&target)?;
    reopened.load_chapter(draft.chapter_index)?;
    Ok(reopened)
}

fn write_repacked_epub(draft: &EpubDraft, temporary: File) -> Result<(), CoreError> {
    let mut source = ZipArchive::new(File::open(&draft.source_path)?)
        .map_err(|_| invalid_epub_edit("无法重新打开 EPUB 容器。"))?;
    let mut writer = ZipWriter::new(temporary);
    let rendered = draft.render_xhtml();
    let mut replaced = false;
    for index in 0..source.len() {
        let entry = source
            .by_index(index)
            .map_err(|_| invalid_epub_edit("无法读取 EPUB 容器条目。"))?;
        if entry.name() == draft.resource_path {
            let name = entry.name().to_owned();
            let compression = entry.compression();
            let modified = entry.last_modified();
            let permissions = entry.unix_mode();
            drop(entry);
            let mut options = SimpleFileOptions::default().compression_method(compression);
            if let Some(modified) = modified {
                options = options.last_modified_time(modified);
            }
            if let Some(permissions) = permissions {
                options = options.unix_permissions(permissions);
            }
            writer
                .start_file(name, options)
                .map_err(|_| invalid_epub_edit("无法写入修改后的 EPUB 章节。"))?;
            writer.write_all(rendered.as_bytes())?;
            replaced = true;
        } else {
            writer
                .raw_copy_file(entry)
                .map_err(|_| invalid_epub_edit("无法保真复制 EPUB 原始条目。"))?;
        }
    }
    if !replaced {
        return Err(invalid_epub_edit("EPUB 当前章节条目已经不存在。"));
    }
    let output = writer
        .finish()
        .map_err(|_| invalid_epub_edit("无法完成 EPUB 重打包。"))?;
    output.sync_all()?;
    Ok(())
}

fn validate_repacked_epub(draft: &EpubDraft, path: &Path) -> Result<(), CoreError> {
    super::epub::validate_archive(path)?;
    let reopened = EpubDocument::open(path)?;
    reopened.load_chapter(draft.chapter_index)?;
    let expected = draft
        .blocks
        .iter()
        .filter(|block| block.editable)
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>();
    let paragraphs = reopened.paragraphs();
    let actual = paragraphs
        .iter()
        .filter(|paragraph| paragraph.editable)
        .map(|paragraph| paragraph.text.as_str())
        .collect::<Vec<_>>();
    if expected != actual {
        return Err(invalid_epub_edit(
            "重打包后的 EPUB 正文校验失败，原文件保持不变。",
        ));
    }
    Ok(())
}

fn parse_xhtml_blocks(
    resource_path: &str,
    source: &str,
) -> Result<(Vec<EditableBlock>, Vec<EpubSourceBlock>), CoreError> {
    struct CurrentBlock {
        element_name: Vec<u8>,
        kind: ParagraphKind,
        text: String,
        text_spans: Vec<Range<usize>>,
    }

    let mut reader = Reader::from_str(source);
    reader.config_mut().check_end_names = true;
    let mut blocks = Vec::new();
    let mut source_blocks = Vec::new();
    let mut current: Option<CurrentBlock> = None;
    let mut hidden_depth = 0usize;
    loop {
        let event_start = reader.buffer_position() as usize;
        let event = reader
            .read_event()
            .map_err(|_| invalid_epub_edit("EPUB 章节 XHTML 结构无效。"))?;
        let event_end = reader.buffer_position() as usize;
        match event {
            Event::Start(element) => {
                let qualified_name = element.name();
                let name = local_name(qualified_name.as_ref());
                if hidden_depth > 0 || is_hidden(name) {
                    hidden_depth = hidden_depth.saturating_add(1);
                } else if name == b"img" {
                    push_image_block(
                        resource_path,
                        event_start,
                        &element,
                        reader.decoder(),
                        &mut blocks,
                        &mut source_blocks,
                    )?;
                } else if current.is_none()
                    && let Some(kind) = block_kind(name)
                {
                    current = Some(CurrentBlock {
                        element_name: name.to_vec(),
                        kind,
                        text: String::new(),
                        text_spans: Vec::new(),
                    });
                }
            }
            Event::End(_) if hidden_depth > 0 => hidden_depth -= 1,
            Event::End(element) => {
                let qualified_name = element.name();
                let name = local_name(qualified_name.as_ref());
                if current
                    .as_ref()
                    .is_some_and(|block| block.element_name.as_slice() == name)
                {
                    let block = current.take().expect("checked current EPUB block");
                    let text = collapse_whitespace(&block.text);
                    blocks.push(EditableBlock {
                        id: BlockId::epub(resource_path, blocks.len()),
                        chapter_key: ChapterKey::new(resource_path),
                        kind: block.kind,
                        text: text.clone(),
                        editable: true,
                    });
                    source_blocks.push(EpubSourceBlock {
                        original_text: text,
                        text_spans: block.text_spans,
                        insertion_offset: event_start,
                    });
                }
            }
            Event::Empty(element) if hidden_depth == 0 => {
                let qualified_name = element.name();
                let name = local_name(qualified_name.as_ref());
                if name == b"img" {
                    push_image_block(
                        resource_path,
                        event_start,
                        &element,
                        reader.decoder(),
                        &mut blocks,
                        &mut source_blocks,
                    )?;
                } else if name == b"br"
                    && let Some(block) = current.as_mut()
                {
                    block.text.push(' ');
                }
            }
            Event::Text(value) if hidden_depth == 0 => {
                if let Some(block) = current.as_mut() {
                    let decoded = value
                        .xml_content(XmlVersion::Implicit1_0)
                        .map_err(|_| invalid_epub_edit("EPUB 章节包含无法解码的文本。"))?;
                    let decoded = unescape(&decoded)
                        .map_err(|_| invalid_epub_edit("EPUB 章节包含无效的字符实体。"))?;
                    block.text.push_str(&decoded);
                    block.text_spans.push(event_start..event_end);
                }
            }
            Event::CData(value) if hidden_depth == 0 => {
                if let Some(block) = current.as_mut() {
                    let decoded = value
                        .decode()
                        .map_err(|_| invalid_epub_edit("EPUB 章节包含无法解码的文本。"))?;
                    block.text.push_str(&decoded);
                    block.text_spans.push(event_start..event_end);
                }
            }
            Event::GeneralRef(reference) if hidden_depth == 0 => {
                if let Some(block) = current.as_mut() {
                    block.text.push_str(&resolve_reference(&reference)?);
                    block.text_spans.push(event_start..event_end);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok((blocks, source_blocks))
}

fn push_image_block(
    resource_path: &str,
    source_offset: usize,
    element: &BytesStart<'_>,
    decoder: Decoder,
    blocks: &mut Vec<EditableBlock>,
    source_blocks: &mut Vec<EpubSourceBlock>,
) -> Result<(), CoreError> {
    let mut alt = None;
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(|_| invalid_epub_edit("EPUB 图片属性无效。"))?;
        if local_name(attribute.key.as_ref()) == b"alt" {
            alt = Some(
                attribute
                    .decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)
                    .map_err(|_| invalid_epub_edit("EPUB 图片属性无法解码。"))?
                    .into_owned(),
            );
        }
    }
    let text = alt
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "图片".to_owned());
    blocks.push(EditableBlock {
        id: BlockId::epub(resource_path, blocks.len()),
        chapter_key: ChapterKey::new(resource_path),
        kind: ParagraphKind::Image,
        text: text.clone(),
        editable: false,
    });
    source_blocks.push(EpubSourceBlock {
        original_text: text,
        text_spans: Vec::new(),
        insertion_offset: source_offset,
    });
    Ok(())
}

fn resolve_reference(reference: &BytesRef<'_>) -> Result<String, CoreError> {
    if let Some(character) = reference
        .resolve_char_ref()
        .map_err(|_| invalid_epub_edit("EPUB 章节包含无效的字符引用。"))?
    {
        return Ok(character.to_string());
    }
    let name = reference
        .decode()
        .map_err(|_| invalid_epub_edit("EPUB 章节包含无法解码的字符引用。"))?;
    resolve_predefined_entity(&name)
        .map(str::to_owned)
        .ok_or_else(|| invalid_epub_edit("EPUB 章节包含未知字符实体。"))
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or_default()
}

fn is_hidden(name: &[u8]) -> bool {
    matches!(
        name,
        b"script" | b"style" | b"iframe" | b"object" | b"embed" | b"template" | b"svg"
    )
}

fn block_kind(name: &[u8]) -> Option<ParagraphKind> {
    match name {
        b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => Some(ParagraphKind::Heading),
        b"p" | b"li" | b"blockquote" | b"pre" | b"dd" | b"dt" => Some(ParagraphKind::Paragraph),
        _ => None,
    }
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn invalid_epub_edit(message: &str) -> CoreError {
    CoreError::Validation(message.to_owned())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::TempDir;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use crate::ReadloomCore;

    use super::*;

    #[test]
    fn xhtml_edits_escape_text_and_keep_inline_elements_and_attributes() {
        let source = r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p class="lead">开头 <em data-x="1">强调</em> 结尾 &amp;</p><img src="cover.png" alt="插图"/></body></html>"#;
        let (blocks, source_blocks) =
            parse_xhtml_blocks("EPUB/one.xhtml", source).expect("parse editable XHTML blocks");
        let mut draft = EpubDraft {
            source_path: PathBuf::from("book.epub"),
            base_fingerprint: "base".to_owned(),
            chapter_index: 0,
            resource_path: "EPUB/one.xhtml".to_owned(),
            source: Arc::from(source),
            blocks,
            source_blocks,
        };
        let paragraph = draft
            .blocks()
            .iter()
            .find(|block| block.editable)
            .expect("editable paragraph")
            .id
            .clone();

        draft
            .replace_block_text(&paragraph, "中文 <安全> & 实体".to_owned())
            .expect("edit XHTML paragraph");
        let rendered = draft.render_xhtml();

        assert!(rendered.contains("<p class=\"lead\">"));
        assert!(rendered.contains("<em data-x=\"1\"></em>"));
        assert!(rendered.contains("中文 &lt;安全&gt; &amp; 实体"));
        assert!(rendered.contains("<img src=\"cover.png\" alt=\"插图\"/>"));
        assert!(!rendered.contains("中文 <安全>"));
    }

    #[test]
    fn saving_edits_one_chapter_and_preserves_the_publication_archive() {
        let (directory, path) = publication_fixture();
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3"))
            .expect("open Readloom core");
        let document = core.open_epub(&path).expect("open publication fixture");
        let mut draft = EpubDraft::from_document(&document).expect("create EPUB draft");
        let target = draft
            .blocks()
            .iter()
            .find(|block| block.text == "原始正文 & 实体")
            .expect("editable fixture paragraph")
            .id
            .clone();
        draft
            .replace_block_text(&target, "修改后的中文 <安全> & 正文".to_owned())
            .expect("edit EPUB text");

        let saved = core.save_epub_draft(&draft).expect("save EPUB draft");

        assert!(
            saved
                .paragraphs()
                .iter()
                .any(|paragraph| paragraph.text == "修改后的中文 <安全> & 正文")
        );
        assert_eq!(saved.images().len(), 1);
        let mut archive = ZipArchive::new(File::open(&path).expect("open saved EPUB"))
            .expect("read saved EPUB archive");
        let first = archive.by_index(0).expect("first EPUB entry");
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
        drop(first);
        for name in [
            "EPUB/nav.xhtml",
            "EPUB/toc.ncx",
            "EPUB/style.css",
            "EPUB/font.woff",
            "EPUB/cover.png",
            "EPUB/unknown/readloom.bin",
        ] {
            assert!(
                archive.by_name(name).is_ok(),
                "missing preserved entry {name}"
            );
        }
        let mut unknown = Vec::new();
        archive
            .by_name("EPUB/unknown/readloom.bin")
            .expect("unknown extension entry")
            .read_to_end(&mut unknown)
            .expect("read unknown entry");
        assert_eq!(unknown, b"opaque-extension-payload");
        let mut chapter = String::new();
        archive
            .by_name("EPUB/chapter.xhtml")
            .expect("saved chapter")
            .read_to_string(&mut chapter)
            .expect("read saved chapter");
        assert!(chapter.contains("修改后的中文 &lt;安全&gt; &amp; 正文"));
        assert!(chapter.contains("<img src=\"cover.png\" alt=\"正文插图\"/>"));
    }

    #[test]
    fn epub_external_modification_conflict_preserves_external_bytes() {
        let (directory, path) = publication_fixture();
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3"))
            .expect("open Readloom core");
        let document = core.open_epub(&path).expect("open publication fixture");
        let mut draft = EpubDraft::from_document(&document).expect("create EPUB draft");
        let target = draft.blocks()[1].id.clone();
        draft
            .replace_block_text(&target, "草稿正文".to_owned())
            .expect("edit EPUB draft");
        let mut external = std::fs::read(&path).expect("read original EPUB");
        external.extend_from_slice(b"external-zip-comment-simulation");
        std::fs::write(&path, &external).expect("write external EPUB change");

        let error = core
            .save_epub_draft(&draft)
            .expect_err("external EPUB change must conflict");

        assert!(error.to_string().contains("其他程序修改"));
        assert_eq!(std::fs::read(&path).expect("read external EPUB"), external);
    }

    #[test]
    fn conflicted_epub_draft_can_be_saved_as_without_overwriting_the_external_file() {
        let (directory, path) = publication_fixture();
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3"))
            .expect("open Readloom core");
        let document = core.open_epub(&path).expect("open publication fixture");
        let mut draft = EpubDraft::from_document(&document).expect("create EPUB draft");
        let target_block = draft.blocks()[1].id.clone();
        draft
            .replace_block_text(&target_block, "冲突后的另存正文".to_owned())
            .expect("edit EPUB draft");
        let external = std::fs::read(&path).expect("read original EPUB");
        let mut externally_changed = external.clone();
        externally_changed.extend_from_slice(b"external-change");
        std::fs::write(&path, &externally_changed).expect("write external EPUB change");
        let saved_as = directory.path().join("conflict-copy.epub");

        let saved = core
            .save_epub_draft_as(&draft, &saved_as)
            .expect("save conflicted EPUB as a new publication");

        assert_eq!(
            std::fs::read(&path).expect("read external original"),
            externally_changed
        );
        assert!(
            saved
                .paragraphs()
                .iter()
                .any(|paragraph| paragraph.text == "冲突后的另存正文")
        );
    }

    #[test]
    fn readonly_epub_save_failure_does_not_damage_the_original() {
        let (directory, path) = publication_fixture();
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3"))
            .expect("open Readloom core");
        let document = core.open_epub(&path).expect("open publication fixture");
        let mut draft = EpubDraft::from_document(&document).expect("create EPUB draft");
        let target = draft.blocks()[1].id.clone();
        draft
            .replace_block_text(&target, "只读目标草稿".to_owned())
            .expect("edit EPUB draft");
        let original = std::fs::read(&path).expect("read original EPUB");
        let original_permissions = std::fs::metadata(&path)
            .expect("EPUB metadata")
            .permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        std::fs::set_permissions(&path, readonly_permissions).expect("make EPUB read-only");

        let result = core.save_epub_draft(&draft);

        std::fs::set_permissions(&path, original_permissions).expect("restore EPUB permissions");
        let error = result.expect_err("read-only EPUB must not be replaced");
        assert!(error.to_string().contains("只读"));
        assert_eq!(std::fs::read(&path).expect("read preserved EPUB"), original);
    }

    #[test]
    fn post_write_validation_failure_rolls_back_before_replacing_the_epub() {
        let (directory, path) = publication_fixture();
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3"))
            .expect("open Readloom core");
        let document = core.open_epub(&path).expect("open publication fixture");
        let mut draft = EpubDraft::from_document(&document).expect("create EPUB draft");
        let original = std::fs::read(&path).expect("read original EPUB");
        draft.source = Arc::from("<invalid-xhtml>");

        let error = core
            .save_epub_draft(&draft)
            .expect_err("invalid repacked EPUB must fail validation");

        assert!(
            error.to_string().contains("XHTML")
                || error.to_string().contains("正文")
                || error.to_string().contains("EPUB")
        );
        assert_eq!(std::fs::read(&path).expect("read preserved EPUB"), original);
    }

    fn publication_fixture() -> (TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("publication.epub");
        let mut writer = ZipWriter::new(File::create(&path).expect("create EPUB fixture"));
        let entries = [
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
                r#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">edit-fixture</dc:identifier><dc:title>保真编辑测试</dc:title><dc:creator>测试作者</dc:creator><dc:language>zh-CN</dc:language><meta property="dcterms:modified">2026-08-13T00:00:00Z</meta></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="css" href="style.css" media-type="text/css"/><item id="font" href="font.woff" media-type="font/woff"/><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest><spine toc="ncx"><itemref idref="chapter"/></spine></package>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/nav.xhtml",
                r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><nav epub:type="toc" xmlns:epub="http://www.idpf.org/2007/ops"><ol><li><a href="chapter.xhtml">第一章</a></li></ol></nav></body></html>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/toc.ncx",
                r#"<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><head/><docTitle><text>保真编辑测试</text></docTitle><navMap><navPoint id="one" playOrder="1"><navLabel><text>第一章</text></navLabel><content src="chapter.xhtml"/></navPoint></navMap></ncx>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/chapter.xhtml",
                r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="style.css"/></head><body><h1>第一章</h1><p class="lead">原始正文 &amp; 实体</p><img src="cover.png" alt="正文插图"/></body></html>"#,
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/style.css",
                "@font-face{font-family:Fixture;src:url(font.woff)} p{color:#123456}",
                CompressionMethod::Deflated,
            ),
            (
                "EPUB/unknown/readloom.bin",
                "opaque-extension-payload",
                CompressionMethod::Deflated,
            ),
        ];
        for (name, content, compression) in entries {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(compression),
                )
                .expect("start EPUB fixture entry");
            writer
                .write_all(content.as_bytes())
                .expect("write EPUB fixture entry");
        }
        writer
            .start_file(
                "EPUB/font.woff",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .expect("start font entry");
        writer.write_all(b"wOFFfixture-font").expect("write font");
        writer
            .start_file(
                "EPUB/cover.png",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("start image entry");
        writer
            .write_all(&[
                137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0,
                1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 8, 215, 99, 248,
                207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68,
                174, 66, 96, 130,
            ])
            .expect("write image");
        writer.finish().expect("finish EPUB fixture");
        (directory, path)
    }
}
