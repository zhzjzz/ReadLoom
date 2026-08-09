use std::{fs, path::Path, sync::Arc};

use crate::{error::AppError, infrastructure::archive::archive_limits::ArchiveLimits};

const MAXIMUM_COVER_EDGE: u32 = 20_000;
const MAXIMUM_COVER_PIXELS: u64 = 100_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedCover {
    pub bytes: Arc<Vec<u8>>,
    pub media_type: String,
    pub extension: String,
    pub width: u32,
    pub height: u32,
    pub content_hash: String,
}

pub(crate) fn load_cover(path: &Path, limits: ArchiveLimits) -> Result<ValidatedCover, AppError> {
    let metadata = fs::metadata(path).map_err(|_| {
        AppError::validation(
            "COVER_FILE_NOT_FOUND",
            "找不到所选封面文件。",
            "请重新选择 PNG、JPEG 或 WebP 图片。",
        )
    })?;
    if !metadata.is_file() {
        return Err(invalid_cover());
    }
    if metadata.len() > limits.maximum_image_bytes {
        return Err(AppError::validation(
            "COVER_TOO_LARGE",
            "封面文件超出安全大小限制。",
            format!(
                "请选择不超过 {} MiB 的图片。",
                limits.maximum_image_bytes / 1024 / 1024
            ),
        ));
    }
    let bytes = fs::read(path).map_err(|_| invalid_cover())?;
    let (media_type, extension, width, height) = inspect_cover(&bytes)?;
    let selected_extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let extension_matches = match extension {
        "jpg" => matches!(selected_extension.as_str(), "jpg" | "jpeg"),
        value => selected_extension == value,
    };
    if !extension_matches {
        return Err(AppError::validation(
            "UNSUPPORTED_COVER_TYPE",
            "封面扩展名与图片真实类型不一致。",
            "请修正文件扩展名，或选择其他图片。",
        ));
    }
    validate_dimensions(width, height)?;
    Ok(ValidatedCover {
        content_hash: blake3::hash(&bytes).to_hex().to_string(),
        bytes: Arc::new(bytes),
        media_type: media_type.to_owned(),
        extension: extension.to_owned(),
        width,
        height,
    })
}

pub(crate) fn inspect_cover(
    bytes: &[u8],
) -> Result<(&'static str, &'static str, u32, u32), AppError> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        if bytes.len() < 33 || &bytes[12..16] != b"IHDR" {
            return Err(invalid_cover());
        }
        let width = u32::from_be_bytes(bytes[16..20].try_into().expect("PNG width slice"));
        let height = u32::from_be_bytes(bytes[20..24].try_into().expect("PNG height slice"));
        return Ok(("image/png", "png", width, height));
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        let (width, height) = jpeg_dimensions(bytes)?;
        return Ok(("image/jpeg", "jpg", width, height));
    }
    if bytes.len() >= 30 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        let (width, height) = webp_dimensions(bytes)?;
        return Ok(("image/webp", "webp", width, height));
    }
    Err(AppError::validation(
        "UNSUPPORTED_COVER_TYPE",
        "Readloom 只支持 PNG、JPEG 和 WebP 封面。",
        "请选择受支持的图片格式。",
    ))
}

fn jpeg_dimensions(bytes: &[u8]) -> Result<(u32, u32), AppError> {
    let mut index = 2;
    while index + 4 <= bytes.len() {
        while index < bytes.len() && bytes[index] != 0xff {
            index += 1;
        }
        while index < bytes.len() && bytes[index] == 0xff {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let marker = bytes[index];
        index += 1;
        if matches!(marker, 0xd8 | 0xd9 | 0x01) || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if index + 2 > bytes.len() {
            break;
        }
        let length = u16::from_be_bytes([bytes[index], bytes[index + 1]]) as usize;
        if length < 2 || index + length > bytes.len() {
            return Err(invalid_cover());
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if length < 7 {
                return Err(invalid_cover());
            }
            let height = u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]) as u32;
            let width = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as u32;
            return Ok((width, height));
        }
        index += length;
    }
    Err(invalid_cover())
}

fn webp_dimensions(bytes: &[u8]) -> Result<(u32, u32), AppError> {
    match &bytes[12..16] {
        b"VP8X" if bytes.len() >= 30 => {
            let width = 1 + u32::from_le_bytes([bytes[24], bytes[25], bytes[26], 0]);
            let height = 1 + u32::from_le_bytes([bytes[27], bytes[28], bytes[29], 0]);
            Ok((width, height))
        }
        b"VP8L" if bytes.len() >= 25 && bytes[20] == 0x2f => {
            let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
            Ok(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8 " if bytes.len() >= 30 && bytes[23..26] == [0x9d, 0x01, 0x2a] => {
            let width = u16::from_le_bytes([bytes[26], bytes[27]]) as u32 & 0x3fff;
            let height = u16::from_le_bytes([bytes[28], bytes[29]]) as u32 & 0x3fff;
            Ok((width, height))
        }
        _ => Err(invalid_cover()),
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), AppError> {
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width == 0
        || height == 0
        || width > MAXIMUM_COVER_EDGE
        || height > MAXIMUM_COVER_EDGE
        || pixels > MAXIMUM_COVER_PIXELS
    {
        return Err(AppError::validation(
            "INVALID_COVER_DIMENSIONS",
            "封面图片尺寸无效或超出安全限制。",
            "请选择边长不超过 20000 像素且总像素不超过一亿的图片。",
        ));
    }
    Ok(())
}

fn invalid_cover() -> AppError {
    AppError::validation(
        "INVALID_COVER_IMAGE",
        "封面文件已损坏或无法识别。",
        "请使用图片工具重新导出后再试。",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_png_jpeg_and_webp_from_content_and_reads_dimensions() {
        let mut png = vec![0_u8; 33];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&600_u32.to_be_bytes());
        png[20..24].copy_from_slice(&800_u32.to_be_bytes());
        assert_eq!(inspect_cover(&png).unwrap(), ("image/png", "png", 600, 800));

        let jpeg = [
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x03, 0x20, 0x02, 0x58, 0x03, 0x01, 0x11,
            0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00,
        ];
        assert_eq!(
            inspect_cover(&jpeg).unwrap(),
            ("image/jpeg", "jpg", 600, 800)
        );

        let mut webp = vec![0_u8; 30];
        webp[..4].copy_from_slice(b"RIFF");
        webp[8..12].copy_from_slice(b"WEBP");
        webp[12..16].copy_from_slice(b"VP8X");
        webp[24..27].copy_from_slice(&[0x57, 0x02, 0]);
        webp[27..30].copy_from_slice(&[0x1f, 0x03, 0]);
        assert_eq!(
            inspect_cover(&webp).unwrap(),
            ("image/webp", "webp", 600, 800)
        );
    }

    #[test]
    fn rejects_spoofed_or_dimensionless_images() {
        assert_eq!(
            inspect_cover(b"not a png").unwrap_err().to_dto().code,
            "UNSUPPORTED_COVER_TYPE"
        );
        assert_eq!(
            inspect_cover(b"\x89PNG\r\n\x1a\ntruncated")
                .unwrap_err()
                .to_dto()
                .code,
            "INVALID_COVER_IMAGE"
        );
    }
}
