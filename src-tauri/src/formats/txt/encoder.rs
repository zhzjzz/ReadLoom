use encoding_rs::{EncoderResult, GB18030, GBK};

use crate::{
    domain::text_document::{LineEnding, SaveLineEnding, TextEncoding},
    error::AppError,
};

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
const UTF16_LE_BOM: &[u8] = &[0xFF, 0xFE];
const UTF16_BE_BOM: &[u8] = &[0xFE, 0xFF];

#[derive(Debug, PartialEq, Eq)]
pub struct EncodedText {
    pub bytes: Vec<u8>,
    pub line_ending: LineEnding,
    pub primary_line_ending: LineEnding,
}

pub fn encode_text(
    content: &str,
    encoding: TextEncoding,
    has_bom: bool,
    requested_line_ending: SaveLineEnding,
    detected_line_ending: LineEnding,
    primary_line_ending: LineEnding,
) -> Result<EncodedText, AppError> {
    validate_bom(encoding, has_bom)?;
    let target_line_ending = resolve_line_ending(
        requested_line_ending,
        detected_line_ending,
        primary_line_ending,
    )?;
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let converted = match target_line_ending {
        LineEnding::Crlf => normalized.replace('\n', "\r\n"),
        LineEnding::Cr => normalized.replace('\n', "\r"),
        LineEnding::Lf | LineEnding::None => normalized,
        LineEnding::Mixed => unreachable!("mixed is never a save target"),
    };

    let bytes = match encoding {
        TextEncoding::Utf8 => {
            let mut bytes = Vec::with_capacity(converted.len() + usize::from(has_bom) * 3);
            if has_bom {
                bytes.extend_from_slice(UTF8_BOM);
            }
            bytes.extend_from_slice(converted.as_bytes());
            bytes
        }
        TextEncoding::Utf16Le => encode_utf16(&converted, true),
        TextEncoding::Utf16Be => encode_utf16(&converted, false),
        TextEncoding::Gbk => encode_legacy(&converted, GBK)?,
        TextEncoding::Gb18030 => encode_legacy(&converted, GB18030)?,
    };

    let line_ending = if converted.contains('\r') || converted.contains('\n') {
        target_line_ending
    } else {
        LineEnding::None
    };
    Ok(EncodedText {
        bytes,
        line_ending,
        primary_line_ending: target_line_ending,
    })
}

fn validate_bom(encoding: TextEncoding, has_bom: bool) -> Result<(), AppError> {
    match (encoding, has_bom) {
        (TextEncoding::Utf16Le | TextEncoding::Utf16Be, false) => Err(AppError::validation(
            "UNSUPPORTED_ENCODING",
            "UTF-16 保存必须包含 BOM。",
            "保留 BOM，或改用 UTF-8。",
        )),
        (TextEncoding::Gbk | TextEncoding::Gb18030, true) => Err(AppError::validation(
            "UNSUPPORTED_ENCODING",
            "GBK 与 GB18030 不使用 BOM。",
            "关闭 BOM 后重试。",
        )),
        _ => Ok(()),
    }
}

fn resolve_line_ending(
    requested: SaveLineEnding,
    detected: LineEnding,
    primary: LineEnding,
) -> Result<LineEnding, AppError> {
    match requested {
        SaveLineEnding::Crlf => Ok(LineEnding::Crlf),
        SaveLineEnding::Lf => Ok(LineEnding::Lf),
        SaveLineEnding::Preserve if detected == LineEnding::Mixed => Err(AppError::validation(
            "LINE_ENDING_SELECTION_REQUIRED",
            "原文件包含混合换行符，保存前需要选择统一格式。",
            "请选择 CRLF 或 LF。",
        )),
        SaveLineEnding::Preserve if primary == LineEnding::None => Ok(LineEnding::Lf),
        SaveLineEnding::Preserve => Ok(primary),
    }
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
) -> Result<Vec<u8>, AppError> {
    let mut encoder = codec.new_encoder();
    let capacity = encoder
        .max_buffer_length_from_utf8_without_replacement(content.len())
        .ok_or_else(|| AppError::internal("ENCODE_FAILED", "legacy output size overflow"))?;
    let mut bytes = Vec::with_capacity(capacity);
    let (result, read) =
        encoder.encode_from_utf8_to_vec_without_replacement(content, &mut bytes, true);
    match result {
        EncoderResult::InputEmpty if read == content.len() => Ok(bytes),
        EncoderResult::Unmappable(character) => Err(AppError::validation(
            "UNREPRESENTABLE_CHARACTERS",
            format!("目标编码无法表示字符“{character}”。"),
            "请改用 UTF-8 后再保存。",
        )),
        EncoderResult::OutputFull | EncoderResult::InputEmpty => Err(AppError::internal(
            "ENCODE_FAILED",
            "legacy encoder did not consume the input",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gbk_refuses_emoji_without_replacement() {
        let error = encode_text(
            "中文 😀",
            TextEncoding::Gbk,
            false,
            SaveLineEnding::Lf,
            LineEnding::Lf,
            LineEnding::Lf,
        )
        .expect_err("emoji is not representable in GBK");

        assert_eq!(error.to_dto().code, "UNREPRESENTABLE_CHARACTERS");
    }

    #[test]
    fn utf_encodings_preserve_bom_and_selected_line_endings() {
        let utf8_bom = encode_text(
            "中\n",
            TextEncoding::Utf8,
            true,
            SaveLineEnding::Crlf,
            LineEnding::Lf,
            LineEnding::Lf,
        )
        .expect("UTF-8 BOM");
        let utf16_le = encode_text(
            "中\n",
            TextEncoding::Utf16Le,
            true,
            SaveLineEnding::Lf,
            LineEnding::Lf,
            LineEnding::Lf,
        )
        .expect("UTF-16 LE");
        let utf16_be = encode_text(
            "中\n",
            TextEncoding::Utf16Be,
            true,
            SaveLineEnding::Lf,
            LineEnding::Lf,
            LineEnding::Lf,
        )
        .expect("UTF-16 BE");

        assert!(utf8_bom.bytes.starts_with(UTF8_BOM));
        assert!(utf8_bom.bytes.ends_with(b"\r\n"));
        assert!(utf16_le.bytes.starts_with(UTF16_LE_BOM));
        assert!(utf16_be.bytes.starts_with(UTF16_BE_BOM));
    }

    #[test]
    fn mixed_line_endings_require_an_explicit_save_choice() {
        let error = encode_text(
            "a\nb\n",
            TextEncoding::Utf8,
            false,
            SaveLineEnding::Preserve,
            LineEnding::Mixed,
            LineEnding::Crlf,
        )
        .expect_err("mixed needs explicit choice");

        assert_eq!(error.to_dto().code, "LINE_ENDING_SELECTION_REQUIRED");
    }

    #[test]
    fn gbk_and_gb18030_round_trip_supported_text_without_replacement() {
        for encoding in [TextEncoding::Gbk, TextEncoding::Gb18030] {
            let encoded = encode_text(
                "中文 English",
                encoding,
                false,
                SaveLineEnding::Lf,
                LineEnding::Lf,
                LineEnding::Lf,
            )
            .expect("legacy encoding");
            let decoded = crate::formats::txt::decode_text(&encoded.bytes, Some(encoding))
                .expect("legacy decode");
            assert_eq!(decoded.content, "中文 English");
        }
    }

    #[test]
    fn adding_a_line_to_a_previously_single_line_file_reports_lf() {
        let encoded = encode_text(
            "first\nsecond",
            TextEncoding::Utf8,
            false,
            SaveLineEnding::Preserve,
            LineEnding::None,
            LineEnding::None,
        )
        .expect("save new line");

        assert_eq!(encoded.bytes, b"first\nsecond");
        assert_eq!(encoded.line_ending, LineEnding::Lf);
        assert_eq!(encoded.primary_line_ending, LineEnding::Lf);
    }
}
