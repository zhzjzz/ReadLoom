use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{CoreError, ReadloomCore};

const MANIFEST_NAME: &str = "readloom-books-v1.json";
const MAXIMUM_BOOK_BYTES: u64 = 1024 * 1024 * 1024;
const MAXIMUM_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupSummary {
    pub source_books: usize,
    pub unique_contents: usize,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreSummary {
    pub restored_books: usize,
    pub skipped_duplicates: usize,
    pub output_directory: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
struct BackupManifest {
    version: u8,
    books: Vec<BackupBook>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupBook {
    sha256: String,
    archive_path: String,
    original_name: String,
    kind: String,
    size: u64,
}

struct SourceBook {
    path: PathBuf,
    metadata: BackupBook,
}

impl ReadloomCore {
    pub fn create_books_backup(&self, target: &Path) -> Result<BackupSummary, CoreError> {
        if target.extension().and_then(|value| value.to_str()) != Some("readloom-backup") {
            return Err(CoreError::Validation(
                "备份文件必须使用 .readloom-backup 扩展名。".to_owned(),
            ));
        }
        if target.exists() {
            return Err(CoreError::Validation(
                "目标备份已存在，请选择新文件名以避免覆盖。".to_owned(),
            ));
        }
        let parent = target
            .parent()
            .ok_or_else(|| CoreError::Validation("备份路径没有有效父目录。".to_owned()))?;
        fs::create_dir_all(parent)?;
        let snapshot = self.library_snapshot(100_000)?;
        let source_books = snapshot.documents.len();
        let mut seen = HashSet::new();
        let mut sources = Vec::new();
        for document in snapshot.documents.into_iter().filter(|book| book.available) {
            let path = PathBuf::from(&document.path);
            let metadata = fs::metadata(&path)?;
            if !metadata.is_file() || metadata.len() > MAXIMUM_BOOK_BYTES {
                continue;
            }
            let sha256 = sha256_file(&path)?;
            if !seen.insert(sha256.clone()) {
                continue;
            }
            let extension = if document.document_kind == "epub" {
                "epub"
            } else {
                "txt"
            };
            sources.push(SourceBook {
                path,
                metadata: BackupBook {
                    archive_path: format!("books/{sha256}.{extension}"),
                    original_name: Path::new(&document.path)
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("book")
                        .to_owned(),
                    kind: extension.to_owned(),
                    size: metadata.len(),
                    sha256,
                },
            });
        }
        let temp = sibling_temp_path(target, "backup");
        let result = write_archive(&temp, &sources);
        if let Err(error) = result {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        fs::rename(&temp, target)?;
        Ok(BackupSummary {
            source_books,
            unique_contents: sources.len(),
            output_path: target.to_path_buf(),
        })
    }

    pub fn restore_books_backups(
        &self,
        backups: &[PathBuf],
        output_directory: &Path,
    ) -> Result<RestoreSummary, CoreError> {
        if backups.is_empty() {
            return Err(CoreError::Validation("请至少选择一个备份文件。".to_owned()));
        }
        fs::create_dir_all(output_directory)?;
        let output_directory = fs::canonicalize(output_directory)?;
        let mut seen = HashSet::new();
        let mut restored = 0;
        let mut skipped = 0;
        for backup in backups {
            let file = File::open(backup)?;
            let mut archive = ZipArchive::new(file)
                .map_err(|_| CoreError::Validation("备份 ZIP 结构无效。".to_owned()))?;
            let manifest = read_manifest(&mut archive)?;
            for book in manifest.books {
                validate_book(&book)?;
                if !seen.insert(book.sha256.clone()) {
                    skipped += 1;
                    continue;
                }
                let target = unused_target(&output_directory, &book.original_name, &book.kind);
                let temp = sibling_temp_path(&target, "restore");
                let mut entry = archive
                    .by_name(&book.archive_path)
                    .map_err(|_| CoreError::Validation("备份缺少清单所列书籍。".to_owned()))?;
                if entry.size() != book.size || entry.size() > MAXIMUM_BOOK_BYTES {
                    return Err(CoreError::Validation(
                        "备份书籍大小与清单不一致。".to_owned(),
                    ));
                }
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temp)?;
                let mut hasher = Sha256::new();
                let mut copied = 0_u64;
                let mut buffer = [0_u8; 64 * 1024];
                loop {
                    let count = entry.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    copied = copied.saturating_add(count as u64);
                    if copied > book.size || copied > MAXIMUM_BOOK_BYTES {
                        let _ = fs::remove_file(&temp);
                        return Err(CoreError::Validation("备份书籍解压大小异常。".to_owned()));
                    }
                    hasher.update(&buffer[..count]);
                    output.write_all(&buffer[..count])?;
                }
                output.sync_all()?;
                drop(output);
                let digest = format!("{:x}", hasher.finalize());
                if copied != book.size || digest != book.sha256 {
                    let _ = fs::remove_file(&temp);
                    return Err(CoreError::Validation(
                        "备份书籍内容哈希校验失败。".to_owned(),
                    ));
                }
                fs::rename(&temp, &target)?;
                let opened = if book.kind == "epub" {
                    self.open_epub(&target).map(|_| ())
                } else {
                    self.open_txt(&target).map(|_| ())
                };
                if let Err(error) = opened {
                    let _ = fs::remove_file(&target);
                    return Err(error);
                }
                restored += 1;
            }
        }
        Ok(RestoreSummary {
            restored_books: restored,
            skipped_duplicates: skipped,
            output_directory,
        })
    }
}

fn write_archive(path: &Path, sources: &[SourceBook]) -> Result<(), CoreError> {
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for source in sources {
        writer
            .start_file(&source.metadata.archive_path, options)
            .map_err(|_| CoreError::Validation("无法写入备份 ZIP。".to_owned()))?;
        let mut input = BufReader::new(File::open(&source.path)?);
        std::io::copy(&mut input, &mut writer)?;
    }
    writer
        .start_file(MANIFEST_NAME, options)
        .map_err(|_| CoreError::Validation("无法写入备份清单。".to_owned()))?;
    let manifest = BackupManifest {
        version: 1,
        books: sources
            .iter()
            .map(|source| source.metadata.clone())
            .collect(),
    };
    serde_json::to_writer(&mut writer, &manifest)
        .map_err(|_| CoreError::Validation("无法序列化备份清单。".to_owned()))?;
    writer
        .finish()
        .map_err(|_| CoreError::Validation("无法完成备份 ZIP。".to_owned()))?
        .sync_all()?;
    Ok(())
}

fn read_manifest(archive: &mut ZipArchive<File>) -> Result<BackupManifest, CoreError> {
    let mut entry = archive
        .by_name(MANIFEST_NAME)
        .map_err(|_| CoreError::Validation("所选文件不是 Readloom 内容备份。".to_owned()))?;
    if entry.size() > MAXIMUM_MANIFEST_BYTES {
        return Err(CoreError::Validation("备份清单过大。".to_owned()));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    let manifest: BackupManifest = serde_json::from_slice(&bytes)
        .map_err(|_| CoreError::Validation("备份清单无效。".to_owned()))?;
    if manifest.version != 1 {
        return Err(CoreError::Validation("暂不支持此备份版本。".to_owned()));
    }
    Ok(manifest)
}

fn validate_book(book: &BackupBook) -> Result<(), CoreError> {
    let expected_path = format!("books/{}.{}", book.sha256, book.kind);
    if book.sha256.len() != 64
        || !book.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !matches!(book.kind.as_str(), "txt" | "epub")
        || book.archive_path != expected_path
        || book.size > MAXIMUM_BOOK_BYTES
    {
        return Err(CoreError::Validation("备份清单包含不安全条目。".to_owned()));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, CoreError> {
    let mut input = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn unused_target(directory: &Path, original_name: &str, kind: &str) -> PathBuf {
    let stem = Path::new(original_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("book")
        .chars()
        .filter(|character| {
            !matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
        })
        .take(100)
        .collect::<String>();
    let stem = if stem.trim().is_empty() {
        "book"
    } else {
        stem.trim()
    };
    for suffix in 0..100_000 {
        let name = if suffix == 0 {
            format!("{stem}.{kind}")
        } else {
            format!("{stem} ({suffix}).{kind}")
        };
        let target = directory.join(name);
        if !target.exists() {
            return target;
        }
    }
    directory.join(format!("book-{}.{}", unique_id(), kind))
}

fn sibling_temp_path(target: &Path, label: &str) -> PathBuf {
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("readloom");
    target.with_file_name(format!(".{name}.{label}-{}.tmp", unique_id()))
}

fn unique_id() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_and_restore_deduplicate_books_by_sha256() {
        let directory = tempfile::tempdir().unwrap();
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3")).unwrap();
        let first = directory.path().join("first.txt");
        let second = directory.path().join("second.txt");
        fs::write(&first, "第一章\n相同内容。\n").unwrap();
        fs::write(&second, "第一章\n相同内容。\n").unwrap();
        core.open_txt(&first).unwrap();
        core.open_txt(&second).unwrap();
        let backup = directory.path().join("books.readloom-backup");
        let summary = core.create_books_backup(&backup).unwrap();
        assert_eq!(summary.source_books, 2);
        assert_eq!(summary.unique_contents, 1);

        let restored = directory.path().join("restored");
        let result = core
            .restore_books_backups(&[backup.clone(), backup], &restored)
            .unwrap();
        assert_eq!(result.restored_books, 1);
        assert_eq!(result.skipped_duplicates, 1);
    }
}
