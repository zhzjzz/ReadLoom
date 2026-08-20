use std::{
    collections::BTreeSet,
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
    BlockId, ChapterKey, CoreError, EditError, EditableBlock, EpubDocument, InsertSide,
    JoinDirection, ParagraphKind, ValidatedImageAsset,
};

#[derive(Debug, Clone)]
enum EpubNodeSource {
    Text {
        original_text: String,
        element_span: Range<usize>,
        text_spans: Vec<Range<usize>>,
        insertion_offset: usize,
        structural_template: Option<EpubTextElementTemplate>,
    },
    StructuredText {
        group_id: u64,
    },
    Image {
        origin: EpubImageOrigin,
        src: String,
        resolved_resource_path: String,
        original_alt: String,
    },
}

#[derive(Debug, Clone)]
enum EpubImageOrigin {
    Existing {
        element_span: Range<usize>,
        removable: bool,
    },
    Inserted {
        insertion_offset: usize,
        archive_path: String,
        asset: ValidatedImageAsset,
    },
}

impl EpubNodeSource {
    fn element_span(&self) -> Option<Range<usize>> {
        match self {
            Self::Text { element_span, .. }
            | Self::Image {
                origin: EpubImageOrigin::Existing { element_span, .. },
                ..
            } => Some(element_span.clone()),
            Self::StructuredText { .. }
            | Self::Image {
                origin: EpubImageOrigin::Inserted { .. },
                ..
            } => None,
        }
    }
}

#[derive(Debug, Clone)]
struct EpubTextElementTemplate {
    tag_name: String,
    attributes: Vec<EpubTextAttribute>,
}

#[derive(Debug, Clone)]
struct EpubTextAttribute {
    name: String,
    value: String,
    unique: bool,
}

impl EpubTextElementTemplate {
    fn sibling(&self) -> Self {
        let tag_name = if matches!(
            self.tag_name.as_str(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        ) {
            "p".to_owned()
        } else {
            self.tag_name.clone()
        };
        Self {
            tag_name,
            attributes: self
                .attributes
                .iter()
                .filter(|attribute| !attribute.unique)
                .cloned()
                .collect(),
        }
    }

    fn supports_join(&self) -> bool {
        matches!(self.tag_name.as_str(), "p" | "li")
    }

    fn render(&self, text: &str) -> String {
        let mut output = format!("<{}", self.tag_name);
        for attribute in &self.attributes {
            output.push(' ');
            output.push_str(&attribute.name);
            output.push_str("=\"");
            output.push_str(&escape(&attribute.value));
            output.push('"');
        }
        output.push('>');
        output.push_str(&escape(text));
        output.push_str("</");
        output.push_str(&self.tag_name);
        output.push('>');
        output
    }
}

#[derive(Debug, Clone)]
struct EpubTextReplacementGroup {
    id: u64,
    source_span: Range<usize>,
    block_ids: Vec<BlockId>,
    templates: Vec<EpubTextElementTemplate>,
}

#[derive(Debug, Clone)]
pub(crate) struct EpubNodeSnapshot {
    pub(crate) block: EditableBlock,
    source: EpubNodeSource,
}

#[derive(Debug, Clone)]
struct EpubImageAddition {
    archive_path: String,
    asset: ValidatedImageAsset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemovedImage {
    element_span: Option<Range<usize>>,
    archive_path: String,
}

#[derive(Debug, Clone)]
pub struct EpubDraft {
    source_path: PathBuf,
    base_fingerprint: String,
    chapter_index: usize,
    resource_path: String,
    source: Arc<str>,
    blocks: Vec<EditableBlock>,
    node_sources: Vec<EpubNodeSource>,
    removed_source_ranges: Vec<Range<usize>>,
    removed_images: Vec<RemovedImage>,
    reserved_archive_paths: BTreeSet<String>,
    next_block_id: u64,
    structured_text_groups: Vec<EpubTextReplacementGroup>,
    next_text_group_id: u64,
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
        let (blocks, node_sources) = parse_xhtml_blocks(&resource_path, &source)?;
        if blocks.iter().all(|block| !block.editable) {
            return Err(invalid_epub_edit("当前章节没有受支持的正文文本块。"));
        }
        drop(entry);
        let reserved_archive_paths = archive
            .file_names()
            .filter_map(|name| normalize_archive_path(name).ok())
            .collect::<BTreeSet<_>>();
        let next_block_id = blocks.len() as u64;
        Ok(Self {
            source_path: document.path().to_owned(),
            base_fingerprint: document.fingerprint().to_owned(),
            chapter_index: document.active_chapter_index(),
            resource_path,
            source: Arc::from(source),
            blocks,
            node_sources,
            removed_source_ranges: Vec::new(),
            removed_images: Vec::new(),
            reserved_archive_paths,
            next_block_id,
            structured_text_groups: Vec::new(),
            next_text_group_id: 0,
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

    pub(crate) fn split_block_text(
        &mut self,
        id: &BlockId,
        parts: Vec<String>,
    ) -> Result<Vec<BlockId>, EditError> {
        debug_assert!(parts.len() > 1);
        let block_index = self
            .blocks
            .iter()
            .position(|block| block.id == *id)
            .ok_or(EditError::MissingBlock)?;
        if !self.blocks[block_index].editable {
            return Err(EditError::ReadOnlyBlock);
        }
        let group_id = self.ensure_structured_text_group(block_index)?;
        let original = self.blocks[block_index].clone();
        let group_index = self
            .structured_text_groups
            .iter()
            .position(|group| group.id == group_id)
            .expect("created EPUB text group exists");
        let member_index = self.structured_text_groups[group_index]
            .block_ids
            .iter()
            .position(|block_id| block_id == id)
            .expect("EPUB text group contains split block");
        let sibling_template =
            self.structured_text_groups[group_index].templates[member_index].sibling();

        let mut replacement_ids = Vec::with_capacity(parts.len());
        replacement_ids.push(original.id.clone());
        self.blocks[block_index].text = parts[0].clone();
        for (offset, text) in parts.into_iter().enumerate().skip(1) {
            let id = BlockId::epub_draft(&self.resource_path, self.next_block_id);
            self.next_block_id = self.next_block_id.wrapping_add(1).max(1);
            replacement_ids.push(id.clone());
            self.blocks.insert(
                block_index + offset,
                EditableBlock {
                    id,
                    chapter_key: original.chapter_key.clone(),
                    kind: ParagraphKind::Paragraph,
                    text,
                    editable: true,
                },
            );
            self.node_sources.insert(
                block_index + offset,
                EpubNodeSource::StructuredText { group_id },
            );
        }
        let group = &mut self.structured_text_groups[group_index];
        group
            .block_ids
            .splice(member_index..=member_index, replacement_ids.iter().cloned());
        group.templates.splice(
            member_index..=member_index,
            std::iter::once(group.templates[member_index].clone()).chain(std::iter::repeat_n(
                sibling_template,
                replacement_ids.len() - 1,
            )),
        );
        Ok(replacement_ids)
    }

    pub(crate) fn join_adjacent_text(
        &mut self,
        id: &BlockId,
        direction: JoinDirection,
    ) -> Result<(BlockId, usize), EditError> {
        let current_index = self
            .blocks
            .iter()
            .position(|block| block.id == *id)
            .ok_or(EditError::MissingBlock)?;
        let (left_index, right_index) = match direction {
            JoinDirection::Previous => (
                current_index
                    .checked_sub(1)
                    .ok_or(EditError::NoAdjacentTextBlock)?,
                current_index,
            ),
            JoinDirection::Next => (
                current_index,
                current_index
                    .checked_add(1)
                    .filter(|index| *index < self.blocks.len())
                    .ok_or(EditError::NoAdjacentTextBlock)?,
            ),
        };
        let left = self.blocks[left_index].clone();
        let right = self.blocks[right_index].clone();
        if !left.editable || !right.editable {
            return Err(EditError::ReadOnlyBlock);
        }
        if left.chapter_key != right.chapter_key
            || !matches!(left.kind, ParagraphKind::Paragraph | ParagraphKind::Blank)
            || !matches!(right.kind, ParagraphKind::Paragraph | ParagraphKind::Blank)
        {
            return Err(EditError::IncompatibleAdjacentTextBlock);
        }
        let left_template = self.structural_template_for_block(left_index)?;
        let right_template = self.structural_template_for_block(right_index)?;
        if !left_template.supports_join() || left_template.tag_name != right_template.tag_name {
            return Err(EditError::IncompatibleAdjacentTextBlock);
        }
        let left_span = self.structural_source_span(left_index)?;
        let right_span = self.structural_source_span(right_index)?;
        if left_span != right_span
            && (left_span.end > right_span.start
                || !self.source[left_span.end..right_span.start]
                    .trim()
                    .is_empty())
        {
            return Err(EditError::IncompatibleAdjacentTextBlock);
        }

        let left_group_id = self.ensure_structured_text_group(left_index)?;
        let right_group_id = self.ensure_structured_text_group(right_index)?;
        if left_group_id != right_group_id {
            self.merge_structured_text_groups(left_group_id, right_group_id)?;
        }
        let group = self
            .structured_text_groups
            .iter_mut()
            .find(|group| group.id == left_group_id)
            .expect("joined EPUB text group exists");
        let right_member = group
            .block_ids
            .iter()
            .position(|block_id| block_id == &right.id)
            .expect("right EPUB block remains in group");
        group.block_ids.remove(right_member);
        group.templates.remove(right_member);

        let caret_utf16 = left.text.encode_utf16().count();
        let kept_id = left.id.clone();
        self.blocks[left_index].text.push_str(&right.text);
        self.blocks[left_index].kind = ParagraphKind::Paragraph;
        self.blocks.remove(right_index);
        self.node_sources.remove(right_index);
        Ok((kept_id, caret_utf16))
    }

    fn structural_template_for_block(
        &self,
        block_index: usize,
    ) -> Result<EpubTextElementTemplate, EditError> {
        match &self.node_sources[block_index] {
            EpubNodeSource::Text {
                structural_template: Some(template),
                ..
            } => Ok(template.clone()),
            EpubNodeSource::StructuredText { group_id } => {
                let block_id = &self.blocks[block_index].id;
                let group = self
                    .structured_text_groups
                    .iter()
                    .find(|group| group.id == *group_id)
                    .expect("EPUB structured member group exists");
                let member = group
                    .block_ids
                    .iter()
                    .position(|id| id == block_id)
                    .expect("EPUB structured group contains block");
                Ok(group.templates[member].clone())
            }
            _ => Err(EditError::UnsafeStructureEdit),
        }
    }

    fn structural_source_span(&self, block_index: usize) -> Result<Range<usize>, EditError> {
        match &self.node_sources[block_index] {
            EpubNodeSource::Text { element_span, .. } => Ok(element_span.clone()),
            EpubNodeSource::StructuredText { group_id } => self
                .structured_text_groups
                .iter()
                .find(|group| group.id == *group_id)
                .map(|group| group.source_span.clone())
                .ok_or(EditError::UnsafeStructureEdit),
            _ => Err(EditError::UnsafeStructureEdit),
        }
    }

    fn ensure_structured_text_group(&mut self, block_index: usize) -> Result<u64, EditError> {
        match self.node_sources[block_index].clone() {
            EpubNodeSource::StructuredText { group_id } => Ok(group_id),
            EpubNodeSource::Text {
                element_span,
                structural_template: Some(template),
                ..
            } => {
                let group_id = self.next_text_group_id;
                self.next_text_group_id = self.next_text_group_id.wrapping_add(1).max(1);
                self.structured_text_groups.push(EpubTextReplacementGroup {
                    id: group_id,
                    source_span: element_span,
                    block_ids: vec![self.blocks[block_index].id.clone()],
                    templates: vec![template],
                });
                self.node_sources[block_index] = EpubNodeSource::StructuredText { group_id };
                Ok(group_id)
            }
            _ => Err(EditError::UnsafeStructureEdit),
        }
    }

    fn merge_structured_text_groups(
        &mut self,
        left_group_id: u64,
        right_group_id: u64,
    ) -> Result<(), EditError> {
        let left_index = self
            .structured_text_groups
            .iter()
            .position(|group| group.id == left_group_id)
            .ok_or(EditError::UnsafeStructureEdit)?;
        let right_index = self
            .structured_text_groups
            .iter()
            .position(|group| group.id == right_group_id)
            .ok_or(EditError::UnsafeStructureEdit)?;
        if right_index <= left_index {
            return Err(EditError::UnsafeStructureEdit);
        }
        let right = self.structured_text_groups.remove(right_index);
        let left = &mut self.structured_text_groups[left_index];
        left.source_span.end = right.source_span.end;
        left.block_ids.extend(right.block_ids);
        left.templates.extend(right.templates);
        for source in &mut self.node_sources {
            if matches!(source, EpubNodeSource::StructuredText { group_id } if *group_id == right_group_id)
            {
                *source = EpubNodeSource::StructuredText {
                    group_id: left_group_id,
                };
            }
        }
        Ok(())
    }

    pub(crate) fn insert_image(
        &mut self,
        anchor_block_id: &BlockId,
        side: InsertSide,
        asset: ValidatedImageAsset,
        alt_text: String,
    ) -> Result<(usize, EpubNodeSnapshot), EditError> {
        let anchor_index = self
            .blocks
            .iter()
            .position(|block| block.id == *anchor_block_id)
            .ok_or(EditError::MissingBlock)?;
        let anchor_span = self.node_sources[anchor_index]
            .element_span()
            .ok_or(EditError::UnsafeImagePosition)?;
        let insertion_offset = match side {
            InsertSide::Before => anchor_span.start,
            InsertSide::After => anchor_span.end,
        };
        let archive_path = self.reserve_image_archive_path(&asset);
        let src = relative_archive_reference(&self.resource_path, &archive_path)
            .map_err(|_| EditError::UnsafeImagePosition)?;
        let resolved_resource_path = archive_path.clone();
        let id = BlockId::epub_draft(&self.resource_path, self.next_block_id);
        self.next_block_id = self.next_block_id.wrapping_add(1).max(1);
        let snapshot = EpubNodeSnapshot {
            block: EditableBlock {
                id,
                chapter_key: ChapterKey::new(&self.resource_path),
                kind: ParagraphKind::Image,
                text: alt_text.clone(),
                editable: false,
            },
            source: EpubNodeSource::Image {
                origin: EpubImageOrigin::Inserted {
                    insertion_offset,
                    archive_path,
                    asset,
                },
                src,
                resolved_resource_path,
                original_alt: alt_text,
            },
        };
        let insertion_index = match side {
            InsertSide::Before => anchor_index,
            InsertSide::After => anchor_index + 1,
        };
        self.restore_node(insertion_index, snapshot.clone())?;
        Ok((insertion_index, snapshot))
    }

    pub(crate) fn remove_image(
        &mut self,
        block_id: &BlockId,
    ) -> Result<(usize, EpubNodeSnapshot), EditError> {
        let index = self
            .blocks
            .iter()
            .position(|block| block.id == *block_id)
            .ok_or(EditError::MissingBlock)?;
        if self.blocks[index].kind != ParagraphKind::Image {
            return Err(EditError::NotImageBlock);
        }
        if matches!(
            self.node_sources[index],
            EpubNodeSource::Image {
                origin: EpubImageOrigin::Existing {
                    removable: false,
                    ..
                },
                ..
            }
        ) {
            return Err(EditError::UnsafeImagePosition);
        }
        let block = self.blocks.remove(index);
        let source = self.node_sources.remove(index);
        if let EpubNodeSource::Image {
            origin: EpubImageOrigin::Existing { element_span, .. },
            resolved_resource_path,
            ..
        } = &source
        {
            self.removed_source_ranges.push(element_span.clone());
            if !resolved_resource_path.is_empty() {
                self.removed_images.push(RemovedImage {
                    element_span: Some(element_span.clone()),
                    archive_path: resolved_resource_path.clone(),
                });
            }
        } else if let EpubNodeSource::Image {
            origin: EpubImageOrigin::Inserted { archive_path, .. },
            ..
        } = &source
        {
            self.removed_images.push(RemovedImage {
                element_span: None,
                archive_path: archive_path.clone(),
            });
        }
        Ok((index, EpubNodeSnapshot { block, source }))
    }

    pub(crate) fn restore_node(
        &mut self,
        index: usize,
        snapshot: EpubNodeSnapshot,
    ) -> Result<(), EditError> {
        if index > self.blocks.len() {
            return Err(EditError::MissingBlock);
        }
        if let EpubNodeSource::Image {
            origin: EpubImageOrigin::Existing { element_span, .. },
            ..
        } = &snapshot.source
            && let Some(range_index) = self
                .removed_source_ranges
                .iter()
                .position(|range| range == element_span)
        {
            self.removed_source_ranges.remove(range_index);
            if let Some(image_index) = self
                .removed_images
                .iter()
                .position(|image| image.element_span.as_ref() == Some(element_span))
            {
                self.removed_images.remove(image_index);
            }
        } else if let EpubNodeSource::Image {
            origin: EpubImageOrigin::Inserted { archive_path, .. },
            ..
        } = &snapshot.source
            && let Some(image_index) = self
                .removed_images
                .iter()
                .position(|image| image.archive_path == *archive_path)
        {
            self.removed_images.remove(image_index);
        }
        self.blocks.insert(index, snapshot.block);
        self.node_sources.insert(index, snapshot.source);
        Ok(())
    }

    pub(crate) fn set_image_alt(
        &mut self,
        block_id: &BlockId,
        alt_text: String,
    ) -> Result<String, EditError> {
        let block = self
            .blocks
            .iter_mut()
            .find(|block| block.id == *block_id)
            .ok_or(EditError::MissingBlock)?;
        if block.kind != ParagraphKind::Image {
            return Err(EditError::NotImageBlock);
        }
        Ok(std::mem::replace(&mut block.text, alt_text))
    }

    pub fn imported_image_asset(&self, block_id: &BlockId) -> Option<&ValidatedImageAsset> {
        let index = self.blocks.iter().position(|block| block.id == *block_id)?;
        match &self.node_sources[index] {
            EpubNodeSource::Image {
                origin: EpubImageOrigin::Inserted { asset, .. },
                ..
            } => Some(asset),
            _ => None,
        }
    }

    fn image_additions(&self) -> Vec<EpubImageAddition> {
        self.node_sources
            .iter()
            .filter_map(|source| match source {
                EpubNodeSource::Image {
                    origin:
                        EpubImageOrigin::Inserted {
                            archive_path,
                            asset,
                            ..
                        },
                    ..
                } => Some(EpubImageAddition {
                    archive_path: archive_path.clone(),
                    asset: asset.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    fn reserve_image_archive_path(&mut self, asset: &ValidatedImageAsset) -> String {
        let digest = asset.safe_digest_prefix();
        let root = self
            .resource_path
            .split_once('/')
            .map_or("", |(root, _)| root);
        let directory = if root.is_empty() {
            "Images".to_owned()
        } else {
            format!("{root}/Images")
        };
        let stem = format!("readloom-{digest}");
        let extension = asset.media_type.extension();
        let mut candidate = format!("{directory}/{stem}.{extension}");
        let mut suffix = 2_u32;
        while self.reserved_archive_paths.contains(&candidate) {
            candidate = format!("{directory}/{stem}-{suffix}.{extension}");
            suffix = suffix.saturating_add(1);
        }
        self.reserved_archive_paths.insert(candidate.clone());
        candidate
    }

    pub fn render_xhtml(&self) -> String {
        struct Replacement {
            range: Range<usize>,
            text: String,
        }

        let mut replacements = self
            .removed_source_ranges
            .iter()
            .cloned()
            .map(|range| Replacement {
                range,
                text: String::new(),
            })
            .collect::<Vec<_>>();
        for (block, source) in self.blocks.iter().zip(&self.node_sources) {
            match source {
                EpubNodeSource::Text {
                    original_text,
                    text_spans,
                    insertion_offset,
                    ..
                } if block.editable && block.text != *original_text => {
                    let escaped = escape(&block.text).into_owned();
                    if let Some(first) = text_spans.first() {
                        replacements.push(Replacement {
                            range: first.clone(),
                            text: escaped,
                        });
                        for span in text_spans.iter().skip(1) {
                            replacements.push(Replacement {
                                range: span.clone(),
                                text: String::new(),
                            });
                        }
                    } else {
                        replacements.push(Replacement {
                            range: *insertion_offset..*insertion_offset,
                            text: escaped,
                        });
                    }
                }
                EpubNodeSource::Image {
                    origin: EpubImageOrigin::Existing { element_span, .. },
                    original_alt,
                    ..
                } if block.text
                    != if original_alt.trim().is_empty() {
                        "图片"
                    } else {
                        original_alt
                    } =>
                {
                    replacements.push(Replacement {
                        range: element_span.clone(),
                        text: patch_start_tag_attribute(
                            &self.source[element_span.clone()],
                            "alt",
                            &block.text,
                        )
                        .expect("parsed EPUB image start tag remains patchable"),
                    });
                }
                EpubNodeSource::Image {
                    origin:
                        EpubImageOrigin::Inserted {
                            insertion_offset, ..
                        },
                    src,
                    ..
                } => {
                    replacements.push(Replacement {
                        range: *insertion_offset..*insertion_offset,
                        text: format!(
                            "<img src=\"{}\" alt=\"{}\" />",
                            escape(src),
                            escape(&block.text)
                        ),
                    });
                }
                _ => {}
            }
        }
        for group in &self.structured_text_groups {
            let rendered = group
                .block_ids
                .iter()
                .zip(&group.templates)
                .map(|(block_id, template)| {
                    let block = self
                        .blocks
                        .iter()
                        .find(|block| &block.id == block_id)
                        .expect("EPUB structured text group references an existing block");
                    template.render(&block.text)
                })
                .collect::<Vec<_>>()
                .join("\n");
            replacements.push(Replacement {
                range: group.source_span.clone(),
                text: rendered,
            });
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
            output.push_str(&replacement.text);
            cursor = replacement.range.end;
        }
        output.push_str(&self.source[cursor..]);
        output
    }
}

fn patch_start_tag_attribute(
    tag: &str,
    attribute_name: &str,
    value: &str,
) -> Result<String, CoreError> {
    let bytes = tag.as_bytes();
    if bytes.first() != Some(&b'<') || bytes.last() != Some(&b'>') {
        return Err(invalid_epub_edit("EPUB 图片标签边界无效。"));
    }
    let mut cursor = 1usize;
    while cursor < bytes.len()
        && !bytes[cursor].is_ascii_whitespace()
        && !matches!(bytes[cursor], b'/' | b'>')
    {
        cursor += 1;
    }
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || matches!(bytes[cursor], b'/' | b'>') {
            break;
        }
        let name_start = cursor;
        while cursor < bytes.len()
            && !bytes[cursor].is_ascii_whitespace()
            && !matches!(bytes[cursor], b'=' | b'/' | b'>')
        {
            cursor += 1;
        }
        let name_end = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'=') {
            return Err(invalid_epub_edit("EPUB 图片属性缺少等号。"));
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let quote = *bytes
            .get(cursor)
            .filter(|quote| matches!(quote, b'\'' | b'\"'))
            .ok_or_else(|| invalid_epub_edit("EPUB 图片属性没有引号。"))?;
        cursor += 1;
        let value_start = cursor;
        while cursor < bytes.len() && bytes[cursor] != quote {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return Err(invalid_epub_edit("EPUB 图片属性没有闭合。"));
        }
        let value_end = cursor;
        cursor += 1;
        let local = tag[name_start..name_end]
            .rsplit(':')
            .next()
            .unwrap_or_default();
        if local == attribute_name {
            let mut output = String::with_capacity(tag.len() + value.len());
            output.push_str(&tag[..value_start]);
            output.push_str(&escape(value));
            output.push_str(&tag[value_end..]);
            return Ok(output);
        }
    }
    let closing = bytes.len() - 1;
    let mut insertion = closing;
    while insertion > 0 && bytes[insertion - 1].is_ascii_whitespace() {
        insertion -= 1;
    }
    if insertion > 0 && bytes[insertion - 1] == b'/' {
        insertion -= 1;
    }
    let mut output = String::with_capacity(tag.len() + attribute_name.len() + value.len() + 4);
    output.push_str(&tag[..insertion]);
    output.push(' ');
    output.push_str(attribute_name);
    output.push_str("=\"");
    output.push_str(&escape(value));
    output.push('"');
    output.push_str(&tag[insertion..]);
    Ok(output)
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
    let additions = draft.image_additions();
    let existing_archive_paths = source
        .file_names()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let (package_path, package_source) = read_package_document(&mut source)?;
    let removals = select_safe_image_removals(
        draft,
        &mut source,
        &package_path,
        &package_source,
        &rendered,
    )?;
    let rendered_package =
        render_package_with_image_changes(&package_source, &package_path, &additions, &removals)?;
    let mut chapter_replaced = false;
    let mut package_replaced = false;
    for index in 0..source.len() {
        let entry = source
            .by_index(index)
            .map_err(|_| invalid_epub_edit("无法读取 EPUB 容器条目。"))?;
        if removals.contains(entry.name()) {
            drop(entry);
            continue;
        }
        if entry.name() == draft.resource_path || entry.name() == package_path {
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
                .start_file(&name, options)
                .map_err(|_| invalid_epub_edit("无法写入修改后的 EPUB 章节。"))?;
            if name == draft.resource_path {
                writer.write_all(rendered.as_bytes())?;
                chapter_replaced = true;
            } else {
                writer.write_all(rendered_package.as_bytes())?;
                package_replaced = true;
            }
        } else {
            writer
                .raw_copy_file(entry)
                .map_err(|_| invalid_epub_edit("无法保真复制 EPUB 原始条目。"))?;
        }
    }
    if !chapter_replaced {
        return Err(invalid_epub_edit("EPUB 当前章节条目已经不存在。"));
    }
    if !package_replaced {
        return Err(invalid_epub_edit("EPUB package 文档已经不存在。"));
    }
    for addition in &additions {
        if existing_archive_paths.contains(&addition.archive_path) {
            continue;
        }
        writer
            .start_file(
                &addition.archive_path,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(|_| invalid_epub_edit("无法写入新增的 EPUB 图片。"))?;
        writer.write_all(&addition.asset.bytes)?;
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
    let paragraphs = reopened.paragraphs();
    let blocks_match = draft.blocks.len() == paragraphs.len()
        && draft
            .blocks
            .iter()
            .zip(paragraphs.iter())
            .all(|(block, paragraph)| {
                if block.kind == ParagraphKind::Image {
                    (paragraph.kind == ParagraphKind::Image && paragraph.text == block.text)
                        || (paragraph.kind == ParagraphKind::Paragraph
                            && paragraph.text == format!("[图片无法显示：{}]", block.text))
                } else {
                    paragraph.kind == block.kind && paragraph.text == block.text
                }
            });
    if !blocks_match {
        return Err(invalid_epub_edit(
            "重打包后的 EPUB 正文校验失败，原文件保持不变。",
        ));
    }
    Ok(())
}

fn parse_xhtml_blocks(
    resource_path: &str,
    source: &str,
) -> Result<(Vec<EditableBlock>, Vec<EpubNodeSource>), CoreError> {
    struct CurrentBlock {
        element_start: usize,
        element_name: Vec<u8>,
        kind: ParagraphKind,
        text: String,
        text_spans: Vec<Range<usize>>,
        structural_template: Option<EpubTextElementTemplate>,
    }

    let mut reader = Reader::from_str(source);
    reader.config_mut().check_end_names = true;
    let mut blocks = Vec::new();
    let mut node_sources = Vec::new();
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
                if let Some(block) = current.as_mut() {
                    block.structural_template = None;
                }
                if hidden_depth > 0 || is_hidden(name) {
                    hidden_depth = hidden_depth.saturating_add(1);
                } else if name == b"img" {
                    push_image_block(
                        resource_path,
                        event_start..event_end,
                        &element,
                        reader.decoder(),
                        false,
                        (&mut blocks, &mut node_sources),
                    )?;
                } else if current.is_none()
                    && let Some(kind) = block_kind(name)
                {
                    current = Some(CurrentBlock {
                        element_start: event_start,
                        element_name: name.to_vec(),
                        kind,
                        text: String::new(),
                        text_spans: Vec::new(),
                        structural_template: structural_text_template(&element, reader.decoder())?,
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
                    node_sources.push(EpubNodeSource::Text {
                        original_text: text,
                        element_span: block.element_start..event_end,
                        text_spans: block.text_spans,
                        insertion_offset: event_start,
                        structural_template: block.structural_template,
                    });
                }
            }
            Event::Empty(element) if hidden_depth == 0 => {
                let qualified_name = element.name();
                let name = local_name(qualified_name.as_ref());
                if let Some(block) = current.as_mut() {
                    block.structural_template = None;
                }
                if name == b"img" {
                    push_image_block(
                        resource_path,
                        event_start..event_end,
                        &element,
                        reader.decoder(),
                        true,
                        (&mut blocks, &mut node_sources),
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
                    block.structural_template = None;
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
    Ok((blocks, node_sources))
}

fn structural_text_template(
    element: &BytesStart<'_>,
    decoder: Decoder,
) -> Result<Option<EpubTextElementTemplate>, CoreError> {
    let qualified_name = element.name();
    let tag_name = std::str::from_utf8(local_name(qualified_name.as_ref()))
        .map_err(|_| invalid_epub_edit("EPUB 正文元素名不是 UTF-8。"))?;
    if !matches!(
        tag_name,
        "p" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
    ) {
        return Ok(None);
    }
    let mut attributes = Vec::new();
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(|_| invalid_epub_edit("EPUB 正文元素属性无效。"))?;
        let name = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|_| invalid_epub_edit("EPUB 正文属性名不是 UTF-8。"))?
            .to_owned();
        let value = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)
            .map_err(|_| invalid_epub_edit("EPUB 正文属性无法解码。"))?
            .into_owned();
        let lower_name = name.to_ascii_lowercase();
        let unique = matches!(lower_name.as_str(), "id" | "xml:id");
        let allowed = unique
            || matches!(
                lower_name.as_str(),
                "class" | "role" | "dir" | "lang" | "xml:lang" | "epub:type" | "title"
            )
            || (lower_name == "style" && safe_epub_style_attribute(&value))
            || (lower_name == "xmlns:epub" && value == "http://www.idpf.org/2007/ops");
        if !allowed || lower_name.starts_with("on") {
            return Ok(None);
        }
        attributes.push(EpubTextAttribute {
            name,
            value,
            unique,
        });
    }
    Ok(Some(EpubTextElementTemplate {
        tag_name: tag_name.to_owned(),
        attributes,
    }))
}

fn safe_epub_style_attribute(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    ![
        "url(",
        "expression(",
        "javascript:",
        "behavior:",
        "@import",
        "-moz-binding",
    ]
    .iter()
    .any(|forbidden| normalized.contains(forbidden))
}

fn push_image_block(
    resource_path: &str,
    source_span: Range<usize>,
    element: &BytesStart<'_>,
    decoder: Decoder,
    removable: bool,
    output: (&mut Vec<EditableBlock>, &mut Vec<EpubNodeSource>),
) -> Result<(), CoreError> {
    let (blocks, node_sources) = output;
    let mut alt = None;
    let mut src = None;
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(|_| invalid_epub_edit("EPUB 图片属性无效。"))?;
        if local_name(attribute.key.as_ref()) == b"alt" {
            alt = Some(
                attribute
                    .decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)
                    .map_err(|_| invalid_epub_edit("EPUB 图片属性无法解码。"))?
                    .into_owned(),
            );
        } else if local_name(attribute.key.as_ref()) == b"src" {
            src = Some(
                attribute
                    .decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)
                    .map_err(|_| invalid_epub_edit("EPUB 图片路径无法解码。"))?
                    .into_owned(),
            );
        }
    }
    let original_alt = alt.unwrap_or_default();
    let text = if original_alt.trim().is_empty() {
        "图片".to_owned()
    } else {
        original_alt.clone()
    };
    let src = src.unwrap_or_default();
    let resolved_resource_path = resolve_archive_reference(resource_path, &src).unwrap_or_default();
    blocks.push(EditableBlock {
        id: BlockId::epub(resource_path, blocks.len()),
        chapter_key: ChapterKey::new(resource_path),
        kind: ParagraphKind::Image,
        text: text.clone(),
        editable: false,
    });
    node_sources.push(EpubNodeSource::Image {
        origin: EpubImageOrigin::Existing {
            element_span: source_span,
            removable,
        },
        src,
        resolved_resource_path,
        original_alt,
    });
    Ok(())
}

fn read_package_document(archive: &mut ZipArchive<File>) -> Result<(String, String), CoreError> {
    let container = read_archive_text(archive, "META-INF/container.xml", "EPUB container")?;
    let mut reader = Reader::from_str(&container);
    reader.config_mut().check_end_names = true;
    let mut package_path = None;
    loop {
        match reader
            .read_event()
            .map_err(|_| invalid_epub_edit("EPUB container.xml 结构无效。"))?
        {
            Event::Start(element) | Event::Empty(element)
                if local_name(element.name().as_ref()) == b"rootfile" =>
            {
                for attribute in element.attributes().with_checks(false) {
                    let attribute =
                        attribute.map_err(|_| invalid_epub_edit("EPUB rootfile 属性无效。"))?;
                    if local_name(attribute.key.as_ref()) == b"full-path" {
                        let value = attribute
                            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                            .map_err(|_| invalid_epub_edit("EPUB package 路径无法解码。"))?
                            .into_owned();
                        package_path = Some(
                            normalize_archive_path(&value)
                                .map_err(|_| invalid_epub_edit("EPUB package 路径不安全。"))?,
                        );
                        break;
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        if package_path.is_some() {
            break;
        }
    }
    let package_path =
        package_path.ok_or_else(|| invalid_epub_edit("EPUB container.xml 缺少 package 路径。"))?;
    let package = read_archive_text(archive, &package_path, "EPUB package")?;
    Ok((package_path, package))
}

fn read_archive_text(
    archive: &mut ZipArchive<File>,
    name: &str,
    label: &str,
) -> Result<String, CoreError> {
    let mut bytes = Vec::new();
    archive
        .by_name(name)
        .map_err(|_| invalid_epub_edit(&format!("{label} 条目已经不存在。")))?
        .read_to_end(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid_epub_edit(&format!("{label} 不是 UTF-8 文本。")))
}

#[derive(Debug)]
struct PackageManifest {
    end: usize,
    used_ids: BTreeSet<String>,
    referenced_ids: BTreeSet<String>,
    resource_references: BTreeSet<String>,
    items: Vec<PackageManifestItem>,
}

#[derive(Debug)]
struct PackageManifestItem {
    range: Option<Range<usize>>,
    id: String,
    archive_path: String,
    media_type: String,
    properties: String,
}

fn parse_package_manifest(source: &str, package_path: &str) -> Result<PackageManifest, CoreError> {
    let mut reader = Reader::from_str(source);
    reader.config_mut().check_end_names = true;
    let mut end = None;
    let mut used_ids = BTreeSet::new();
    let mut referenced_ids = BTreeSet::new();
    let mut resource_references = BTreeSet::new();
    let mut items = Vec::new();
    loop {
        let event_start = reader.buffer_position() as usize;
        let event = reader
            .read_event()
            .map_err(|_| invalid_epub_edit("EPUB package 文档结构无效。"))?;
        let event_end = reader.buffer_position() as usize;
        let is_empty_event = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(element) | Event::Empty(element) => {
                let is_item = local_name(element.name().as_ref()) == b"item";
                let mut id = String::new();
                let mut href = String::new();
                let mut media_type = String::new();
                let mut properties = String::new();
                for attribute in element.attributes().with_checks(false) {
                    let attribute =
                        attribute.map_err(|_| invalid_epub_edit("EPUB package 属性无效。"))?;
                    let name = local_name(attribute.key.as_ref());
                    let value = attribute
                        .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|_| invalid_epub_edit("EPUB package 属性无法解码。"))?
                        .into_owned();
                    match name {
                        b"id" => {
                            id.clone_from(&value);
                            used_ids.insert(value);
                        }
                        b"href" => href = value,
                        b"media-type" => media_type = value,
                        b"properties" => properties = value,
                        b"idref" | b"fallback" | b"media-overlay" => {
                            referenced_ids.insert(value);
                        }
                        b"refines" if value.starts_with('#') => {
                            referenced_ids.insert(value[1..].to_owned());
                        }
                        _ => {}
                    }
                }
                if is_item {
                    let archive_path =
                        resolve_archive_reference(package_path, &href).unwrap_or_default();
                    items.push(PackageManifestItem {
                        range: is_empty_event.then_some(event_start..event_end),
                        id,
                        archive_path,
                        media_type,
                        properties,
                    });
                } else if !href.is_empty()
                    && let Ok(path) = resolve_archive_reference(package_path, &href)
                {
                    resource_references.insert(path);
                }
            }
            Event::End(element) if local_name(element.name().as_ref()) == b"manifest" => {
                end = Some(event_start);
                break;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(PackageManifest {
        end: end.ok_or_else(|| invalid_epub_edit("EPUB package 文档缺少 manifest。"))?,
        used_ids,
        referenced_ids,
        resource_references,
        items,
    })
}

fn select_safe_image_removals(
    draft: &EpubDraft,
    archive: &mut ZipArchive<File>,
    package_path: &str,
    package_source: &str,
    rendered_chapter: &str,
) -> Result<BTreeSet<String>, CoreError> {
    if draft.removed_images.is_empty() {
        return Ok(BTreeSet::new());
    }
    let manifest = parse_package_manifest(package_source, package_path)?;
    let archive_paths = archive
        .file_names()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut removals = BTreeSet::new();
    for removed in &draft.removed_images {
        let candidate = &removed.archive_path;
        if !archive_paths.contains(candidate) || removals.contains(candidate) {
            continue;
        }
        let matching = manifest
            .items
            .iter()
            .filter(|item| item.archive_path == *candidate)
            .collect::<Vec<_>>();
        let [item] = matching.as_slice() else {
            continue;
        };
        if item.range.is_none()
            || !item.media_type.starts_with("image/")
            || item
                .properties
                .split_whitespace()
                .any(|value| value == "cover-image")
            || manifest.referenced_ids.contains(&item.id)
            || manifest.resource_references.contains(candidate)
            || archive_has_resource_reference(
                archive,
                package_path,
                &draft.resource_path,
                rendered_chapter,
                candidate,
            )?
        {
            continue;
        }
        removals.insert(candidate.clone());
    }
    Ok(removals)
}

fn archive_has_resource_reference(
    archive: &mut ZipArchive<File>,
    package_path: &str,
    edited_resource_path: &str,
    rendered_chapter: &str,
    target: &str,
) -> Result<bool, CoreError> {
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| invalid_epub_edit("无法建立 EPUB 资源引用索引。"))?;
        let name = entry.name().to_owned();
        if name == package_path || name == target || entry.is_dir() {
            continue;
        }
        let extension = Path::new(&name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if name == edited_resource_path {
            if xml_references_resource(&name, rendered_chapter, target).unwrap_or(true) {
                return Ok(true);
            }
        } else if matches!(
            extension.as_str(),
            "xhtml" | "html" | "htm" | "xml" | "svg" | "ncx"
        ) {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            let Ok(text) = String::from_utf8(bytes) else {
                return Ok(true);
            };
            if xml_references_resource(&name, &text, target).unwrap_or(true) {
                return Ok(true);
            }
        } else if extension == "css" {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            let basename = target.rsplit('/').next().unwrap_or(target).as_bytes();
            if !basename.is_empty()
                && bytes
                    .windows(basename.len())
                    .any(|window| window == basename)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn xml_references_resource(base_path: &str, source: &str, target: &str) -> Result<bool, ()> {
    let mut reader = Reader::from_str(source);
    reader.config_mut().check_end_names = true;
    loop {
        match reader.read_event().map_err(|_| ())? {
            Event::Start(element) | Event::Empty(element) => {
                for attribute in element.attributes().with_checks(false) {
                    let attribute = attribute.map_err(|_| ())?;
                    if matches!(
                        local_name(attribute.key.as_ref()),
                        b"src" | b"href" | b"poster" | b"data"
                    ) {
                        let value = attribute
                            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                            .map_err(|_| ())?;
                        if resolve_archive_reference(base_path, &value)
                            .is_ok_and(|resolved| resolved == target)
                        {
                            return Ok(true);
                        }
                    }
                }
            }
            Event::Eof => return Ok(false),
            _ => {}
        }
    }
}

fn render_package_with_image_changes(
    source: &str,
    package_path: &str,
    additions: &[EpubImageAddition],
    removals: &BTreeSet<String>,
) -> Result<String, CoreError> {
    if additions.is_empty() && removals.is_empty() {
        return Ok(source.to_owned());
    }
    let manifest = parse_package_manifest(source, package_path)?;
    let mut used_ids = manifest.used_ids;
    let mut items = String::new();
    for addition in additions {
        if manifest
            .items
            .iter()
            .any(|item| item.archive_path == addition.archive_path)
        {
            continue;
        }
        let href = relative_archive_reference(package_path, &addition.archive_path)
            .map_err(|_| invalid_epub_edit("新增 EPUB 图片无法映射到 manifest。"))?;
        let base_id = format!("readloom-image-{}", addition.asset.safe_digest_prefix());
        let mut id = base_id.clone();
        let mut suffix = 2_u32;
        while used_ids.contains(&id) {
            id = format!("{base_id}-{suffix}");
            suffix = suffix.saturating_add(1);
        }
        used_ids.insert(id.clone());
        items.push_str(&format!(
            "<item id=\"{id}\" href=\"{}\" media-type=\"{}\"/>",
            escape(&href),
            addition.asset.media_type.media_type()
        ));
    }
    let mut replacements = manifest
        .items
        .into_iter()
        .filter(|item| removals.contains(&item.archive_path))
        .filter_map(|item| item.range)
        .map(|range| (range, String::new()))
        .collect::<Vec<_>>();
    replacements.push((manifest.end..manifest.end, items));
    replacements.sort_by_key(|(range, _)| range.start);
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0usize;
    for (range, replacement) in replacements {
        if range.start < cursor {
            return Err(invalid_epub_edit("EPUB manifest 补丁范围发生重叠。"));
        }
        output.push_str(&source[cursor..range.start]);
        output.push_str(&replacement);
        cursor = range.end;
    }
    output.push_str(&source[cursor..]);
    Ok(output)
}

fn normalize_archive_path(value: &str) -> Result<String, ()> {
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
    {
        return Err(());
    }
    let segments = value.split('/').collect::<Vec<_>>();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || matches!(*segment, "." | ".."))
    {
        return Err(());
    }
    Ok(segments.join("/"))
}

fn resolve_archive_reference(base_resource: &str, reference: &str) -> Result<String, ()> {
    let reference = reference
        .split(['?', '#'])
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(())?;
    if reference.starts_with('/') || reference.starts_with('\\') || reference.contains(':') {
        return Err(());
    }
    let base = normalize_archive_path(base_resource)?;
    let mut segments = base.rsplit_once('/').map_or(Vec::new(), |(parent, _)| {
        parent.split('/').map(str::to_owned).collect()
    });
    for segment in reference.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop().ok_or(())?;
            }
            value if value.contains('\\') => return Err(()),
            value => segments.push(value.to_owned()),
        }
    }
    normalize_archive_path(&segments.join("/"))
}

fn relative_archive_reference(base_resource: &str, target: &str) -> Result<String, ()> {
    let base = normalize_archive_path(base_resource)?;
    let target = normalize_archive_path(target)?;
    let base_directory = base.rsplit_once('/').map_or_else(Vec::new, |(parent, _)| {
        parent.split('/').collect::<Vec<_>>()
    });
    let target_segments = target.split('/').collect::<Vec<_>>();
    let common = base_directory
        .iter()
        .zip(&target_segments)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = Vec::new();
    relative.extend(std::iter::repeat_n("..", base_directory.len() - common));
    relative.extend(target_segments[common..].iter().copied());
    if relative.is_empty() {
        return Err(());
    }
    Ok(relative.join("/"))
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

    use crate::{
        DocumentDraft, EditCommand, EditSession, ImageMediaType, InsertSide, ReadloomCore,
        ValidatedImageAsset, ViewAnchor,
    };

    use super::*;

    #[test]
    fn inserted_epub_image_uses_the_shared_reversible_edit_session() {
        let source = r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>锚点正文</p><img src="../images/original.png" alt="原图"/></body></html>"#;
        let (blocks, node_sources) =
            parse_xhtml_blocks("EPUB/text/one.xhtml", source).expect("parse EPUB nodes");
        let next_block_id = blocks.len() as u64;
        let anchor_block_id = blocks[0].id.clone();
        let original_image_id = blocks[1].id.clone();
        let draft = EpubDraft {
            source_path: PathBuf::from("book.epub"),
            base_fingerprint: "base".to_owned(),
            chapter_index: 0,
            resource_path: "EPUB/text/one.xhtml".to_owned(),
            source: Arc::from(source),
            blocks,
            node_sources,
            removed_source_ranges: Vec::new(),
            removed_images: Vec::new(),
            reserved_archive_paths: BTreeSet::new(),
            next_block_id,
            structured_text_groups: Vec::new(),
            next_text_group_id: 0,
        };
        let mut session = EditSession::new(
            "base".to_owned(),
            DocumentDraft::Epub(draft),
            ViewAnchor {
                chapter_key: ChapterKey::new("EPUB/text/one.xhtml"),
                block_id: anchor_block_id.clone(),
                character_offset_utf16: 0,
                pixel_offset_from_viewport_top: -7.0,
            },
        );
        let asset = ValidatedImageAsset {
            bytes: Arc::from([1_u8, 2, 3]),
            media_type: ImageMediaType::Png,
            width: 1,
            height: 1,
            digest: "0123456789abcdef".to_owned(),
        };

        let change = session
            .apply(EditCommand::InsertEpubImage {
                anchor_block_id: anchor_block_id.clone(),
                side: InsertSide::After,
                asset,
                alt_text: "新增 <插图>".to_owned(),
            })
            .expect("insert EPUB image through EditSession");

        assert!(change.structure_changed());
        let inserted_id = session.draft().blocks()[1].id.clone();
        assert_eq!(session.draft().blocks()[0].id, anchor_block_id);
        assert_eq!(session.draft().blocks()[1].kind, ParagraphKind::Image);
        assert_eq!(session.draft().blocks()[1].text, "新增 <插图>");
        assert_eq!(session.draft().blocks()[2].id, original_image_id);
        assert!(session.undo().expect("undo image insertion"));
        assert_eq!(session.draft().blocks().len(), 2);
        assert_eq!(session.draft().blocks()[1].id, original_image_id);
        assert!(session.redo().expect("redo image insertion"));
        assert_eq!(session.draft().blocks()[1].id, inserted_id);
    }

    #[test]
    fn saving_an_inserted_image_updates_xhtml_manifest_and_archive_together() {
        let (directory, path) = publication_fixture();
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3"))
            .expect("open Readloom core");
        let document = core.open_epub(&path).expect("open publication fixture");
        let mut draft = EpubDraft::from_document(&document).expect("create EPUB draft");
        let anchor = draft
            .blocks()
            .iter()
            .find(|block| block.editable && block.kind == ParagraphKind::Paragraph)
            .expect("editable paragraph anchor")
            .id
            .clone();
        let asset = ValidatedImageAsset {
            bytes: Arc::from(tiny_png()),
            media_type: ImageMediaType::Png,
            width: 1,
            height: 1,
            digest: "0123456789abcdef".to_owned(),
        };

        draft
            .insert_image(&anchor, InsertSide::After, asset, "新增 & 插图".to_owned())
            .expect("insert image into EPUB draft");
        let saved = core.save_epub_draft(&draft).expect("save image edit");

        assert_eq!(saved.images().len(), 2);
        assert!(saved.paragraphs().iter().any(|paragraph| {
            paragraph.kind == ParagraphKind::Image && paragraph.text == "新增 & 插图"
        }));
        let mut archive = ZipArchive::new(File::open(&path).expect("open saved EPUB"))
            .expect("read saved EPUB archive");
        let image_path = "EPUB/Images/readloom-0123456789ab.png";
        let mut image = Vec::new();
        archive
            .by_name(image_path)
            .expect("inserted image archive entry")
            .read_to_end(&mut image)
            .expect("read inserted image");
        assert_eq!(image, tiny_png());
        let mut package = String::new();
        archive
            .by_name("EPUB/package.opf")
            .expect("saved package document")
            .read_to_string(&mut package)
            .expect("read package document");
        assert!(
            package.contains("href=\"Images/readloom-0123456789ab.png\" media-type=\"image/png\"")
        );
        let mut chapter = String::new();
        archive
            .by_name("EPUB/chapter.xhtml")
            .expect("saved chapter")
            .read_to_string(&mut chapter)
            .expect("read saved chapter");
        assert!(
            chapter.contains(
                "<img src=\"Images/readloom-0123456789ab.png\" alt=\"新增 &amp; 插图\" />"
            )
        );
    }

    #[test]
    fn repeated_saves_do_not_duplicate_and_can_later_remove_an_inserted_image() {
        let (directory, path) = publication_fixture();
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3"))
            .expect("open Readloom core");
        let document = core.open_epub(&path).expect("open publication fixture");
        let mut draft = EpubDraft::from_document(&document).expect("create EPUB draft");
        let anchor = draft
            .blocks()
            .iter()
            .find(|block| block.kind == ParagraphKind::Paragraph)
            .expect("paragraph anchor")
            .id
            .clone();
        let asset = ValidatedImageAsset {
            bytes: Arc::from(tiny_png()),
            media_type: ImageMediaType::Png,
            width: 1,
            height: 1,
            digest: "fedcba9876543210".to_owned(),
        };
        let (_, inserted) = draft
            .insert_image(&anchor, InsertSide::After, asset, "首次插图".to_owned())
            .expect("insert image");
        let inserted_id = inserted.block.id.clone();

        let first = core.save_epub_draft(&draft).expect("first save");
        draft.rebase(first.path().to_owned(), first.fingerprint().to_owned());
        draft
            .set_image_alt(&inserted_id, "二次保存插图".to_owned())
            .expect("edit inserted image alt");
        let second = core.save_epub_draft(&draft).expect("second save");
        draft.rebase(second.path().to_owned(), second.fingerprint().to_owned());

        let image_path = "EPUB/Images/readloom-fedcba987654.png";
        let mut archive = ZipArchive::new(File::open(&path).expect("open twice-saved EPUB"))
            .expect("read twice-saved EPUB");
        assert_eq!(
            archive
                .file_names()
                .filter(|name| *name == image_path)
                .count(),
            1
        );
        let mut package = String::new();
        archive
            .by_name("EPUB/package.opf")
            .expect("package after second save")
            .read_to_string(&mut package)
            .expect("read package");
        assert_eq!(
            package.matches("Images/readloom-fedcba987654.png").count(),
            1
        );
        drop(archive);

        draft
            .remove_image(&inserted_id)
            .expect("remove previously saved inserted image");
        core.save_epub_draft(&draft)
            .expect("save inserted image removal");
        let mut archive = ZipArchive::new(File::open(&path).expect("open deletion save"))
            .expect("read deletion save");
        assert!(archive.by_name(image_path).is_err());
        let mut package = String::new();
        archive
            .by_name("EPUB/package.opf")
            .expect("package after removal")
            .read_to_string(&mut package)
            .expect("read package after removal");
        assert!(!package.contains("readloom-fedcba987654.png"));
    }

    #[test]
    fn deleting_an_unshared_image_removes_its_tag_manifest_item_and_archive_entry() {
        let (directory, path) = image_deletion_fixture(false);
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3"))
            .expect("open Readloom core");
        let document = core.open_epub(&path).expect("open deletion fixture");
        let mut draft = EpubDraft::from_document(&document).expect("create EPUB draft");
        let image = draft
            .blocks()
            .iter()
            .find(|block| block.kind == ParagraphKind::Image)
            .expect("deletable image")
            .id
            .clone();

        draft.remove_image(&image).expect("remove unshared image");
        core.save_epub_draft(&draft).expect("save image deletion");

        let mut archive = ZipArchive::new(File::open(&path).expect("open saved EPUB"))
            .expect("read saved archive");
        assert!(archive.by_name("EPUB/Images/target.png").is_err());
        let mut package = String::new();
        archive
            .by_name("EPUB/package.opf")
            .expect("saved package")
            .read_to_string(&mut package)
            .expect("read saved package");
        assert!(!package.contains("target.png"));
        let mut chapter = String::new();
        archive
            .by_name("EPUB/chapter.xhtml")
            .expect("saved chapter")
            .read_to_string(&mut chapter)
            .expect("read saved chapter");
        assert!(!chapter.contains("target.png"));
    }

    #[test]
    fn deleting_a_shared_image_keeps_manifest_and_binary_for_other_chapters() {
        let (directory, path) = image_deletion_fixture(true);
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3"))
            .expect("open Readloom core");
        let document = core.open_epub(&path).expect("open shared deletion fixture");
        let mut draft = EpubDraft::from_document(&document).expect("create EPUB draft");
        let image = draft
            .blocks()
            .iter()
            .find(|block| block.kind == ParagraphKind::Image)
            .expect("shared image")
            .id
            .clone();

        draft
            .remove_image(&image)
            .expect("remove current image reference");
        core.save_epub_draft(&draft)
            .expect("save shared image reference deletion");

        let mut archive = ZipArchive::new(File::open(&path).expect("open saved EPUB"))
            .expect("read saved archive");
        assert!(archive.by_name("EPUB/Images/target.png").is_ok());
        let mut package = String::new();
        archive
            .by_name("EPUB/package.opf")
            .expect("saved package")
            .read_to_string(&mut package)
            .expect("read saved package");
        assert!(package.contains("Images/target.png"));
        let mut second = String::new();
        archive
            .by_name("EPUB/second.xhtml")
            .expect("preserved second chapter")
            .read_to_string(&mut second)
            .expect("read second chapter");
        assert!(second.contains("Images/target.png"));
    }

    #[test]
    fn xhtml_edits_escape_text_and_keep_inline_elements_and_attributes() {
        let source = r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p class="lead">开头 <em data-x="1">强调</em> 结尾 &amp;</p><img src="cover.png" alt="插图"/></body></html>"#;
        let (blocks, node_sources) =
            parse_xhtml_blocks("EPUB/one.xhtml", source).expect("parse editable XHTML blocks");
        let next_block_id = blocks.len() as u64;
        let mut draft = EpubDraft {
            source_path: PathBuf::from("book.epub"),
            base_fingerprint: "base".to_owned(),
            chapter_index: 0,
            resource_path: "EPUB/one.xhtml".to_owned(),
            source: Arc::from(source),
            blocks,
            node_sources,
            removed_source_ranges: Vec::new(),
            removed_images: Vec::new(),
            reserved_archive_paths: BTreeSet::new(),
            next_block_id,
            structured_text_groups: Vec::new(),
            next_text_group_id: 0,
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
    fn structured_text_edit_splits_epub_paragraph_and_passes_strict_reopen_validation() {
        let (directory, path) = publication_fixture();
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3"))
            .expect("open Readloom core");
        let document = core.open_epub(&path).expect("open EPUB fixture");
        let draft = EpubDraft::from_document(&document).expect("create EPUB draft");
        let target = draft
            .blocks()
            .iter()
            .find(|block| block.text == "原始正文 & 实体")
            .expect("editable EPUB paragraph")
            .id
            .clone();
        let mut session = EditSession::new(
            document.fingerprint().to_owned(),
            DocumentDraft::Epub(draft),
            ViewAnchor {
                chapter_key: ChapterKey::new("EPUB/chapter.xhtml"),
                block_id: target.clone(),
                character_offset_utf16: 0,
                pixel_offset_from_viewport_top: -9.0,
            },
        );

        let change = session
            .apply(EditCommand::ReplaceText {
                block_id: target,
                text: "甲😀\n乙".to_owned(),
                caret_utf16: 5,
            })
            .expect("split EPUB paragraph");

        assert!(change.structure_changed());
        let ticket = session.begin_save();
        let saved_draft = match ticket.into_draft() {
            DocumentDraft::Epub(draft) => draft,
            DocumentDraft::Txt(_) => panic!("expected EPUB draft"),
        };
        let rendered = saved_draft.render_xhtml();
        assert!(rendered.contains("<p class=\"lead\">甲😀</p>"));
        assert!(rendered.contains("<p class=\"lead\">乙</p>"));

        let reopened = core
            .save_epub_draft(&saved_draft)
            .expect("save and validate split EPUB");
        assert_eq!(
            reopened
                .paragraphs()
                .iter()
                .filter(|paragraph| paragraph.editable)
                .map(|paragraph| paragraph.text.as_str())
                .collect::<Vec<_>>(),
            [
                "第一章",
                "甲😀",
                "乙",
                "[图片无法显示：外部图片必须保持隔离]",
            ]
        );

        let merged_draft = EpubDraft::from_document(&reopened).expect("open split EPUB draft");
        let second_id = merged_draft
            .blocks()
            .iter()
            .find(|block| block.text == "乙")
            .expect("second split EPUB paragraph")
            .id
            .clone();
        let mut merge_session = EditSession::new(
            reopened.fingerprint().to_owned(),
            DocumentDraft::Epub(merged_draft),
            ViewAnchor {
                chapter_key: ChapterKey::new("EPUB/chapter.xhtml"),
                block_id: second_id.clone(),
                character_offset_utf16: 0,
                pixel_offset_from_viewport_top: -9.0,
            },
        );
        merge_session
            .apply(EditCommand::JoinAdjacentText {
                block_id: second_id,
                direction: JoinDirection::Previous,
            })
            .expect("merge adjacent EPUB paragraphs");
        let merged_ticket = merge_session.begin_save();
        let merged_draft = match merged_ticket.into_draft() {
            DocumentDraft::Epub(draft) => draft,
            DocumentDraft::Txt(_) => panic!("expected EPUB draft"),
        };
        let reopened = core
            .save_epub_draft(&merged_draft)
            .expect("save and validate merged EPUB");
        assert_eq!(
            reopened
                .paragraphs()
                .iter()
                .filter(|paragraph| paragraph.editable)
                .map(|paragraph| paragraph.text.as_str())
                .collect::<Vec<_>>(),
            ["第一章", "甲😀乙", "[图片无法显示：外部图片必须保持隔离]",]
        );
    }

    #[test]
    fn structured_text_edit_splits_heading_into_heading_and_paragraph_and_reopens() {
        let (directory, path) = publication_fixture();
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3"))
            .expect("open Readloom core");
        let document = core.open_epub(&path).expect("open EPUB fixture");
        let draft = EpubDraft::from_document(&document).expect("create EPUB draft");
        let target = draft
            .blocks()
            .iter()
            .find(|block| block.text == "第一章")
            .expect("editable EPUB heading")
            .id
            .clone();
        let mut session = EditSession::new(
            document.fingerprint().to_owned(),
            DocumentDraft::Epub(draft),
            ViewAnchor {
                chapter_key: ChapterKey::new("EPUB/chapter.xhtml"),
                block_id: target.clone(),
                character_offset_utf16: 3,
                pixel_offset_from_viewport_top: 0.0,
            },
        );
        session
            .apply(EditCommand::ReplaceText {
                block_id: target,
                text: "第一章\nEPUB 独立新段🙂".to_owned(),
                caret_utf16: 15,
            })
            .expect("split EPUB heading");
        let saved_draft = match session.begin_save().into_draft() {
            DocumentDraft::Epub(draft) => draft,
            DocumentDraft::Txt(_) => panic!("expected EPUB draft"),
        };

        let reopened = core
            .save_epub_draft(&saved_draft)
            .expect("save and validate split EPUB heading");

        assert_eq!(
            reopened
                .paragraphs()
                .iter()
                .filter(|paragraph| paragraph.editable)
                .map(|paragraph| (paragraph.kind, paragraph.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (ParagraphKind::Heading, "第一章"),
                (ParagraphKind::Paragraph, "EPUB 独立新段🙂"),
                (ParagraphKind::Paragraph, "原始正文 & 实体"),
                (
                    ParagraphKind::Paragraph,
                    "[图片无法显示：外部图片必须保持隔离]",
                ),
            ]
        );
    }

    #[test]
    fn structured_text_edit_preserves_safe_attributes_without_duplicate_ids() {
        let source = r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p id="unique" class="lead" style="text-align:center" role="note">甲乙</p></body></html>"#;
        let (blocks, node_sources) =
            parse_xhtml_blocks("EPUB/one.xhtml", source).expect("parse attributed paragraph");
        let target = blocks[0].id.clone();
        let mut draft = EpubDraft {
            source_path: PathBuf::from("book.epub"),
            base_fingerprint: "base".to_owned(),
            chapter_index: 0,
            resource_path: "EPUB/one.xhtml".to_owned(),
            source: Arc::from(source),
            next_block_id: blocks.len() as u64,
            blocks,
            node_sources,
            removed_source_ranges: Vec::new(),
            removed_images: Vec::new(),
            reserved_archive_paths: BTreeSet::new(),
            structured_text_groups: Vec::new(),
            next_text_group_id: 0,
        };

        draft
            .split_block_text(&target, vec!["甲".to_owned(), "乙".to_owned()])
            .expect("split attributed paragraph");
        let rendered = draft.render_xhtml();

        assert_eq!(rendered.matches("id=\"unique\"").count(), 1);
        assert_eq!(rendered.matches("class=\"lead\"").count(), 2);
        assert_eq!(rendered.matches("style=\"text-align:center\"").count(), 2);
        assert_eq!(rendered.matches("role=\"note\"").count(), 2);
    }

    #[test]
    fn structured_text_edit_rejects_inline_and_image_boundaries_without_dirty_draft() {
        let source = r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>甲<em>强调</em>乙</p><img src="cover.png" alt="图"/><p>丙</p></body></html>"#;
        let (blocks, node_sources) =
            parse_xhtml_blocks("EPUB/one.xhtml", source).expect("parse unsafe structure fixture");
        let first_id = blocks[0].id.clone();
        let mut session = EditSession::new(
            "base".to_owned(),
            DocumentDraft::Epub(EpubDraft {
                source_path: PathBuf::from("book.epub"),
                base_fingerprint: "base".to_owned(),
                chapter_index: 0,
                resource_path: "EPUB/one.xhtml".to_owned(),
                source: Arc::from(source),
                next_block_id: blocks.len() as u64,
                blocks,
                node_sources,
                removed_source_ranges: Vec::new(),
                removed_images: Vec::new(),
                reserved_archive_paths: BTreeSet::new(),
                structured_text_groups: Vec::new(),
                next_text_group_id: 0,
            }),
            ViewAnchor {
                chapter_key: ChapterKey::new("EPUB/one.xhtml"),
                block_id: first_id.clone(),
                character_offset_utf16: 0,
                pixel_offset_from_viewport_top: 0.0,
            },
        );

        assert_eq!(
            session
                .apply(EditCommand::ReplaceText {
                    block_id: first_id.clone(),
                    text: "甲\n乙".to_owned(),
                    caret_utf16: 2,
                })
                .expect_err("inline paragraph split must reject"),
            EditError::UnsafeStructureEdit
        );
        assert_eq!(
            session
                .apply(EditCommand::JoinAdjacentText {
                    block_id: first_id,
                    direction: JoinDirection::Next,
                })
                .expect_err("image boundary join must reject"),
            EditError::ReadOnlyBlock
        );
        assert!(!session.is_dirty());
        assert_eq!(session.draft().blocks().len(), 3);
    }

    #[test]
    fn existing_image_alt_text_is_patched_without_rebuilding_the_tag() {
        let source = r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>正文</p><img class='hero' src="cover.png" data-note="keep" alt="旧图"/></body></html>"#;
        let (blocks, node_sources) =
            parse_xhtml_blocks("EPUB/one.xhtml", source).expect("parse XHTML image");
        let next_block_id = blocks.len() as u64;
        let image_id = blocks
            .iter()
            .find(|block| block.kind == ParagraphKind::Image)
            .expect("image block")
            .id
            .clone();
        let mut draft = EpubDraft {
            source_path: PathBuf::from("book.epub"),
            base_fingerprint: "base".to_owned(),
            chapter_index: 0,
            resource_path: "EPUB/one.xhtml".to_owned(),
            source: Arc::from(source),
            blocks,
            node_sources,
            removed_source_ranges: Vec::new(),
            removed_images: Vec::new(),
            reserved_archive_paths: BTreeSet::new(),
            next_block_id,
            structured_text_groups: Vec::new(),
            next_text_group_id: 0,
        };

        draft
            .set_image_alt(&image_id, "新 <图> & \"说明\"".to_owned())
            .expect("edit image alt text");
        let rendered = draft.render_xhtml();

        assert!(rendered.contains("class='hero'"));
        assert!(rendered.contains("src=\"cover.png\" data-note=\"keep\""));
        assert!(rendered.contains("alt=\"新 &lt;图&gt; &amp; &quot;说明&quot;\""));
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
                r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="style.css"/></head><body><h1>第一章</h1><p class="lead">原始正文 &amp; 实体</p><img src="cover.png" alt="正文插图"/><img src="https://tracker.invalid/pixel.png" alt="外部图片必须保持隔离"/></body></html>"#,
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

    fn image_deletion_fixture(shared: bool) -> (TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("image-deletion.epub");
        let mut writer = ZipWriter::new(File::create(&path).expect("create EPUB fixture"));
        let second_manifest = if shared {
            r#"<item id="second" href="second.xhtml" media-type="application/xhtml+xml"/>"#
        } else {
            ""
        };
        let second_spine = if shared {
            r#"<itemref idref="second"/>"#
        } else {
            ""
        };
        let package = format!(
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier>delete-image</dc:identifier><dc:title>图片删除测试</dc:title><dc:language>zh-CN</dc:language></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>{second_manifest}<item id="target-image" href="Images/target.png" media-type="image/png"/></manifest><spine><itemref idref="chapter"/>{second_spine}</spine></package>"#
        );
        let entries = [
            (
                "mimetype",
                "application/epub+zip",
                CompressionMethod::Stored,
            ),
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
                CompressionMethod::Deflated,
            ),
            ("EPUB/package.opf", &package, CompressionMethod::Deflated),
            (
                "EPUB/chapter.xhtml",
                r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>正文</p><img src="Images/target.png" alt="可删除图"/></body></html>"#,
                CompressionMethod::Deflated,
            ),
        ];
        for (name, content, compression) in entries {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(compression),
                )
                .expect("start deletion fixture entry");
            writer
                .write_all(content.as_bytes())
                .expect("write deletion fixture entry");
        }
        if shared {
            writer
                .start_file(
                    "EPUB/second.xhtml",
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .expect("start second chapter");
            writer
                .write_all(
                    r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>共享正文</p><img src="Images/target.png" alt="共享图"/></body></html>"#
                        .as_bytes(),
                )
                .expect("write second chapter");
        }
        writer
            .start_file(
                "EPUB/Images/target.png",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("start target image");
        writer.write_all(tiny_png()).expect("write target image");
        writer.finish().expect("finish deletion fixture");
        (directory, path)
    }

    fn tiny_png() -> &'static [u8] {
        &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 8, 215, 99, 248, 207,
            192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
            96, 130,
        ]
    }
}
