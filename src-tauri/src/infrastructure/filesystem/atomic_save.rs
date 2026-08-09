use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

use super::{FileFingerprint, fingerprint_file};

static ARTIFACT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveJournal {
    target: PathBuf,
    temporary: PathBuf,
    backup: Option<PathBuf>,
    old_hash: Option<String>,
    new_hash: String,
}

pub fn atomic_save(
    target: &Path,
    bytes: &[u8],
    expected: Option<&FileFingerprint>,
) -> Result<FileFingerprint, AppError> {
    validate_target_parent(target)?;
    reconcile_interrupted_save(target)?;

    let existed = target.exists();
    if existed
        && fs::metadata(target)
            .map_err(map_target_error)?
            .permissions()
            .readonly()
    {
        return Err(AppError::validation(
            "PERMISSION_DENIED",
            "目标文件是只读文件。",
            "取消只读属性，或使用“另存为”。",
        ));
    }

    let (mut temporary_file, temporary_path) = create_unique_artifact(target, "tmp", true)?;
    if let Err(error) = write_and_sync(&mut temporary_file, bytes) {
        drop(temporary_file);
        remove_if_present(&temporary_path);
        return Err(map_temporary_error(&error));
    }
    drop(temporary_file);

    let temporary_bytes = fs::read(&temporary_path).map_err(|error| {
        remove_if_present(&temporary_path);
        map_temporary_error(&error)
    })?;
    if temporary_bytes != bytes {
        remove_if_present(&temporary_path);
        return Err(AppError::validation(
            "SAVE_VALIDATION_FAILED",
            "临时文件校验失败，原文件没有被替换。",
            "检查磁盘状态后重试。",
        ));
    }

    commit_prepared_output(target, &temporary_path, expected)
}

pub(crate) fn create_prepared_output(target: &Path) -> Result<(File, PathBuf), AppError> {
    validate_target_parent(target)?;
    reconcile_interrupted_save(target)?;
    create_unique_artifact(target, "epub", true)
}

pub(crate) fn commit_prepared_output(
    target: &Path,
    temporary_path: &Path,
    expected: Option<&FileFingerprint>,
) -> Result<FileFingerprint, AppError> {
    validate_target_parent(target)?;
    reconcile_interrupted_save(target)?;
    if !temporary_path.exists() || !is_safe_artifact(target, temporary_path) {
        return Err(AppError::validation(
            "TEMPORARY_OUTPUT_FAILED",
            "EPUB 临时输出文件无效。",
            "请重新选择保存位置后重试。",
        ));
    }
    let existed = target.exists();
    if existed
        && fs::metadata(target)
            .map_err(map_target_error)?
            .permissions()
            .readonly()
    {
        remove_if_present(temporary_path);
        return Err(AppError::validation(
            "PERMISSION_DENIED",
            "目标文件是只读文件。",
            "取消只读属性，或选择其他文件名。",
        ));
    }

    let current = if existed {
        Some(fingerprint_file(target).map_err(map_fingerprint_error)?)
    } else {
        None
    };
    match (expected, current.as_ref()) {
        (Some(expected), Some(current)) if expected != current => {
            remove_if_present(temporary_path);
            return Err(external_modification_error());
        }
        (Some(_), None) => {
            remove_if_present(temporary_path);
            return Err(external_modification_error());
        }
        (None, Some(_)) => {
            remove_if_present(temporary_path);
            return Err(AppError::validation(
                "DESTINATION_EXISTS",
                "另存为目标在确认后发生了变化。",
                "重新选择文件名并确认是否覆盖。",
            ));
        }
        _ => {}
    }

    let backup_path = if existed {
        Some(create_unique_artifact(target, "backup", false)?.1)
    } else {
        None
    };
    let new_hash = file_hash(temporary_path).ok_or_else(|| {
        remove_if_present(temporary_path);
        AppError::validation(
            "TEMPORARY_OUTPUT_FAILED",
            "无法校验 EPUB 临时输出文件。",
            "检查磁盘状态后重试。",
        )
    })?;
    let journal = SaveJournal {
        target: target.to_owned(),
        temporary: temporary_path.to_owned(),
        backup: backup_path.clone(),
        old_hash: current
            .as_ref()
            .map(|fingerprint| fingerprint.blake3.clone()),
        new_hash,
    };
    let journal_path = journal_path(target)?;
    if let Err(error) = write_journal(&journal_path, &journal) {
        remove_if_present(temporary_path);
        return Err(AppError::validation(
            "BACKUP_FAILED",
            format!("无法创建保存恢复记录：{error}"),
            "确认目标目录可写并有足够空间后重试。",
        ));
    }

    let replace_result = if existed {
        replace_existing(target, temporary_path, backup_path.as_deref())
    } else {
        move_new_file(temporary_path, target)
    };
    if let Err(error) = replace_result {
        if file_hash(target).as_deref() == Some(journal.new_hash.as_str()) {
            cleanup_journal_artifacts(&journal_path, &journal);
            return fingerprint_file(target).map_err(map_fingerprint_error);
        }
        if file_hash(target).as_deref() == journal.old_hash.as_deref() {
            cleanup_journal_artifacts(&journal_path, &journal);
        }
        return Err(map_replace_error(&error));
    }

    if file_hash(target).as_deref() != Some(journal.new_hash.as_str()) {
        let restored = restore_original(target, &journal);
        if restored {
            cleanup_journal_artifacts(&journal_path, &journal);
        }
        return Err(AppError::validation(
            "SAVE_VALIDATION_FAILED",
            "替换后的文件校验失败。",
            if restored {
                "已恢复原文件，请检查磁盘后重试。"
            } else {
                "恢复记录已保留，请不要移动同目录中的 Readloom 恢复文件。"
            },
        ));
    }

    let fingerprint = fingerprint_file(target).map_err(map_fingerprint_error)?;
    cleanup_journal_artifacts(&journal_path, &journal);
    Ok(fingerprint)
}

pub fn reconcile_interrupted_save(target: &Path) -> Result<(), AppError> {
    let journal_path = journal_path(target)?;
    if !journal_path.exists() {
        return Ok(());
    }
    let journal_bytes = fs::read(&journal_path).map_err(|_| recovery_available_error())?;
    let journal: SaveJournal =
        serde_json::from_slice(&journal_bytes).map_err(|_| recovery_available_error())?;
    if journal.target != target
        || !is_safe_artifact(target, &journal.temporary)
        || journal
            .backup
            .as_deref()
            .is_some_and(|backup| !is_safe_artifact(target, backup))
    {
        return Err(recovery_available_error());
    }

    let current_hash = file_hash(target);
    if current_hash.as_deref() == Some(journal.new_hash.as_str())
        || current_hash.as_deref() == journal.old_hash.as_deref()
    {
        cleanup_journal_artifacts(&journal_path, &journal);
        return Ok(());
    }
    if restore_original(target, &journal) {
        cleanup_journal_artifacts(&journal_path, &journal);
        return Ok(());
    }
    Err(recovery_available_error())
}

fn validate_target_parent(target: &Path) -> Result<(), AppError> {
    let parent = target.parent().ok_or_else(|| {
        AppError::validation(
            "INVALID_PATH",
            "目标路径没有有效的父目录。",
            "请选择一个有效的 .txt 保存位置。",
        )
    })?;
    let metadata = fs::metadata(parent).map_err(map_target_error)?;
    if !metadata.is_dir() {
        return Err(AppError::validation(
            "INVALID_PATH",
            "目标文件的父路径不是目录。",
            "请选择一个有效的文件夹。",
        ));
    }
    Ok(())
}

fn create_unique_artifact(
    target: &Path,
    kind: &str,
    create: bool,
) -> Result<(File, PathBuf), AppError> {
    let parent = target.parent().ok_or_else(|| {
        AppError::validation("INVALID_PATH", "目标路径无效。", "请选择有效的保存位置。")
    })?;
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            AppError::validation(
                "INVALID_PATH",
                "目标文件名不是有效的 Unicode 文本。",
                "请选择其他文件名。",
            )
        })?;
    for _ in 0..256 {
        let id = ARTIFACT_ID.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{file_name}.readloom-{kind}-{}-{id:016x}.tmp",
            std::process::id()
        ));
        if create {
            match OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&candidate)
            {
                Ok(file) => return Ok((file, candidate)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(map_temporary_error(&error)),
            }
        } else if !candidate.exists() {
            let placeholder = OpenOptions::new()
                .read(true)
                .open(target)
                .map_err(map_target_error)?;
            return Ok((placeholder, candidate));
        }
    }
    Err(AppError::validation(
        "TEMPORARY_FILE_FAILED",
        "无法创建不冲突的临时文件名。",
        "清理目标目录中的旧 Readloom 临时文件后重试。",
    ))
}

fn write_and_sync(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()
}

fn journal_path(target: &Path) -> Result<PathBuf, AppError> {
    let parent = target.parent().ok_or_else(|| {
        AppError::validation("INVALID_PATH", "目标路径无效。", "请选择有效的保存位置。")
    })?;
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            AppError::validation("INVALID_PATH", "目标文件名无效。", "请选择其他文件名。")
        })?;
    Ok(parent.join(format!(".{file_name}.readloom-save.json")))
}

fn write_journal(path: &Path, journal: &SaveJournal) -> io::Result<()> {
    let bytes = serde_json::to_vec(journal).map_err(io::Error::other)?;
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.sync_all()
}

fn cleanup_journal_artifacts(journal_path: &Path, journal: &SaveJournal) {
    remove_if_present(&journal.temporary);
    if let Some(backup) = &journal.backup {
        remove_if_present(backup);
    }
    remove_if_present(journal_path);
}

fn restore_original(target: &Path, journal: &SaveJournal) -> bool {
    let Some(backup) = journal.backup.as_deref() else {
        return false;
    };
    if file_hash(backup).as_deref() != journal.old_hash.as_deref() {
        return false;
    }
    let result = if target.exists() {
        let rollback = create_unique_artifact(target, "rollback", false)
            .ok()
            .map(|(_, path)| path);
        replace_existing(target, backup, rollback.as_deref())
    } else {
        move_new_file(backup, target)
    };
    result.is_ok() && file_hash(target).as_deref() == journal.old_hash.as_deref()
}

fn file_hash(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
}

fn is_safe_artifact(target: &Path, artifact: &Path) -> bool {
    if target.parent() != artifact.parent() {
        return false;
    }
    let Some(target_name) = target.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    artifact
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(&format!(".{target_name}.readloom-")))
}

fn remove_if_present(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

fn external_modification_error() -> AppError {
    AppError::validation(
        "EXTERNAL_MODIFICATION",
        "文件已被其他程序修改，Readloom 没有覆盖它。",
        "请选择另存为、重新加载或取消。",
    )
}

fn recovery_available_error() -> AppError {
    AppError::validation(
        "RECOVERY_AVAILABLE",
        "检测到未完成的保存，无法自动判断应保留哪个版本。",
        "不要移动同目录中的 Readloom 恢复文件；请先备份该目录再处理。",
    )
}

fn map_target_error(error: io::Error) -> AppError {
    match error.kind() {
        io::ErrorKind::NotFound => AppError::validation(
            "FILE_NOT_FOUND",
            "目标文件或目录不存在。",
            "确认路径后重试。",
        ),
        io::ErrorKind::PermissionDenied => AppError::validation(
            "PERMISSION_DENIED",
            "没有访问目标文件或目录的权限。",
            "检查权限，或选择其他保存位置。",
        ),
        _ => AppError::internal("INTERNAL", "inspect save target"),
    }
}

fn map_temporary_error(error: &io::Error) -> AppError {
    match error.kind() {
        io::ErrorKind::PermissionDenied => AppError::validation(
            "PERMISSION_DENIED",
            "目标目录不可写。",
            "检查目录权限，或选择其他保存位置。",
        ),
        _ => AppError::validation(
            "TEMPORARY_FILE_FAILED",
            "无法创建或写入安全保存临时文件。",
            "检查磁盘空间、目录权限和安全软件后重试。",
        ),
    }
}

fn map_fingerprint_error(error: io::Error) -> AppError {
    match error.kind() {
        io::ErrorKind::NotFound => external_modification_error(),
        io::ErrorKind::PermissionDenied => AppError::validation(
            "PERMISSION_DENIED",
            "保存前无法读取目标文件。",
            "检查权限，或使用“另存为”。",
        ),
        _ => external_modification_error(),
    }
}

fn map_replace_error(error: &io::Error) -> AppError {
    match error.raw_os_error() {
        Some(5) => AppError::validation(
            "PERMISSION_DENIED",
            "Windows 拒绝替换目标文件。",
            "检查文件属性和目录权限，或使用“另存为”。",
        ),
        Some(32 | 33) => AppError::validation(
            "FILE_LOCKED",
            "文件正被其他程序锁定，原文件没有被覆盖。",
            "关闭占用该文件的程序后重试。",
        ),
        Some(112) => AppError::validation(
            "TEMPORARY_FILE_FAILED",
            "磁盘空间不足，无法完成保存。",
            "释放磁盘空间后重试。",
        ),
        _ => AppError::validation(
            "REPLACE_FAILED",
            "Windows 无法安全替换目标文件。",
            "原文件或恢复记录已保留；检查文件占用和磁盘状态后重试。",
        ),
    }
}

#[cfg(windows)]
fn replace_existing(target: &Path, replacement: &Path, backup: Option<&Path>) -> io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};

    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let target = wide(target);
    let replacement = wide(replacement);
    let backup = backup.map(wide);
    let backup_ptr = backup.as_ref().map_or(ptr::null(), |path| path.as_ptr());
    let succeeded = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            replacement.as_ptr(),
            backup_ptr,
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            ptr::null(),
            ptr::null(),
        )
    };
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn move_new_file(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let source = wide(source);
    let target = wide(target);
    let succeeded =
        unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_existing(target: &Path, replacement: &Path, backup: Option<&Path>) -> io::Result<()> {
    if let Some(backup) = backup {
        fs::copy(target, backup)?;
    }
    fs::rename(replacement, target)
}

#[cfg(not(windows))]
fn move_new_file(source: &Path, target: &Path) -> io::Result<()> {
    fs::rename(source, target)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir_in;

    use super::*;

    #[test]
    fn unique_temporary_names_do_not_collide() {
        let directory = tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target"))
            .expect("temporary directory");
        let target = directory.path().join("sample.txt");
        fs::write(&target, b"old").expect("fixture");
        let (first_file, first) = create_unique_artifact(&target, "tmp", true).expect("first");
        let (second_file, second) = create_unique_artifact(&target, "tmp", true).expect("second");
        drop(first_file);
        drop(second_file);

        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());
    }
}
