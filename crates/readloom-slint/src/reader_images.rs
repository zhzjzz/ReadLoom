use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
    sync::{Arc, mpsc},
    thread,
};

#[cfg(debug_assertions)]
use std::time::{Duration, Instant};

use readloom_core::EpubImageResource;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

const EPUB_IMAGE_MAX_WIDTH: u32 = 1_200;
const EPUB_IMAGE_MAX_HEIGHT: u32 = 1_200;
const EPUB_IMAGE_DECODE_MAX_DIMENSION: u32 = 8_192;
const EPUB_IMAGE_DECODE_MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAXIMUM_PENDING_IMAGE_DECODES: usize = 8;
const MAXIMUM_FAILED_IMAGE_KEYS: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ReaderImageKey {
    book_fingerprint: Arc<str>,
    chapter_index: usize,
    image_index: usize,
}

impl ReaderImageKey {
    pub(crate) fn new(
        book_fingerprint: Arc<str>,
        chapter_index: usize,
        image_index: usize,
    ) -> Self {
        Self {
            book_fingerprint,
            chapter_index,
            image_index,
        }
    }

    pub(crate) fn matches(
        &self,
        book_fingerprint: &str,
        chapter_index: usize,
        image_index: usize,
    ) -> bool {
        self.book_fingerprint.as_ref() == book_fingerprint
            && self.chapter_index == chapter_index
            && self.image_index == image_index
    }
}

struct CachedDecodedImage {
    image: Image,
    decoded_bytes: usize,
    last_used: u64,
}

struct DecodedImageCache {
    entries: HashMap<ReaderImageKey, CachedDecodedImage>,
    decoded_bytes: usize,
    maximum_bytes: usize,
    clock: u64,
}

impl DecodedImageCache {
    fn new(maximum_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            decoded_bytes: 0,
            maximum_bytes,
            clock: 0,
        }
    }

    fn get(&mut self, key: &ReaderImageKey) -> Option<Image> {
        self.clock = self.clock.wrapping_add(1).max(1);
        let entry = self.entries.get_mut(key)?;
        entry.last_used = self.clock;
        Some(entry.image.clone())
    }

    fn insert(&mut self, key: ReaderImageKey, image: Image) {
        if let Some(previous) = self.entries.remove(&key) {
            self.decoded_bytes = self.decoded_bytes.saturating_sub(previous.decoded_bytes);
        }
        self.clock = self.clock.wrapping_add(1).max(1);
        let decoded_bytes = decoded_image_bytes(&image);
        self.decoded_bytes = self.decoded_bytes.saturating_add(decoded_bytes);
        self.entries.insert(
            key,
            CachedDecodedImage {
                image,
                decoded_bytes,
                last_used: self.clock,
            },
        );
        while self.decoded_bytes > self.maximum_bytes {
            let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            let Some(removed) = self.entries.remove(&oldest_key) else {
                break;
            };
            self.decoded_bytes = self.decoded_bytes.saturating_sub(removed.decoded_bytes);
        }
    }
}

struct ImageDecodeRequest {
    key: ReaderImageKey,
    bytes: Arc<[u8]>,
}

struct ImageDecodeResult {
    key: ReaderImageKey,
    pixels: Option<SharedPixelBuffer<Rgba8Pixel>>,
    #[cfg(debug_assertions)]
    elapsed: Duration,
}

struct FailedImageCache {
    keys: HashSet<ReaderImageKey>,
    order: VecDeque<ReaderImageKey>,
    maximum_keys: usize,
}

impl FailedImageCache {
    fn new(maximum_keys: usize) -> Self {
        Self {
            keys: HashSet::new(),
            order: VecDeque::new(),
            maximum_keys,
        }
    }

    fn contains(&self, key: &ReaderImageKey) -> bool {
        self.keys.contains(key)
    }

    fn insert(&mut self, key: ReaderImageKey) {
        if !self.keys.insert(key.clone()) {
            return;
        }
        self.order.push_back(key);
        while self.order.len() > self.maximum_keys {
            if let Some(oldest) = self.order.pop_front() {
                self.keys.remove(&oldest);
            }
        }
    }
}

/// Owns the byte-budgeted EPUB image cache and its single bounded decode queue.
///
/// Callers only request an image and drain completed keys. Compressed bytes,
/// decoding, scaling, duplicate suppression and cache eviction stay behind this
/// seam, so Slint's `Model::row_data` never performs image work.
pub(crate) struct ReaderImagePipeline {
    cache: RefCell<DecodedImageCache>,
    failed: RefCell<FailedImageCache>,
    pending: RefCell<HashSet<ReaderImageKey>>,
    request_sender: mpsc::SyncSender<ImageDecodeRequest>,
    result_receiver: RefCell<mpsc::Receiver<ImageDecodeResult>>,
}

impl ReaderImagePipeline {
    pub(crate) fn new(maximum_bytes: usize) -> Rc<Self> {
        let (request_sender, request_receiver) =
            mpsc::sync_channel::<ImageDecodeRequest>(MAXIMUM_PENDING_IMAGE_DECODES);
        let (result_sender, result_receiver) =
            mpsc::sync_channel::<ImageDecodeResult>(MAXIMUM_PENDING_IMAGE_DECODES);
        thread::spawn(move || image_decode_worker(request_receiver, result_sender));
        Rc::new(Self {
            cache: RefCell::new(DecodedImageCache::new(maximum_bytes)),
            failed: RefCell::new(FailedImageCache::new(MAXIMUM_FAILED_IMAGE_KEYS)),
            pending: RefCell::new(HashSet::new()),
            request_sender,
            result_receiver: RefCell::new(result_receiver),
        })
    }

    pub(crate) fn image_or_request(
        &self,
        key: ReaderImageKey,
        resource: &EpubImageResource,
    ) -> Image {
        if let Some(image) = self.cache.borrow_mut().get(&key) {
            return image;
        }
        if self.failed.borrow().contains(&key) {
            return Image::default();
        }
        if !self.pending.borrow_mut().insert(key.clone()) {
            return Image::default();
        }
        let request = ImageDecodeRequest {
            key: key.clone(),
            bytes: resource.bytes.clone(),
        };
        if self.request_sender.try_send(request).is_err() {
            self.pending.borrow_mut().remove(&key);
        }
        Image::default()
    }

    pub(crate) fn drain_ready(&self) -> Vec<ReaderImageKey> {
        let mut ready = Vec::new();
        while let Ok(result) = self.result_receiver.borrow_mut().try_recv() {
            self.pending.borrow_mut().remove(&result.key);
            match result.pixels {
                Some(pixels) => {
                    self.cache
                        .borrow_mut()
                        .insert(result.key.clone(), Image::from_rgba8(pixels));
                    ready.push(result.key.clone());
                }
                None => self.failed.borrow_mut().insert(result.key.clone()),
            }
            #[cfg(debug_assertions)]
            eprintln!(
                "[readloom:perf] EPUB image decode chapter={} image={} elapsed={:?}",
                result.key.chapter_index, result.key.image_index, result.elapsed
            );
        }
        ready
    }
}

fn image_decode_worker(
    requests: mpsc::Receiver<ImageDecodeRequest>,
    results: mpsc::SyncSender<ImageDecodeResult>,
) {
    while let Ok(request) = requests.recv() {
        #[cfg(debug_assertions)]
        let started = Instant::now();
        let pixels = decode_epub_image_pixels(&request.bytes);
        if results
            .send(ImageDecodeResult {
                key: request.key,
                pixels,
                #[cfg(debug_assertions)]
                elapsed: started.elapsed(),
            })
            .is_err()
        {
            return;
        }
    }
}

pub(crate) fn decode_epub_image_pixels(bytes: &[u8]) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let cursor = std::io::Cursor::new(bytes);
    let mut reader = image::ImageReader::new(cursor).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(EPUB_IMAGE_DECODE_MAX_DIMENSION);
    limits.max_image_height = Some(EPUB_IMAGE_DECODE_MAX_DIMENSION);
    limits.max_alloc = Some(EPUB_IMAGE_DECODE_MAX_BYTES);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let decoded = resize_to_fit(decoded, EPUB_IMAGE_MAX_WIDTH, EPUB_IMAGE_MAX_HEIGHT);
    let rgba = decoded.into_rgba8();
    Some(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    ))
}

fn resize_to_fit(
    decoded: image::DynamicImage,
    maximum_width: u32,
    maximum_height: u32,
) -> image::DynamicImage {
    if decoded.width() > maximum_width || decoded.height() > maximum_height {
        decoded.resize(
            maximum_width,
            maximum_height,
            image::imageops::FilterType::Triangle,
        )
    } else {
        decoded
    }
}

fn decoded_image_bytes(image: &Image) -> usize {
    let size = image.size();
    (size.width as usize)
        .saturating_mul(size.height as usize)
        .saturating_mul(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(index: usize) -> ReaderImageKey {
        ReaderImageKey::new(Arc::from("test-book"), 0, index)
    }

    fn image() -> Image {
        Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1))
    }

    #[test]
    fn decoded_image_cache_is_a_bounded_lru() {
        let mut cache = DecodedImageCache::new(8);
        cache.insert(key(1), image());
        cache.insert(key(2), image());
        assert!(cache.get(&key(1)).is_some());

        cache.insert(key(3), image());

        assert!(cache.entries.contains_key(&key(1)));
        assert!(!cache.entries.contains_key(&key(2)));
        assert!(cache.entries.contains_key(&key(3)));
        assert!(cache.decoded_bytes <= cache.maximum_bytes);
    }

    #[test]
    fn epub_images_are_downscaled_before_becoming_slint_images() {
        let source = image::RgbaImage::from_pixel(1_600, 800, image::Rgba([24, 48, 72, 255]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("encode test png");

        let pixels = decode_epub_image_pixels(&encoded.into_inner()).expect("decode image");

        assert_eq!(pixels.width(), 1_200);
        assert_eq!(pixels.height(), 600);
    }

    #[test]
    fn oversized_epub_image_dimensions_are_rejected_before_full_decode() {
        let source = image::RgbaImage::from_pixel(9_000, 1, image::Rgba([24, 48, 72, 255]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("encode oversized test png");

        assert!(decode_epub_image_pixels(&encoded.into_inner()).is_none());
    }

    #[test]
    fn failed_image_keys_are_bounded() {
        let mut failures = FailedImageCache::new(2);
        failures.insert(key(1));
        failures.insert(key(2));
        failures.insert(key(3));

        assert!(!failures.contains(&key(1)));
        assert!(failures.contains(&key(2)));
        assert!(failures.contains(&key(3)));
    }
}
