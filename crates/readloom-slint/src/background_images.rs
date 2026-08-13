use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
};

use readloom_core::ReadloomCore;
use slint::{Rgba8Pixel, SharedPixelBuffer};

const BACKGROUND_IMAGE_MAX_WIDTH: u32 = 1_920;
const BACKGROUND_IMAGE_MAX_HEIGHT: u32 = 1_920;
const BACKGROUND_IMAGE_DECODE_MAX_DIMENSION: u32 = 8_192;
const BACKGROUND_IMAGE_DECODE_MAX_BYTES: u64 = 192 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackgroundImageOperation {
    Load,
    Select,
    Clear,
}

pub(crate) struct BackgroundImageRequest {
    pub(crate) request_id: u64,
    pub(crate) operation: BackgroundImageOperation,
    pub(crate) path: Option<PathBuf>,
}

pub(crate) struct BackgroundImageResult {
    pub(crate) request_id: u64,
    pub(crate) operation: BackgroundImageOperation,
    pub(crate) result: Result<Option<SharedPixelBuffer<Rgba8Pixel>>, String>,
}

pub(crate) fn spawn_background_image_worker(
    core: Arc<ReadloomCore>,
) -> (
    mpsc::Sender<BackgroundImageRequest>,
    mpsc::Receiver<BackgroundImageResult>,
) {
    let (request_sender, request_receiver) = mpsc::channel::<BackgroundImageRequest>();
    let (result_sender, result_receiver) = mpsc::channel::<BackgroundImageResult>();
    thread::spawn(move || {
        while let Ok(request) = request_receiver.recv() {
            let result = process_background_image_request(&core, &request);
            if result_sender
                .send(BackgroundImageResult {
                    request_id: request.request_id,
                    operation: request.operation,
                    result,
                })
                .is_err()
            {
                return;
            }
        }
    });
    (request_sender, result_receiver)
}

fn process_background_image_request(
    core: &ReadloomCore,
    request: &BackgroundImageRequest,
) -> Result<Option<SharedPixelBuffer<Rgba8Pixel>>, String> {
    match request.operation {
        BackgroundImageOperation::Load => {
            let Some(path) = core
                .background_image_path()
                .map_err(|error| error.to_string())?
            else {
                return Ok(None);
            };
            decode_background_image_pixels(&path).map(Some)
        }
        BackgroundImageOperation::Select => {
            let path = request
                .path
                .as_deref()
                .ok_or_else(|| "没有选择背景图片。".to_owned())?;
            // Decode and bound the image before changing the persisted selection. A corrupt
            // image can therefore never replace a background that was already working.
            let pixels = decode_background_image_pixels(path)?;
            core.set_background_image(path)
                .map_err(|error| error.to_string())?;
            Ok(Some(pixels))
        }
        BackgroundImageOperation::Clear => {
            core.clear_background_image()
                .map_err(|error| error.to_string())?;
            Ok(None)
        }
    }
}

pub(crate) fn decode_background_image_pixels(
    path: &Path,
) -> Result<SharedPixelBuffer<Rgba8Pixel>, String> {
    let mut reader = image::ImageReader::open(path)
        .map_err(|error| format!("无法读取背景图片：{error}"))?
        .with_guessed_format()
        .map_err(|error| format!("无法识别背景图片格式：{error}"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(BACKGROUND_IMAGE_DECODE_MAX_DIMENSION);
    limits.max_image_height = Some(BACKGROUND_IMAGE_DECODE_MAX_DIMENSION);
    limits.max_alloc = Some(BACKGROUND_IMAGE_DECODE_MAX_BYTES);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| format!("背景图片解码失败：{error}"))?;
    let decoded = if decoded.width() > BACKGROUND_IMAGE_MAX_WIDTH
        || decoded.height() > BACKGROUND_IMAGE_MAX_HEIGHT
    {
        decoded.resize(
            BACKGROUND_IMAGE_MAX_WIDTH,
            BACKGROUND_IMAGE_MAX_HEIGHT,
            image::imageops::FilterType::Triangle,
        )
    } else {
        decoded
    };
    let rgba = decoded.into_rgba8();
    Ok(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    ))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn background_images_are_downscaled_before_reaching_slint() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("background.png");
        let source = image::RgbaImage::from_pixel(2_400, 1_200, image::Rgba([24, 48, 72, 255]));
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("encode test png");
        std::fs::write(&path, encoded.into_inner()).expect("write test png");

        let pixels = decode_background_image_pixels(&path).expect("decode background");

        assert_eq!(pixels.width(), 1_920);
        assert_eq!(pixels.height(), 960);
    }

    #[test]
    fn oversized_background_dimensions_are_rejected_before_full_decode() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("oversized.png");
        let source = image::RgbaImage::from_pixel(9_000, 1, image::Rgba([24, 48, 72, 255]));
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("encode oversized test png");
        std::fs::write(&path, encoded.into_inner()).expect("write oversized test png");

        assert!(decode_background_image_pixels(&path).is_err());
    }

    #[test]
    fn invalid_selection_does_not_replace_the_previous_background() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let core = ReadloomCore::open(&directory.path().join("state.sqlite"))
            .expect("open temporary core");
        let valid = directory.path().join("valid.png");
        let source = image::RgbaImage::from_pixel(2, 2, image::Rgba([24, 48, 72, 255]));
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("encode valid png");
        std::fs::write(&valid, encoded.into_inner()).expect("write valid png");
        let stored = core
            .set_background_image(&valid)
            .expect("store initial background");
        let corrupt = directory.path().join("corrupt.png");
        std::fs::write(&corrupt, b"\x89PNG\r\n\x1a\ncorrupt").expect("write corrupt png");

        let request = BackgroundImageRequest {
            request_id: 1,
            operation: BackgroundImageOperation::Select,
            path: Some(corrupt),
        };
        assert!(process_background_image_request(&core, &request).is_err());
        assert_eq!(core.background_image_path().unwrap(), Some(stored));
    }
}
