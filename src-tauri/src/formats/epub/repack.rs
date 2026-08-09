use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use unicode_normalization::UnicodeNormalization;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    error::AppError,
    infrastructure::archive::{
        archive_limits::ArchiveLimits,
        safe_zip::{SafeArchivePath, SafeEpubArchive},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepackReport {
    pub copied_entries: usize,
    pub modified_entries: usize,
    pub added_entries: usize,
    pub copied_uncompressed_bytes: u64,
}

pub(crate) fn repack_epub(
    source: &Path,
    output: File,
    overlays: &HashMap<String, Vec<u8>>,
    limits: ArchiveLimits,
    cancelled: &Arc<AtomicBool>,
) -> Result<(File, RepackReport), AppError> {
    SafeEpubArchive::open(source, limits)?;
    validate_overlay_paths(overlays, limits)?;
    let source_file = File::open(source).map_err(|_| repack_failed())?;
    let mut archive = ZipArchive::new(source_file).map_err(|_| repack_failed())?;
    let mut writer = ZipWriter::new(output);
    writer
        .start_file(
            "mimetype",
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .map_err(|_| repack_failed())?;
    writer
        .write_all(b"application/epub+zip")
        .map_err(|_| repack_failed())?;

    let mut report = RepackReport {
        copied_entries: 0,
        modified_entries: 0,
        added_entries: 0,
        copied_uncompressed_bytes: 0,
    };
    let mut written = HashSet::new();
    written.insert(collision_key("mimetype"));

    for index in 1..archive.len() {
        ensure_not_cancelled(cancelled)?;
        let mut entry = archive.by_index(index).map_err(|_| repack_failed())?;
        let normalized = SafeArchivePath::parse(entry.name())?;
        let key = collision_key(normalized.as_str());
        if !written.insert(key) {
            return Err(duplicate_path());
        }
        if entry.is_dir() {
            writer
                .add_directory(
                    entry.name(),
                    SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
                )
                .map_err(|_| repack_failed())?;
            continue;
        }
        let compression = match entry.compression() {
            CompressionMethod::Stored => CompressionMethod::Stored,
            CompressionMethod::Deflated => CompressionMethod::Deflated,
            _ => return Err(repack_failed()),
        };
        writer
            .start_file(
                entry.name(),
                SimpleFileOptions::default().compression_method(compression),
            )
            .map_err(|_| repack_failed())?;
        if let Some(replacement) = overlays.get(normalized.as_str()) {
            writer.write_all(replacement).map_err(|_| repack_failed())?;
            report.modified_entries += 1;
        } else {
            let copied = std::io::copy(&mut entry, &mut writer).map_err(|_| repack_failed())?;
            report.copied_uncompressed_bytes = report
                .copied_uncompressed_bytes
                .checked_add(copied)
                .ok_or_else(repack_limit_exceeded)?;
            report.copied_entries += 1;
        }
    }

    for (path, body) in overlays {
        let safe = SafeArchivePath::parse(path)?;
        let key = collision_key(safe.as_str());
        if written.contains(&key) {
            continue;
        }
        ensure_not_cancelled(cancelled)?;
        written.insert(key);
        writer
            .start_file(
                safe.as_str(),
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .map_err(|_| repack_failed())?;
        writer.write_all(body).map_err(|_| repack_failed())?;
        report.added_entries += 1;
    }
    if written.len() > limits.maximum_entries {
        return Err(repack_limit_exceeded());
    }
    let mut output = writer.finish().map_err(|_| repack_failed())?;
    output.flush().map_err(|_| temporary_output_failed())?;
    output.sync_all().map_err(|_| temporary_output_failed())?;
    let output_size = output
        .metadata()
        .map_err(|_| temporary_output_failed())?
        .len();
    if output_size > limits.maximum_archive_bytes {
        return Err(repack_limit_exceeded());
    }
    Ok((output, report))
}

pub(crate) fn verify_unchanged_resources(
    source: &Path,
    generated: &Path,
    modified_paths: &HashSet<String>,
) -> Result<(), AppError> {
    let mut source = ZipArchive::new(File::open(source).map_err(|_| round_trip_mismatch())?)
        .map_err(|_| round_trip_mismatch())?;
    let mut generated = ZipArchive::new(File::open(generated).map_err(|_| round_trip_mismatch())?)
        .map_err(|_| round_trip_mismatch())?;
    let generated_names = (0..generated.len())
        .map(|index| {
            generated
                .by_index(index)
                .map(|entry| collision_key(entry.name().trim_end_matches('/')))
                .map_err(|_| round_trip_mismatch())
        })
        .collect::<Result<HashSet<_>, _>>()?;
    for index in 0..source.len() {
        let mut original = source.by_index(index).map_err(|_| round_trip_mismatch())?;
        let normalized = SafeArchivePath::parse(original.name())?;
        if original.is_dir() || modified_paths.contains(normalized.as_str()) {
            continue;
        }
        if !generated_names.contains(&collision_key(normalized.as_str())) {
            return Err(round_trip_mismatch());
        }
        let original_name = original.name().to_owned();
        let original_hash = hash_reader(&mut original)?;
        let mut regenerated = generated
            .by_name(&original_name)
            .map_err(|_| round_trip_mismatch())?;
        if original_hash != hash_reader(&mut regenerated)? {
            return Err(round_trip_mismatch());
        }
    }
    Ok(())
}

fn validate_overlay_paths(
    overlays: &HashMap<String, Vec<u8>>,
    limits: ArchiveLimits,
) -> Result<(), AppError> {
    let mut paths = HashSet::new();
    let mut total = 0_u64;
    for (path, bytes) in overlays {
        let safe = SafeArchivePath::parse(path)?;
        if !paths.insert(collision_key(safe.as_str())) {
            return Err(duplicate_path());
        }
        if bytes.len() as u64 > limits.maximum_entry_bytes {
            return Err(repack_limit_exceeded());
        }
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or_else(repack_limit_exceeded)?;
        if total > limits.maximum_total_uncompressed_bytes {
            return Err(repack_limit_exceeded());
        }
    }
    Ok(())
}

fn hash_reader(reader: &mut impl Read) -> Result<blake3::Hash, AppError> {
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| round_trip_mismatch())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize())
}

fn collision_key(path: &str) -> String {
    path.nfkc().flat_map(char::to_lowercase).collect()
}

fn ensure_not_cancelled(cancelled: &Arc<AtomicBool>) -> Result<(), AppError> {
    if cancelled.load(Ordering::Acquire) {
        Err(AppError::validation(
            "SAVE_CANCELLED",
            "已取消 EPUB 另存为。",
            "编辑草稿和原 EPUB 均保持不变。",
        ))
    } else {
        Ok(())
    }
}

fn repack_failed() -> AppError {
    AppError::validation(
        "REPACK_FAILED",
        "无法安全重新打包 EPUB。",
        "检查源文件和磁盘状态后重试。",
    )
}

fn repack_limit_exceeded() -> AppError {
    AppError::validation(
        "REPACK_LIMIT_EXCEEDED",
        "生成的 EPUB 超出安全限制。",
        "请选择较小的封面或 EPUB 后重试。",
    )
}

fn duplicate_path() -> AppError {
    AppError::validation(
        "REPACK_FAILED",
        "生成 EPUB 时检测到重复内部路径。",
        "请选择结构正常的源 EPUB。",
    )
}

fn temporary_output_failed() -> AppError {
    AppError::validation(
        "TEMPORARY_OUTPUT_FAILED",
        "无法写入或同步 EPUB 临时文件。",
        "检查目标目录权限和磁盘空间后重试。",
    )
}

fn round_trip_mismatch() -> AppError {
    AppError::validation(
        "ROUND_TRIP_MISMATCH",
        "重新打包后有未修改资源发生变化。",
        "生成文件未写入目标；请保留草稿并重试。",
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use tempfile::tempdir;

    use super::*;
    use crate::{epub_test_fixtures::minimal_epub3, formats::epub::parser::parse_epub_document};

    #[test]
    fn force_repack_keeps_mimetype_rules_and_all_unmodified_resource_bytes() {
        let source = minimal_epub3();
        let directory = tempdir().unwrap();
        let target = directory.path().join("roundtrip.epub");
        let file = File::create(&target).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let (_, report) = repack_epub(
            source.path(),
            file,
            &HashMap::new(),
            ArchiveLimits::default(),
            &cancelled,
        )
        .unwrap();

        assert!(report.copied_entries >= 4);
        SafeEpubArchive::open(&target, ArchiveLimits::default()).unwrap();
        verify_unchanged_resources(source.path(), &target, &HashSet::new()).unwrap();
        let original = parse_epub_document(source.path(), ArchiveLimits::default()).unwrap();
        let generated = parse_epub_document(&target, ArchiveLimits::default()).unwrap();
        assert_eq!(generated.package_resource_id, original.package_resource_id);
        assert_eq!(generated.spine, original.spine);
        assert_eq!(generated.manifest, original.manifest);
    }

    #[test]
    fn cancellation_stops_before_committing_an_archive() {
        let source = minimal_epub3();
        let directory = tempdir().unwrap();
        let target = directory.path().join("cancelled.epub");
        let cancelled = Arc::new(AtomicBool::new(true));
        let error = repack_epub(
            source.path(),
            File::create(&target).unwrap(),
            &HashMap::new(),
            ArchiveLimits::default(),
            &cancelled,
        )
        .unwrap_err();
        assert_eq!(error.to_dto().code, "SAVE_CANCELLED");
    }
}
