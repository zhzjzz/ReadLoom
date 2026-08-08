use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use unicode_normalization::UnicodeNormalization;
use zip::{CompressionMethod, ZipArchive};

use crate::{error::AppError, infrastructure::archive::archive_limits::ArchiveLimits};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SafeArchivePath(String);

impl SafeArchivePath {
    pub(crate) fn parse(value: &str) -> Result<Self, AppError> {
        if value.len() > 1024 {
            return Err(unsafe_archive_path());
        }
        let bytes = value.as_bytes();
        let has_windows_drive =
            bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
        if value.starts_with('/') || has_windows_drive || value.contains(['\\', '\0']) {
            return Err(unsafe_archive_path());
        }

        let mut segments = Vec::new();
        let normalized_input = value.strip_suffix('/').unwrap_or(value);
        for segment in normalized_input.split('/') {
            match segment {
                "" => return Err(unsafe_archive_path()),
                "." => {}
                ".." => {
                    if segments.pop().is_none() {
                        return Err(unsafe_archive_path());
                    }
                }
                value if is_unsafe_windows_segment(value) => return Err(unsafe_archive_path()),
                value => segments.push(value),
            }
        }

        if segments.is_empty() {
            return Err(unsafe_archive_path());
        }

        Ok(Self(segments.join("/")))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_unsafe_windows_segment(segment: &str) -> bool {
    if segment.len() > 255
        || segment.ends_with(['.', ' '])
        || segment
            .chars()
            .any(|character| character.is_control() || "<>:\"|?*".contains(character))
    {
        return true;
    }
    let base = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || base
            .strip_prefix("COM")
            .or_else(|| base.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn unsafe_archive_path() -> AppError {
    AppError::validation(
        "UNSAFE_ARCHIVE_PATH",
        "EPUB 包含不安全的内部路径。",
        "请不要打开来源不可信或已损坏的 EPUB。",
    )
}

#[derive(Debug, Clone)]
pub(crate) struct SafeEpubArchive {
    #[allow(dead_code)]
    path: PathBuf,
    #[allow(dead_code)]
    limits: ArchiveLimits,
    entries: HashMap<SafeArchivePath, SafeArchiveEntry>,
}

#[derive(Debug, Clone)]
struct SafeArchiveEntry {
    original_name: String,
    size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceClass {
    Xhtml,
    Css,
    Image,
    Font,
    Xml,
}

impl SafeEpubArchive {
    pub(crate) fn open(path: &Path, limits: ArchiveLimits) -> Result<Self, AppError> {
        let metadata = fs::metadata(path).map_err(|_| invalid_zip())?;
        if !metadata.is_file() || metadata.len() > limits.maximum_archive_bytes {
            return Err(archive_limit_exceeded());
        }

        let file = File::open(path).map_err(|_| invalid_zip())?;
        let mut archive = ZipArchive::new(file).map_err(|_| invalid_zip())?;
        if archive.is_empty() {
            return Err(invalid_epub_container());
        }
        if archive.len() > limits.maximum_entries {
            return Err(archive_limit_exceeded());
        }

        {
            let mut mimetype = archive.by_index(0).map_err(|_| invalid_zip())?;
            if mimetype.name() != "mimetype"
                || mimetype.compression() != CompressionMethod::Stored
                || mimetype.size() != b"application/epub+zip".len() as u64
            {
                return Err(invalid_epub_container());
            }
            let mut value = Vec::with_capacity(b"application/epub+zip".len());
            mimetype
                .by_ref()
                .take(32)
                .read_to_end(&mut value)
                .map_err(|_| invalid_zip())?;
            if value != b"application/epub+zip" {
                return Err(invalid_epub_container());
            }
        }

        let mut canonical_names = HashSet::with_capacity(archive.len());
        let mut entries = HashMap::with_capacity(archive.len());
        let mut total_uncompressed_bytes = 0_u64;
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(|_| invalid_zip())?;
            if std::str::from_utf8(entry.name_raw()).is_err() {
                return Err(unsafe_archive_path());
            }
            if entry.encrypted() {
                return Err(AppError::validation(
                    "ENCRYPTED_EPUB",
                    "Readloom 不支持加密 EPUB。",
                    "请选择未加密、无 DRM 的 EPUB 文件。",
                ));
            }
            if entry.is_symlink() {
                return Err(unsafe_archive_path());
            }
            if !matches!(
                entry.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            ) {
                return Err(AppError::validation(
                    "UNSUPPORTED_MEDIA_TYPE",
                    "EPUB 使用了不受支持的 ZIP 压缩方式。",
                    "请将 EPUB 转换为 Stored 或 Deflate 压缩。",
                ));
            }
            if entry.size() > limits.maximum_entry_bytes {
                return Err(archive_limit_exceeded());
            }
            if entry.size() > 0
                && (entry.compressed_size() == 0
                    || entry.size() / entry.compressed_size().max(1)
                        > limits.maximum_compression_ratio)
            {
                return Err(archive_limit_exceeded());
            }
            total_uncompressed_bytes = total_uncompressed_bytes
                .checked_add(entry.size())
                .ok_or_else(archive_limit_exceeded)?;
            if total_uncompressed_bytes > limits.maximum_total_uncompressed_bytes {
                return Err(archive_limit_exceeded());
            }
            let path = SafeArchivePath::parse(entry.name())?;
            let collision_key = path
                .as_str()
                .nfkc()
                .flat_map(char::to_lowercase)
                .collect::<String>();
            if !canonical_names.insert(collision_key) {
                return Err(unsafe_archive_path());
            }
            if entry.is_file() {
                entries.insert(
                    path,
                    SafeArchiveEntry {
                        original_name: entry.name().to_owned(),
                        size: entry.size(),
                    },
                );
            }
        }

        let result = Self {
            path: path.to_owned(),
            limits,
            entries,
        };
        for marker in ["META-INF/rights.xml", "META-INF/encryption.xml"] {
            let marker = SafeArchivePath::parse(marker).expect("static EPUB marker path");
            if result.contains(&marker) {
                return Err(if marker.as_str().ends_with("rights.xml") {
                    drm_protected_epub()
                } else {
                    encrypted_epub()
                });
            }
        }
        Ok(result)
    }

    pub(crate) fn contains(&self, path: &SafeArchivePath) -> bool {
        self.entries.contains_key(path)
    }

    pub(crate) fn read(
        &self,
        path: &SafeArchivePath,
        class: ResourceClass,
    ) -> Result<Vec<u8>, AppError> {
        let entry = self.entries.get(path).ok_or_else(resource_not_found)?;
        let class_limit = match class {
            ResourceClass::Xhtml => self.limits.maximum_xhtml_bytes,
            ResourceClass::Css => self.limits.maximum_css_bytes,
            ResourceClass::Image => self.limits.maximum_image_bytes,
            ResourceClass::Font => self.limits.maximum_font_bytes,
            ResourceClass::Xml => self.limits.maximum_xml_bytes,
        };
        if entry.size > class_limit {
            return Err(archive_limit_exceeded());
        }

        let file = File::open(&self.path).map_err(|_| invalid_zip())?;
        let mut archive = ZipArchive::new(file).map_err(|_| invalid_zip())?;
        let zip_entry = archive
            .by_name(&entry.original_name)
            .map_err(|_| resource_not_found())?;
        let mut body = Vec::with_capacity(entry.size.min(class_limit) as usize);
        zip_entry
            .take(class_limit + 1)
            .read_to_end(&mut body)
            .map_err(|_| invalid_zip())?;
        if body.len() as u64 > class_limit || body.len() as u64 != entry.size {
            return Err(archive_limit_exceeded());
        }
        Ok(body)
    }
}

fn invalid_zip() -> AppError {
    AppError::validation(
        "INVALID_ZIP",
        "所选文件不是有效的 EPUB ZIP 容器。",
        "请选择未损坏的 EPUB 文件。",
    )
}

fn invalid_epub_container() -> AppError {
    AppError::validation(
        "INVALID_EPUB",
        "所选 ZIP 不符合 EPUB 容器格式。",
        "请选择 mimetype 正确且未损坏的 EPUB 文件。",
    )
}

fn encrypted_epub() -> AppError {
    AppError::validation(
        "ENCRYPTED_EPUB",
        "Readloom 不支持包含加密资源的 EPUB。",
        "请选择未加密、无 DRM 的 EPUB 文件。",
    )
}

fn drm_protected_epub() -> AppError {
    AppError::validation(
        "DRM_PROTECTED_EPUB",
        "Readloom 不支持受 DRM 保护的 EPUB。",
        "请使用提供方的授权阅读器打开此文件。",
    )
}

fn archive_limit_exceeded() -> AppError {
    AppError::validation(
        "ARCHIVE_LIMIT_EXCEEDED",
        "EPUB 超出安全读取限制。",
        "请选择体积和内容规模较小的 EPUB。",
    )
}

fn resource_not_found() -> AppError {
    AppError::validation(
        "RESOURCE_NOT_FOUND",
        "EPUB 资源不存在或已失效。",
        "返回目录并重新打开章节。",
    )
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use tempfile::tempdir;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::*;

    fn write_zip(entries: &[(&str, &[u8])]) -> PathBuf {
        write_zip_with_method(entries, CompressionMethod::Stored)
    }

    fn write_zip_with_method(entries: &[(&str, &[u8])], compression: CompressionMethod) -> PathBuf {
        let directory = tempdir().expect("temporary fixture directory");
        let path = directory.keep().join("fixture.epub");
        let file = File::create(&path).expect("create fixture");
        let mut writer = ZipWriter::new(file);
        for (name, bytes) in entries {
            let entry_compression = if *name == "mimetype" {
                CompressionMethod::Stored
            } else {
                compression
            };
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(entry_compression),
                )
                .expect("start fixture entry");
            writer.write_all(bytes).expect("write fixture entry");
        }
        writer.finish().expect("finish fixture");
        path
    }

    #[test]
    fn rejects_a_path_that_escapes_the_epub_root() {
        let error = SafeArchivePath::parse("OPS/../../private.txt")
            .expect_err("parent traversal must be rejected");

        assert_eq!(error.to_dto().code, "UNSAFE_ARCHIVE_PATH");
    }

    #[test]
    fn rejects_windows_separators_before_normalization() {
        let error = SafeArchivePath::parse("OPS\\..\\private.txt")
            .expect_err("backslash aliases must be rejected");

        assert_eq!(error.to_dto().code, "UNSAFE_ARCHIVE_PATH");
    }

    #[test]
    fn rejects_host_absolute_paths() {
        for path in ["/etc/passwd", "C:/Windows/win.ini", "//server/share/book"] {
            let error = SafeArchivePath::parse(path).expect_err("host paths must be rejected");
            assert_eq!(error.to_dto().code, "UNSAFE_ARCHIVE_PATH");
        }
    }

    #[test]
    fn opening_an_archive_rejects_an_unsafe_entry() {
        let path = write_zip(&[("mimetype", b"application/epub+zip"), ("../escape", b"bad")]);

        let error = SafeEpubArchive::open(&path, ArchiveLimits::default())
            .expect_err("unsafe entry must reject the whole archive");

        assert_eq!(error.to_dto().code, "UNSAFE_ARCHIVE_PATH");
    }

    #[test]
    fn opening_an_archive_rejects_canonical_name_collisions() {
        let path = write_zip(&[
            ("mimetype", b"application/epub+zip"),
            ("OPS/chapter.xhtml", b"first"),
            ("OPS/./chapter.xhtml", b"second"),
        ]);

        let error = SafeEpubArchive::open(&path, ArchiveLimits::default())
            .expect_err("canonical duplicates must reject the whole archive");

        assert_eq!(error.to_dto().code, "UNSAFE_ARCHIVE_PATH");
    }

    #[test]
    fn opening_an_archive_rejects_unicode_normalization_collisions() {
        let path = write_zip(&[
            ("mimetype", b"application/epub+zip"),
            ("OPS/caf\u{e9}.xhtml", b"first"),
            ("OPS/cafe\u{301}.xhtml", b"second"),
        ]);

        let error = SafeEpubArchive::open(&path, ArchiveLimits::default())
            .expect_err("Unicode aliases must reject the whole archive");

        assert_eq!(error.to_dto().code, "UNSAFE_ARCHIVE_PATH");
    }

    #[test]
    fn opening_an_archive_rejects_an_oversized_entry() {
        let path = write_zip(&[
            ("mimetype", b"application/epub+zip"),
            ("OPS/large.bin", b"12345"),
        ]);
        let limits = ArchiveLimits {
            maximum_entry_bytes: 4,
            ..ArchiveLimits::default()
        };

        let error = SafeEpubArchive::open(&path, limits)
            .expect_err("oversized entries must reject the whole archive");

        assert_eq!(error.to_dto().code, "ARCHIVE_LIMIT_EXCEEDED");
    }

    #[test]
    fn opening_an_archive_rejects_too_many_entries() {
        let path = write_zip(&[("mimetype", b"application/epub+zip"), ("OPS/a", b"a")]);
        let limits = ArchiveLimits {
            maximum_entries: 1,
            ..ArchiveLimits::default()
        };

        let error = SafeEpubArchive::open(&path, limits)
            .expect_err("the entry-count limit must cover the entire archive");

        assert_eq!(error.to_dto().code, "ARCHIVE_LIMIT_EXCEEDED");
    }

    #[test]
    fn opening_an_archive_rejects_excessive_total_uncompressed_size() {
        let path = write_zip(&[("mimetype", b"application/epub+zip"), ("OPS/a", b"12345")]);
        let limits = ArchiveLimits {
            maximum_total_uncompressed_bytes: 24,
            ..ArchiveLimits::default()
        };

        let error = SafeEpubArchive::open(&path, limits)
            .expect_err("the uncompressed total must be bounded across entries");

        assert_eq!(error.to_dto().code, "ARCHIVE_LIMIT_EXCEEDED");
    }

    #[test]
    fn opening_an_archive_rejects_a_suspicious_compression_ratio() {
        let repeated = vec![b'x'; 16 * 1024];
        let path = write_zip_with_method(
            &[
                ("mimetype", b"application/epub+zip"),
                ("OPS/bomb", &repeated),
            ],
            CompressionMethod::Deflated,
        );
        let limits = ArchiveLimits {
            maximum_compression_ratio: 2,
            ..ArchiveLimits::default()
        };

        let error = SafeEpubArchive::open(&path, limits)
            .expect_err("high-ratio entries must be rejected before extraction");

        assert_eq!(error.to_dto().code, "ARCHIVE_LIMIT_EXCEEDED");
    }

    #[test]
    fn rejects_windows_reserved_and_ambiguous_paths() {
        for path in ["OPS/CON.txt", "OPS/chapter. ", "OPS//chapter.xhtml"] {
            let error = SafeArchivePath::parse(path).expect_err("unsafe Windows path");
            assert_eq!(error.to_dto().code, "UNSAFE_ARCHIVE_PATH");
        }
    }

    #[test]
    fn epub_mimetype_must_be_first_exact_and_uncompressed() {
        let not_first = write_zip(&[
            ("META-INF/container.xml", b"container"),
            ("mimetype", b"application/epub+zip"),
        ]);
        let error = SafeEpubArchive::open(&not_first, ArchiveLimits::default())
            .expect_err("mimetype must be the first entry");
        assert_eq!(error.to_dto().code, "INVALID_EPUB");

        let wrong_value = write_zip(&[("mimetype", b"application/zip")]);
        let error = SafeEpubArchive::open(&wrong_value, ArchiveLimits::default())
            .expect_err("mimetype must be exact");
        assert_eq!(error.to_dto().code, "INVALID_EPUB");
    }

    #[test]
    fn rejects_declared_encryption_and_drm() {
        for (marker, code) in [
            ("META-INF/encryption.xml", "ENCRYPTED_EPUB"),
            ("META-INF/rights.xml", "DRM_PROTECTED_EPUB"),
        ] {
            let path = write_zip(&[("mimetype", b"application/epub+zip"), (marker, b"<xml/>")]);
            let error = SafeEpubArchive::open(&path, ArchiveLimits::default())
                .expect_err("protected EPUB must be rejected");
            assert_eq!(error.to_dto().code, code);
        }
    }
}
