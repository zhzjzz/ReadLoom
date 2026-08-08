use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{GB18030, GBK};

use crate::{domain::text_document::TextEncoding, error::AppError};

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
const UTF16_LE_BOM: &[u8] = &[0xFF, 0xFE];
const UTF16_BE_BOM: &[u8] = &[0xFE, 0xFF];

#[derive(Debug, PartialEq, Eq)]
pub struct DecodedText {
    pub content: String,
    pub encoding: TextEncoding,
    pub has_bom: bool,
}

pub fn decode_text(
    bytes: &[u8],
    encoding_override: Option<TextEncoding>,
) -> Result<DecodedText, AppError> {
    if let Some(encoding) = encoding_override {
        return decode_as(bytes, encoding, bom_matches(bytes, encoding));
    }

    if bytes.starts_with(UTF8_BOM) {
        return decode_utf8(&bytes[UTF8_BOM.len()..], true);
    }
    if bytes.starts_with(UTF16_LE_BOM) {
        return decode_utf16(&bytes[UTF16_LE_BOM.len()..], true, true);
    }
    if bytes.starts_with(UTF16_BE_BOM) {
        return decode_utf16(&bytes[UTF16_BE_BOM.len()..], false, true);
    }
    if std::str::from_utf8(bytes).is_ok() {
        return decode_utf8(bytes, false);
    }

    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let guessed = detector.guess(None, Utf8Detection::Deny);

    if guessed == GBK || guessed == GB18030 {
        let encoding = if guessed == GB18030 {
            TextEncoding::Gb18030
        } else {
            TextEncoding::Gbk
        };
        return decode_as(bytes, encoding, false);
    }

    Err(AppError::validation(
        "LOW_ENCODING_CONFIDENCE",
        "无法可靠判断这个 TXT 文件的编码。",
        "请选择 UTF-8、UTF-16、GBK 或 GB18030 后重新打开。",
    ))
}

fn decode_as(bytes: &[u8], encoding: TextEncoding, has_bom: bool) -> Result<DecodedText, AppError> {
    let payload = strip_matching_bom(bytes, encoding);
    match encoding {
        TextEncoding::Utf8 => decode_utf8(payload, has_bom),
        TextEncoding::Utf16Le => decode_utf16(payload, true, has_bom),
        TextEncoding::Utf16Be => decode_utf16(payload, false, has_bom),
        TextEncoding::Gbk => decode_legacy(payload, GBK, encoding),
        TextEncoding::Gb18030 => decode_legacy(payload, GB18030, encoding),
    }
}

fn decode_utf8(bytes: &[u8], has_bom: bool) -> Result<DecodedText, AppError> {
    let content = std::str::from_utf8(bytes).map_err(|_| {
        AppError::validation(
            "DECODE_FAILED",
            "文件不是有效的 UTF-8 文本。",
            "请选择其他编码后重新打开。",
        )
    })?;
    Ok(DecodedText {
        content: content.to_owned(),
        encoding: TextEncoding::Utf8,
        has_bom,
    })
}

fn decode_utf16(bytes: &[u8], little_endian: bool, has_bom: bool) -> Result<DecodedText, AppError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(AppError::validation(
            "DECODE_FAILED",
            "UTF-16 文件包含不完整的双字节字符。",
            "检查文件是否损坏，或选择其他编码。",
        ));
    }

    let units = bytes.chunks_exact(2).map(|pair| {
        let pair = [pair[0], pair[1]];
        if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    });
    let content = char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|_| {
            AppError::validation(
                "DECODE_FAILED",
                "UTF-16 文件包含无效的代理项。",
                "检查文件是否损坏，或选择其他编码。",
            )
        })?;

    Ok(DecodedText {
        content,
        encoding: if little_endian {
            TextEncoding::Utf16Le
        } else {
            TextEncoding::Utf16Be
        },
        has_bom,
    })
}

fn decode_legacy(
    bytes: &[u8],
    codec: &'static encoding_rs::Encoding,
    encoding: TextEncoding,
) -> Result<DecodedText, AppError> {
    let content = codec
        .decode_without_bom_handling_and_without_replacement(bytes)
        .ok_or_else(|| {
            AppError::validation(
                "DECODE_FAILED",
                "文件包含所选编码无法解码的字节。",
                "请选择其他编码后重新打开。",
            )
        })?;

    Ok(DecodedText {
        content: content.into_owned(),
        encoding,
        has_bom: false,
    })
}

fn bom_matches(bytes: &[u8], encoding: TextEncoding) -> bool {
    match encoding {
        TextEncoding::Utf8 => bytes.starts_with(UTF8_BOM),
        TextEncoding::Utf16Le => bytes.starts_with(UTF16_LE_BOM),
        TextEncoding::Utf16Be => bytes.starts_with(UTF16_BE_BOM),
        TextEncoding::Gbk | TextEncoding::Gb18030 => false,
    }
}

fn strip_matching_bom(bytes: &[u8], encoding: TextEncoding) -> &[u8] {
    match encoding {
        TextEncoding::Utf8 if bytes.starts_with(UTF8_BOM) => &bytes[UTF8_BOM.len()..],
        TextEncoding::Utf16Le if bytes.starts_with(UTF16_LE_BOM) => &bytes[UTF16_LE_BOM.len()..],
        TextEncoding::Utf16Be if bytes.starts_with(UTF16_BE_BOM) => &bytes[UTF16_BE_BOM.len()..],
        _ => bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_utf8_with_and_without_bom_including_empty_payloads() {
        let plain = decode_text("中文 English 😀".as_bytes(), None).expect("UTF-8");
        let bom = decode_text(b"\xEF\xBB\xBFtext", None).expect("UTF-8 BOM");
        let empty = decode_text(b"", None).expect("empty UTF-8");
        let only_bom = decode_text(UTF8_BOM, None).expect("only BOM");

        assert_eq!(plain.encoding, TextEncoding::Utf8);
        assert!(!plain.has_bom);
        assert_eq!(plain.content, "中文 English 😀");
        assert!(bom.has_bom);
        assert_eq!(bom.content, "text");
        assert_eq!(empty.content, "");
        assert_eq!(only_bom.content, "");
        assert!(only_bom.has_bom);
    }

    #[test]
    fn opens_utf16_little_and_big_endian_bom() {
        let little = decode_text(&[0xFF, 0xFE, 0x2D, 0x4E, 0x3D, 0xD8, 0x00, 0xDE], None)
            .expect("UTF-16 LE");
        let big = decode_text(&[0xFE, 0xFF, 0x4E, 0x2D, 0xD8, 0x3D, 0xDE, 0x00], None)
            .expect("UTF-16 BE");

        assert_eq!(little.content, "中😀");
        assert_eq!(little.encoding, TextEncoding::Utf16Le);
        assert!(little.has_bom);
        assert_eq!(big.content, "中😀");
        assert_eq!(big.encoding, TextEncoding::Utf16Be);
        assert!(big.has_bom);
    }

    #[test]
    fn opens_common_gbk_chinese_when_selected() {
        let decoded =
            decode_text(&[0xD6, 0xD0, 0xCE, 0xC4], Some(TextEncoding::Gbk)).expect("GBK Chinese");

        assert_eq!(decoded.content, "中文");
        assert_eq!(decoded.encoding, TextEncoding::Gbk);
        assert!(!decoded.has_bom);
    }

    #[test]
    fn invalid_selected_encoding_returns_decode_failed() {
        let error = decode_text(&[0xFF], Some(TextEncoding::Utf8)).expect_err("invalid UTF-8");
        assert_eq!(error.to_dto().code, "DECODE_FAILED");
    }
}
