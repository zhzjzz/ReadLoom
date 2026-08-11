use std::fs;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::{
    error::AppError,
    infrastructure::storage::local_state::{BackgroundImageSource, LocalStateStore},
};

const MAX_BACKGROUND_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetBackgroundImageRequest {
    path: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackgroundImageDto {
    key: String,
    media_type: String,
}

#[tauri::command]
pub(crate) fn get_background_image(
    local_state: State<'_, LocalStateStore>,
) -> Result<Option<BackgroundImageDto>, AppError> {
    let Some(source) = local_state.background_image()? else {
        return Ok(None);
    };
    if !stored_background_is_valid(&source) {
        local_state.clear_background_image()?;
        return Ok(None);
    }
    Ok(Some(source.into()))
}

#[tauri::command]
pub(crate) fn set_background_image(
    app: AppHandle,
    local_state: State<'_, LocalStateStore>,
    request: SetBackgroundImageRequest,
) -> Result<BackgroundImageDto, AppError> {
    let source_path = fs::canonicalize(request.path.trim()).map_err(|_| invalid_background())?;
    if !source_path.is_file() {
        return Err(invalid_background());
    }
    let bytes = fs::read(&source_path).map_err(|_| invalid_background())?;
    if bytes.is_empty() || bytes.len() > MAX_BACKGROUND_BYTES {
        return Err(AppError::validation(
            "BACKGROUND_IMAGE_TOO_LARGE",
            "背景图片为空或超过 20 MiB。",
            "请选择较小的 PNG、JPEG 或 WebP 图片。",
        ));
    }
    let (media_type, extension) = detected_image_type(&bytes).ok_or_else(invalid_background)?;
    let key = blake3::hash(&bytes).to_hex().to_string();
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|_| AppError::internal("BACKGROUND_IMAGE_FAILED", "resolve app data directory"))?
        .join("backgrounds");
    fs::create_dir_all(&directory).map_err(|_| {
        AppError::internal("BACKGROUND_IMAGE_FAILED", "create background directory")
    })?;
    let destination = directory.join(format!("background-{key}.{extension}"));
    if fs::read(&destination).ok().as_deref() != Some(bytes.as_slice()) {
        fs::write(&destination, &bytes)
            .map_err(|_| AppError::internal("BACKGROUND_IMAGE_FAILED", "write background image"))?;
        if fs::read(&destination).ok().as_deref() != Some(bytes.as_slice()) {
            return Err(AppError::internal(
                "BACKGROUND_IMAGE_FAILED",
                "verify background image",
            ));
        }
    }
    let previous = local_state.background_image()?;
    let stored = BackgroundImageSource {
        path: destination.clone(),
        key,
        media_type: media_type.to_owned(),
    };
    local_state.set_background_image(&stored)?;
    if let Some(previous) = previous
        && previous.path != destination
        && previous.path.parent() == Some(directory.as_path())
    {
        let _ = fs::remove_file(previous.path);
    }
    Ok(stored.into())
}

#[tauri::command]
pub(crate) fn clear_background_image(
    app: AppHandle,
    local_state: State<'_, LocalStateStore>,
) -> Result<(), AppError> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|_| AppError::internal("BACKGROUND_IMAGE_FAILED", "resolve app data directory"))?
        .join("backgrounds");
    if let Some(source) = local_state.background_image()?
        && source.path.parent() == Some(directory.as_path())
    {
        let _ = fs::remove_file(source.path);
    }
    local_state.clear_background_image()
}

fn detected_image_type(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(("image/jpeg", "jpg"))
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

fn stored_background_is_valid(source: &BackgroundImageSource) -> bool {
    let Ok(bytes) = fs::read(&source.path) else {
        return false;
    };
    !bytes.is_empty()
        && bytes.len() <= MAX_BACKGROUND_BYTES
        && detected_image_type(&bytes)
            .is_some_and(|(media_type, _)| media_type == source.media_type)
}

fn invalid_background() -> AppError {
    AppError::validation(
        "INVALID_BACKGROUND_IMAGE",
        "无法读取这张背景图片，或图片格式不受支持。",
        "请选择 20 MiB 以内的 PNG、JPEG 或 WebP 图片。",
    )
}

impl From<BackgroundImageSource> for BackgroundImageDto {
    fn from(source: BackgroundImageSource) -> Self {
        Self {
            key: source.key,
            media_type: source.media_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_only_supported_raster_signatures() {
        assert_eq!(
            detected_image_type(b"\x89PNG\r\n\x1a\nrest"),
            Some(("image/png", "png"))
        );
        assert_eq!(detected_image_type(b"not an image"), None);
    }
}
