use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{EncoderResult, GB18030, GBK};
use serde::{Deserialize, Serialize};

use crate::core::CoreError;

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
const UTF16_LE_BOM: &[u8] = &[0xFF, 0xFE];
const UTF16_BE_BOM: &[u8] = &[0xFE, 0xFF];

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TextEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Gbk,
    Gb18030,
}

impl TextEncoding {
    pub fn label(self, has_bom: bool) -> &'static str {
        match (self, has_bom) {
            (Self::Utf8, true) => "UTF-8 BOM",
            (Self::Utf8, false) => "UTF-8",
            (Self::Utf16Le, _) => "UTF-16 LE",
            (Self::Utf16Be, _) => "UTF-16 BE",
            (Self::Gbk, _) => "GBK",
            (Self::Gb18030, _) => "GB18030",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LineEnding {
    None,
    Lf,
    Crlf,
    Cr,
    Mixed,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "无换行",
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
            Self::Cr => "CR",
            Self::Mixed => "混合→主格式",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveTextOptions {
    pub encoding: Option<TextEncoding>,
    pub has_bom: Option<bool>,
    pub line_ending: Option<LineEnding>,
}

impl SaveTextOptions {
    pub const PRESERVE: Self = Self {
        encoding: None,
        has_bom: None,
        line_ending: None,
    };
}

#[derive(Debug)]
pub(crate) struct DecodedText {
    pub content: String,
    pub encoding: TextEncoding,
    pub has_bom: bool,
    pub line_ending: LineEnding,
    pub primary_line_ending: LineEnding,
}

pub(crate) fn decode_text(bytes: &[u8]) -> Result<DecodedText, CoreError> {
    let (content, encoding, has_bom) = if let Some(payload) = bytes.strip_prefix(UTF8_BOM) {
        (decode_utf8(payload)?, TextEncoding::Utf8, true)
    } else if let Some(payload) = bytes.strip_prefix(UTF16_LE_BOM) {
        (decode_utf16(payload, true)?, TextEncoding::Utf16Le, true)
    } else if let Some(payload) = bytes.strip_prefix(UTF16_BE_BOM) {
        (decode_utf16(payload, false)?, TextEncoding::Utf16Be, true)
    } else if std::str::from_utf8(bytes).is_ok() {
        (decode_utf8(bytes)?, TextEncoding::Utf8, false)
    } else {
        let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
        detector.feed(bytes, true);
        let guessed = detector.guess(None, Utf8Detection::Deny);
        if guessed != GBK && guessed != GB18030 {
            return Err(CoreError::Validation(
                "无法可靠判断这个 TXT 文件的编码。".to_owned(),
            ));
        }
        let selected = if guessed == GB18030 || contains_gb18030_four_byte_sequence(bytes) {
            GB18030
        } else {
            GBK
        };
        let content = selected
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|text| text.into_owned())
            .ok_or_else(decode_error)?;
        let encoding = if selected == GBK {
            TextEncoding::Gbk
        } else {
            TextEncoding::Gb18030
        };
        (content, encoding, false)
    };
    let (line_ending, primary_line_ending) = analyze_line_endings(&content);
    Ok(DecodedText {
        content: normalize_line_endings(&content),
        encoding,
        has_bom,
        line_ending,
        primary_line_ending,
    })
}

fn contains_gb18030_four_byte_sequence(bytes: &[u8]) -> bool {
    bytes.windows(4).any(|bytes| {
        matches!(bytes[0], 0x81..=0xfe)
            && bytes[1].is_ascii_digit()
            && matches!(bytes[2], 0x81..=0xfe)
            && bytes[3].is_ascii_digit()
    })
}

pub(crate) fn encode_text(
    content: &str,
    encoding: TextEncoding,
    has_bom: bool,
    requested_line_ending: LineEnding,
) -> Result<Vec<u8>, CoreError> {
    match (encoding, has_bom) {
        (TextEncoding::Utf16Le | TextEncoding::Utf16Be, false) => {
            return Err(CoreError::Validation(
                "UTF-16 保存必须保留 BOM。".to_owned(),
            ));
        }
        (TextEncoding::Gbk | TextEncoding::Gb18030, true) => {
            return Err(CoreError::Validation(
                "GBK 与 GB18030 不使用 BOM。".to_owned(),
            ));
        }
        _ => {}
    }
    let normalized = normalize_line_endings(content);
    let converted = match requested_line_ending {
        LineEnding::Crlf => normalized.replace('\n', "\r\n"),
        LineEnding::Cr => normalized.replace('\n', "\r"),
        LineEnding::Lf | LineEnding::None => normalized,
        LineEnding::Mixed => {
            return Err(CoreError::Validation(
                "混合换行文件保存前需要选择 CRLF、LF 或 CR。".to_owned(),
            ));
        }
    };
    match encoding {
        TextEncoding::Utf8 => {
            let mut bytes = Vec::with_capacity(converted.len() + usize::from(has_bom) * 3);
            if has_bom {
                bytes.extend_from_slice(UTF8_BOM);
            }
            bytes.extend_from_slice(converted.as_bytes());
            Ok(bytes)
        }
        TextEncoding::Utf16Le => Ok(encode_utf16(&converted, true)),
        TextEncoding::Utf16Be => Ok(encode_utf16(&converted, false)),
        TextEncoding::Gbk => encode_legacy(&converted, GBK),
        TextEncoding::Gb18030 => encode_legacy(&converted, GB18030),
    }
}

fn analyze_line_endings(content: &str) -> (LineEnding, LineEnding) {
    let bytes = content.as_bytes();
    let (mut crlf, mut lf, mut cr) = (0usize, 0usize, 0usize);
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                crlf += 1;
                index += 2;
            }
            b'\r' => {
                cr += 1;
                index += 1;
            }
            b'\n' => {
                lf += 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    let kinds = usize::from(crlf > 0) + usize::from(lf > 0) + usize::from(cr > 0);
    let detected = match kinds {
        0 => LineEnding::None,
        1 if crlf > 0 => LineEnding::Crlf,
        1 if lf > 0 => LineEnding::Lf,
        1 => LineEnding::Cr,
        _ => LineEnding::Mixed,
    };
    let primary = [
        (LineEnding::Crlf, crlf),
        (LineEnding::Lf, lf),
        (LineEnding::Cr, cr),
    ]
    .into_iter()
    .max_by_key(|(_, count)| *count)
    .filter(|(_, count)| *count > 0)
    .map_or(LineEnding::None, |(ending, _)| ending);
    (detected, primary)
}

fn normalize_line_endings(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\r', "\n")
}

fn decode_utf8(bytes: &[u8]) -> Result<String, CoreError> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| decode_error())
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Result<String, CoreError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(decode_error());
    }
    let units = bytes.chunks_exact(2).map(|pair| {
        let pair = [pair[0], pair[1]];
        if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    });
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|_| decode_error())
}

fn encode_utf16(content: &str, little_endian: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(content.encode_utf16().count() * 2 + 2);
    bytes.extend_from_slice(if little_endian {
        UTF16_LE_BOM
    } else {
        UTF16_BE_BOM
    });
    for unit in content.encode_utf16() {
        let encoded = if little_endian {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        };
        bytes.extend_from_slice(&encoded);
    }
    bytes
}

fn encode_legacy(
    content: &str,
    codec: &'static encoding_rs::Encoding,
) -> Result<Vec<u8>, CoreError> {
    let mut encoder = codec.new_encoder();
    let capacity = encoder
        .max_buffer_length_from_utf8_without_replacement(content.len())
        .ok_or_else(|| CoreError::Validation("TXT 编码后的文件过大。".to_owned()))?;
    let mut bytes = Vec::with_capacity(capacity);
    let (result, read) =
        encoder.encode_from_utf8_to_vec_without_replacement(content, &mut bytes, true);
    match result {
        EncoderResult::InputEmpty if read == content.len() => Ok(bytes),
        EncoderResult::Unmappable(character) => Err(CoreError::Validation(format!(
            "目标编码无法表示字符“{character}”，请改用 UTF-8。"
        ))),
        EncoderResult::OutputFull | EncoderResult::InputEmpty => Err(CoreError::Validation(
            "TXT 编码没有完整处理输入内容。".to_owned(),
        )),
    }
}

fn decode_error() -> CoreError {
    CoreError::Validation("TXT 包含当前编码无法解码的字节。".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_and_normalizes_line_endings() {
        let decoded = decode_text(b"first\r\nsecond\r\n").expect("decode");
        assert_eq!(decoded.line_ending, LineEnding::Crlf);
        assert_eq!(decoded.primary_line_ending, LineEnding::Crlf);
        assert_eq!(decoded.content, "first\nsecond\n");
    }

    #[test]
    fn gbk_refuses_unrepresentable_characters() {
        let error = encode_text("中文 😀", TextEncoding::Gbk, false, LineEnding::Lf)
            .expect_err("emoji is not GBK representable");
        assert!(error.to_string().contains("UTF-8"));
    }

    #[test]
    fn gb18030_four_byte_sequences_are_not_downgraded_to_gbk() {
        let bytes = encode_text("𠀀", TextEncoding::Gb18030, false, LineEnding::Lf)
            .expect("encode GB18030 supplementary character");
        let decoded = decode_text(&bytes).expect("decode GB18030");
        assert_eq!(decoded.encoding, TextEncoding::Gb18030);
        assert_eq!(decoded.content, "𠀀");
    }
}
