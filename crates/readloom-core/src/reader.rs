#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphKind {
    Heading,
    Paragraph,
    Blank,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingParagraph {
    pub block_id: crate::BlockId,
    pub editable: bool,
    pub kind: ParagraphKind,
    pub text: String,
    pub source_start: usize,
    pub source_end: usize,
    pub source_start_utf16: usize,
    pub source_end_utf16: usize,
    pub line_number: usize,
    pub chapter_index: usize,
    pub paragraph_index: usize,
    pub image_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxtChapter {
    pub title: String,
    pub paragraph_index: usize,
    pub source_start: usize,
    pub line_number: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub paragraph_index: usize,
    pub chapter_index: usize,
    pub character_offset_in_paragraph: usize,
    pub preview: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TextReadingLocator {
    pub version: u8,
    pub character_offset: usize,
    pub line_number: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chapter_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paragraph_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_offset_in_paragraph: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ReaderDocument {
    path: Option<PathBuf>,
    title: String,
    content: String,
    paragraphs: Vec<ReadingParagraph>,
    chapters: Vec<TxtChapter>,
    encoding: TextEncoding,
    has_bom: bool,
    line_ending: LineEnding,
    primary_line_ending: LineEnding,
    source_fingerprint: Option<String>,
}

impl ReaderDocument {
    pub fn from_text(title: impl Into<String>, content: String) -> Self {
        Self::from_text_with_settings(
            title,
            content,
            &crate::TxtSettings::default(),
            crate::DEFAULT_TXT_CHAPTER_PATTERN,
        )
    }

    pub fn from_text_with_settings(
        title: impl Into<String>,
        content: String,
        settings: &crate::TxtSettings,
        chapter_pattern: &str,
    ) -> Self {
        let mut paragraphs = Vec::new();
        let mut chapters = Vec::new();
        let mut source_start = 0;
        let mut source_start_utf16 = 0;
        let mut chapter_index = 0;
        let heading_pattern = regex::Regex::new(chapter_pattern)
            .expect("chapter pattern is validated before document construction");

        for (line_index, source_line) in content.split_inclusive('\n').enumerate() {
            let line = source_line.strip_suffix('\n').unwrap_or(source_line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let source_end = source_start + line.len();
            let source_end_utf16 = source_start_utf16 + line.encode_utf16().count();
            let trimmed = line.trim();
            let kind = if trimmed.is_empty() {
                ParagraphKind::Blank
            } else if heading_pattern.is_match(trimmed) {
                if chapters.is_empty() && !paragraphs.is_empty() {
                    chapters.push(TxtChapter {
                        title: "开篇".to_owned(),
                        paragraph_index: 0,
                        source_start: 0,
                        line_number: 1,
                    });
                    chapter_index = 1;
                } else if !chapters.is_empty() {
                    chapter_index += 1;
                }
                chapters.push(TxtChapter {
                    title: trimmed.to_owned(),
                    paragraph_index: paragraphs.len(),
                    source_start,
                    line_number: line_index + 1,
                });
                ParagraphKind::Heading
            } else {
                ParagraphKind::Paragraph
            };
            if kind == ParagraphKind::Blank {
                match settings.blank_lines {
                    crate::TxtBlankLines::Remove => {
                        source_start += source_line.len();
                        source_start_utf16 += source_line.encode_utf16().count();
                        continue;
                    }
                    crate::TxtBlankLines::Single
                        if paragraphs
                            .last()
                            .is_some_and(|paragraph: &ReadingParagraph| {
                                paragraph.kind == ParagraphKind::Blank
                            }) =>
                    {
                        source_start += source_line.len();
                        source_start_utf16 += source_line.encode_utf16().count();
                        continue;
                    }
                    _ => {}
                }
            }
            if settings.merge_wrapped_lines
                && kind == ParagraphKind::Paragraph
                && paragraphs
                    .last()
                    .is_some_and(|paragraph: &ReadingParagraph| {
                        paragraph.kind == ParagraphKind::Paragraph
                            && !ends_sentence(&paragraph.text)
                    })
            {
                let previous = paragraphs.last_mut().expect("checked above");
                previous.text.push_str(trimmed);
                previous.source_end = source_end;
                previous.source_end_utf16 = source_end_utf16;
                source_start += source_line.len();
                source_start_utf16 += source_line.encode_utf16().count();
                continue;
            }
            let text = if kind == ParagraphKind::Paragraph
                && matches!(settings.leading_indent, crate::TxtLeadingIndent::Preserve)
            {
                line.trim_end().to_owned()
            } else {
                trimmed.to_owned()
            };
            paragraphs.push(ReadingParagraph {
                block_id: crate::BlockId::new(format!("txt:block:{}", paragraphs.len())),
                editable: true,
                kind,
                text,
                source_start,
                source_end,
                source_start_utf16,
                source_end_utf16,
                line_number: line_index + 1,
                chapter_index,
                paragraph_index: paragraphs.len(),
                image_index: None,
            });
            source_start += source_line.len();
            source_start_utf16 += source_line.encode_utf16().count();
        }

        if chapters.is_empty() && !paragraphs.is_empty() {
            chapters.push(TxtChapter {
                title: "全文".to_owned(),
                paragraph_index: 0,
                source_start: 0,
                line_number: 1,
            });
        }

        Self {
            path: None,
            title: title.into(),
            content,
            paragraphs,
            chapters,
            encoding: TextEncoding::Utf8,
            has_bom: false,
            line_ending: LineEnding::Lf,
            primary_line_ending: LineEnding::Lf,
            source_fingerprint: None,
        }
    }

    pub(crate) fn from_opened(
        path: PathBuf,
        title: String,
        decoded: DecodedText,
        source_fingerprint: String,
        settings: &crate::TxtSettings,
        chapter_pattern: &str,
    ) -> Self {
        let mut document =
            Self::from_text_with_settings(title, decoded.content, settings, chapter_pattern);
        document.path = Some(path);
        document.encoding = decoded.encoding;
        document.has_bom = decoded.has_bom;
        document.line_ending = decoded.line_ending;
        document.primary_line_ending = decoded.primary_line_ending;
        document.source_fingerprint = Some(source_fingerprint);
        document
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    pub fn has_bom(&self) -> bool {
        self.has_bom
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    pub fn primary_line_ending(&self) -> LineEnding {
        self.primary_line_ending
    }

    pub fn fingerprint(&self) -> Option<&str> {
        self.source_fingerprint.as_deref()
    }

    pub(crate) fn source_fingerprint(&self) -> Option<&str> {
        self.fingerprint()
    }

    pub(crate) fn with_encoding_hint(mut self, encoding: TextEncoding) -> Self {
        self.encoding = encoding;
        self
    }

    pub fn paragraphs(&self) -> &[ReadingParagraph] {
        &self.paragraphs
    }

    pub fn chapters(&self) -> &[TxtChapter] {
        &self.chapters
    }

    pub fn search(&self, query: &str, maximum: usize) -> Vec<SearchHit> {
        let query = query.trim();
        if query.is_empty() || maximum == 0 {
            return Vec::new();
        }
        self.paragraphs
            .iter()
            .filter(|paragraph| paragraph.kind != ParagraphKind::Blank)
            .filter_map(|paragraph| {
                let byte_offset = paragraph.text.find(query)?;
                let character_offset_in_paragraph =
                    paragraph.text[..byte_offset].encode_utf16().count();
                Some(SearchHit {
                    paragraph_index: paragraph.paragraph_index,
                    chapter_index: paragraph.chapter_index,
                    character_offset_in_paragraph,
                    preview: paragraph.text.clone(),
                })
            })
            .take(maximum.min(5_000))
            .collect()
    }

    pub fn locator_for_paragraph(
        &self,
        paragraph_index: usize,
        character_offset_in_paragraph: usize,
    ) -> TextReadingLocator {
        let paragraph_index = paragraph_index.min(self.paragraphs.len().saturating_sub(1));
        let paragraph = self.paragraphs.get(paragraph_index);
        let Some(paragraph) = paragraph else {
            return TextReadingLocator {
                version: 1,
                character_offset: 0,
                line_number: 1,
                chapter_index: Some(0),
                paragraph_index: Some(0),
                character_offset_in_paragraph: Some(0),
            };
        };
        let offset = character_offset_in_paragraph.min(paragraph.text.encode_utf16().count());
        let chapter_start = self
            .chapters
            .get(paragraph.chapter_index)
            .map_or(0, |chapter| chapter.paragraph_index);
        TextReadingLocator {
            version: 1,
            character_offset: paragraph.source_start_utf16.saturating_add(offset),
            line_number: paragraph.line_number,
            chapter_index: Some(paragraph.chapter_index),
            paragraph_index: Some(paragraph_index.saturating_sub(chapter_start)),
            character_offset_in_paragraph: Some(offset),
        }
    }

    pub fn resolve_locator(&self, locator: &TextReadingLocator) -> usize {
        if self.paragraphs.is_empty() {
            return 0;
        }
        if locator.version == 1
            && let (Some(chapter_index), Some(paragraph_index)) =
                (locator.chapter_index, locator.paragraph_index)
        {
            let chapter_start = self
                .chapters
                .get(chapter_index)
                .map_or(0, |chapter| chapter.paragraph_index);
            let candidate = chapter_start.saturating_add(paragraph_index);
            if let Some(paragraph) = self.paragraphs.get(candidate)
                && paragraph.chapter_index == chapter_index
            {
                return candidate;
            }
        }
        self.paragraphs
            .partition_point(|paragraph| paragraph.source_end_utf16 < locator.character_offset)
            .min(self.paragraphs.len() - 1)
    }
}

fn ends_sentence(value: &str) -> bool {
    value
        .trim_end()
        .chars()
        .next_back()
        .is_some_and(|character| {
            matches!(
                character,
                '。' | '！' | '？' | '；' | '…' | '.' | '!' | '?' | ';' | '”' | '’'
            )
        })
}
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::text_codec::{DecodedText, LineEnding, TextEncoding};
