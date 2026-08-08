use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    config::TextDocumentLimits,
    domain::text_document::{
        DocumentId, LineEnding, OpenedTextDocument, SaveLineEnding, SavedTextDocument, TextEncoding,
    },
    error::AppError,
    formats::txt::{analyze_and_normalize, decode_text, encode_text},
    infrastructure::filesystem::{
        FileFingerprint, atomic_save, fingerprint_from_bytes, reconcile_interrupted_save,
    },
};

#[derive(Debug)]
pub struct OpenTextDocument {
    pub path: PathBuf,
    pub encoding_override: Option<TextEncoding>,
    pub allow_large: bool,
}

#[derive(Debug)]
pub struct SaveTextDocument {
    pub document_id: DocumentId,
    pub content: String,
    pub encoding: TextEncoding,
    pub has_bom: bool,
    pub line_ending: SaveLineEnding,
    pub expected_revision: u64,
}

#[derive(Debug)]
pub struct SaveTextDocumentAs {
    pub document_id: DocumentId,
    pub target_path: PathBuf,
    pub content: String,
    pub encoding: TextEncoding,
    pub has_bom: bool,
    pub line_ending: SaveLineEnding,
    pub expected_revision: u64,
    pub allow_overwrite: bool,
}

#[derive(Debug, Clone)]
struct TextDocumentSession {
    path: PathBuf,
    file_name: String,
    encoding: TextEncoding,
    has_bom: bool,
    detected_line_ending: LineEnding,
    primary_line_ending: LineEnding,
    fingerprint: FileFingerprint,
    revision: u64,
    read_only: bool,
}

pub struct TextDocumentService {
    limits: TextDocumentLimits,
    next_document_id: AtomicU64,
    sessions: Mutex<HashMap<DocumentId, TextDocumentSession>>,
}

impl TextDocumentService {
    pub fn new(limits: TextDocumentLimits) -> Self {
        Self {
            limits,
            next_document_id: AtomicU64::new(1),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn open(&self, request: OpenTextDocument) -> Result<OpenedTextDocument, AppError> {
        let canonical_path =
            fs::canonicalize(&request.path).map_err(|error| map_open_error(&error))?;
        reconcile_interrupted_save(&canonical_path)?;
        let metadata_before =
            fs::metadata(&canonical_path).map_err(|error| map_open_error(&error))?;
        if !metadata_before.is_file() {
            return Err(AppError::validation(
                "INVALID_PATH",
                "所选路径不是普通文件。",
                "请选择一个普通文件。",
            ));
        }
        self.validate_file_size(metadata_before.len(), request.allow_large)?;

        let bytes = fs::read(&canonical_path).map_err(|error| map_open_error(&error))?;
        let metadata_after =
            fs::metadata(&canonical_path).map_err(|error| map_open_error(&error))?;
        if metadata_before.len() != metadata_after.len()
            || metadata_before.modified().ok() != metadata_after.modified().ok()
            || bytes.len() as u64 != metadata_after.len()
        {
            return Err(AppError::validation(
                "EXTERNAL_MODIFICATION",
                "文件在打开过程中发生了变化。",
                "等待其他程序完成写入后重新打开。",
            ));
        }
        self.validate_file_size(metadata_after.len(), request.allow_large)?;

        let decoded = decode_text(&bytes, request.encoding_override)?;
        let line_endings = analyze_and_normalize(&decoded.content);
        let file_name = file_name(&canonical_path)?;
        let document_number = self.next_document_id.fetch_add(1, Ordering::Relaxed);
        let document_id = DocumentId(format!("txt-{document_number:016x}"));
        let fingerprint = fingerprint_from_bytes(&bytes, &metadata_after);
        let read_only = metadata_after.permissions().readonly();
        let session = TextDocumentSession {
            path: canonical_path.clone(),
            file_name: file_name.clone(),
            encoding: decoded.encoding,
            has_bom: decoded.has_bom,
            detected_line_ending: line_endings.detected,
            primary_line_ending: line_endings.primary,
            fingerprint,
            revision: 0,
            read_only,
        };
        self.sessions
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock document sessions"))?
            .insert(document_id.clone(), session);

        Ok(OpenedTextDocument {
            document_id,
            path: canonical_path,
            file_name,
            content: line_endings.normalized,
            encoding: decoded.encoding,
            has_bom: decoded.has_bom,
            line_ending: line_endings.detected,
            size_bytes: metadata_after.len(),
            read_only,
            revision: 0,
        })
    }

    pub fn save(&self, request: SaveTextDocument) -> Result<SavedTextDocument, AppError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock document sessions"))?;
        let session = sessions.get_mut(&request.document_id).ok_or_else(|| {
            AppError::validation(
                "DOCUMENT_NOT_FOUND",
                "文档会话已经关闭或失效。",
                "重新打开文件后再保存。",
            )
        })?;
        if request.expected_revision != session.revision {
            return Err(AppError::validation(
                "REVISION_CONFLICT",
                "保存请求基于过期的文档版本。",
                "等待当前保存完成后重试。",
            ));
        }
        if session.read_only {
            return Err(AppError::validation(
                "PERMISSION_DENIED",
                "当前文档是只读文件。",
                "取消只读属性，或使用“另存为”。",
            ));
        }

        let encoded = encode_text(
            &request.content,
            request.encoding,
            request.has_bom,
            request.line_ending,
            session.detected_line_ending,
            session.primary_line_ending,
        )?;
        let fingerprint = atomic_save(&session.path, &encoded.bytes, Some(&session.fingerprint))?;
        let metadata = fs::metadata(&session.path).map_err(|error| map_open_error(&error))?;
        session.encoding = request.encoding;
        session.has_bom = request.has_bom;
        session.detected_line_ending = encoded.line_ending;
        session.primary_line_ending = encoded.primary_line_ending;
        session.fingerprint = fingerprint;
        session.revision += 1;
        session.read_only = metadata.permissions().readonly();

        Ok(saved_document(&request.document_id, session))
    }

    pub fn save_as(&self, request: SaveTextDocumentAs) -> Result<SavedTextDocument, AppError> {
        validate_txt_save_path(&request.target_path)?;
        let target_path = normalize_save_target(&request.target_path)?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock document sessions"))?;
        let session = sessions.get_mut(&request.document_id).ok_or_else(|| {
            AppError::validation(
                "DOCUMENT_NOT_FOUND",
                "文档会话已经关闭或失效。",
                "重新打开文件后再保存。",
            )
        })?;
        if request.expected_revision != session.revision {
            return Err(AppError::validation(
                "REVISION_CONFLICT",
                "另存为请求基于过期的文档版本。",
                "等待当前保存完成后重试。",
            ));
        }

        let target_fingerprint = if target_path.exists() {
            let metadata = fs::metadata(&target_path).map_err(|error| map_open_error(&error))?;
            if !metadata.is_file() {
                return Err(AppError::validation(
                    "INVALID_PATH",
                    "另存为目标不是普通文件。",
                    "请选择一个 .txt 文件路径。",
                ));
            }
            if target_path != session.path && !request.allow_overwrite {
                return Err(AppError::validation(
                    "DESTINATION_EXISTS",
                    "另存为目标已经存在。",
                    "确认覆盖后重试，或选择其他文件名。",
                ));
            }
            if target_path == session.path {
                Some(session.fingerprint.clone())
            } else {
                Some(
                    crate::infrastructure::filesystem::fingerprint_file(&target_path).map_err(
                        |_| AppError::internal("INTERNAL", "fingerprint save-as target"),
                    )?,
                )
            }
        } else {
            None
        };

        let encoded = encode_text(
            &request.content,
            request.encoding,
            request.has_bom,
            request.line_ending,
            session.detected_line_ending,
            session.primary_line_ending,
        )?;
        let fingerprint = atomic_save(&target_path, &encoded.bytes, target_fingerprint.as_ref())?;
        let metadata = fs::metadata(&target_path).map_err(|error| map_open_error(&error))?;
        session.path = target_path.clone();
        session.file_name = file_name(&target_path)?;
        session.encoding = request.encoding;
        session.has_bom = request.has_bom;
        session.detected_line_ending = encoded.line_ending;
        session.primary_line_ending = encoded.primary_line_ending;
        session.fingerprint = fingerprint;
        session.revision += 1;
        session.read_only = metadata.permissions().readonly();

        Ok(saved_document(&request.document_id, session))
    }

    pub fn reopen(
        &self,
        document_id: &DocumentId,
        encoding: TextEncoding,
        allow_large: bool,
    ) -> Result<OpenedTextDocument, AppError> {
        let path = self
            .sessions
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock document sessions"))?
            .get(document_id)
            .map(|session| session.path.clone())
            .ok_or_else(|| {
                AppError::validation(
                    "DOCUMENT_NOT_FOUND",
                    "文档会话已经关闭或失效。",
                    "重新打开文件后再试。",
                )
            })?;
        let opened = self.open(OpenTextDocument {
            path,
            encoding_override: Some(encoding),
            allow_large,
        })?;
        self.close(document_id)?;
        Ok(opened)
    }

    pub fn close(&self, document_id: &DocumentId) -> Result<(), AppError> {
        let removed = self
            .sessions
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock document sessions"))?
            .remove(document_id);
        if removed.is_none() {
            return Err(AppError::validation(
                "DOCUMENT_NOT_FOUND",
                "文档会话已经关闭。",
                "无需再次关闭。",
            ));
        }
        Ok(())
    }

    fn validate_file_size(&self, size_bytes: u64, allow_large: bool) -> Result<(), AppError> {
        if size_bytes > self.limits.maximum_editable_bytes {
            return Err(AppError::validation(
                "FILE_TOO_LARGE",
                format!(
                    "文件大小为 {:.1} MiB，超过当前版本 {:.0} MiB 的完整编辑上限。",
                    size_bytes as f64 / 1_048_576.0,
                    self.limits.maximum_editable_bytes as f64 / 1_048_576.0
                ),
                "请选择较小文件；只读大文件模式将在后续阶段提供。",
            ));
        }
        if size_bytes > self.limits.confirmation_threshold_bytes && !allow_large {
            return Err(AppError::validation(
                "LARGE_FILE_CONFIRMATION_REQUIRED",
                format!(
                    "文件大小为 {:.1} MiB，完整打开可能占用较多内存。",
                    size_bytes as f64 / 1_048_576.0
                ),
                "确认后可继续打开，或取消操作。",
            ));
        }
        Ok(())
    }
}

fn saved_document(document_id: &DocumentId, session: &TextDocumentSession) -> SavedTextDocument {
    SavedTextDocument {
        document_id: document_id.clone(),
        path: session.path.clone(),
        file_name: session.file_name.clone(),
        encoding: session.encoding,
        has_bom: session.has_bom,
        line_ending: session.detected_line_ending,
        size_bytes: session.fingerprint.size_bytes,
        read_only: session.read_only,
        revision: session.revision,
    }
}

fn file_name(path: &Path) -> Result<String, AppError> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| {
            AppError::validation(
                "INVALID_PATH",
                "无法从所选路径取得文件名。",
                "请选择一个有效的 .txt 文件。",
            )
        })
}

fn normalize_save_target(path: &Path) -> Result<PathBuf, AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::validation(
            "INVALID_PATH",
            "保存路径没有有效的父目录。",
            "请选择一个有效的保存位置。",
        )
    })?;
    let canonical_parent = fs::canonicalize(parent).map_err(|error| map_open_error(&error))?;
    let target_name = path.file_name().ok_or_else(|| {
        AppError::validation(
            "INVALID_PATH",
            "保存路径缺少文件名。",
            "请输入以 .txt 结尾的文件名。",
        )
    })?;
    Ok(canonical_parent.join(target_name))
}

fn validate_txt_save_path(path: &Path) -> Result<(), AppError> {
    let is_txt = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"));
    if !is_txt {
        return Err(AppError::validation(
            "INVALID_PATH",
            "文本另存为目标必须使用 .txt 扩展名。",
            "请输入扩展名为 .txt 的文件名。",
        ));
    }
    Ok(())
}

fn map_open_error(error: &std::io::Error) -> AppError {
    match error.kind() {
        std::io::ErrorKind::NotFound => AppError::validation(
            "FILE_NOT_FOUND",
            "找不到所选文件。",
            "确认文件仍然存在后重试。",
        ),
        std::io::ErrorKind::PermissionDenied => AppError::validation(
            "PERMISSION_DENIED",
            "没有权限读取所选文件。",
            "检查文件权限或选择其他文件。",
        ),
        _ => AppError::internal("INTERNAL", "read text document"),
    }
}
