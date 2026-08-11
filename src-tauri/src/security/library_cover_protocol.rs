use tauri::http::{Method, Request, Response, StatusCode};

use crate::{
    infrastructure::{
        archive::{
            archive_limits::ArchiveLimits,
            safe_zip::{ResourceClass, SafeArchivePath, SafeEpubArchive},
        },
        storage::local_state::LocalStateStore,
    },
    security::{
        epub_content::sanitize_svg,
        image_protocol::{
            error_response, image_signature_matches, is_supported_image_type, parse_opaque_key,
            secure_image_response,
        },
    },
};

const ERROR_BODY: &[u8] = b"Readloom library cover unavailable";

pub(crate) fn handle_library_cover_protocol(
    local_state: &LocalStateStore,
    limits: ArchiveLimits,
    webview_label: &str,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    if webview_label != "main" {
        return library_error(StatusCode::FORBIDDEN);
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return library_error(StatusCode::METHOD_NOT_ALLOWED);
    }
    let Some(cover_key) = parse_opaque_key(request.uri().path()) else {
        return library_error(StatusCode::BAD_REQUEST);
    };
    let Ok(Some(source)) = local_state.library_cover_source(&cover_key) else {
        return library_error(StatusCode::NOT_FOUND);
    };
    if !is_supported_image_type(&source.media_type, true) {
        return library_error(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let resource_path = match SafeArchivePath::parse(&source.resource_id) {
        Ok(path) => path,
        Err(_) => return library_error(StatusCode::FORBIDDEN),
    };
    let body = SafeEpubArchive::open(&source.path, limits)
        .and_then(|archive| archive.read(&resource_path, ResourceClass::Image))
        .and_then(|body| validated_cover_body(&source.media_type, body));
    let Ok(mut body) = body else {
        return library_error(StatusCode::NOT_FOUND);
    };
    let mut content_type = source.media_type;
    let mut content_security_policy = None;
    if content_type == "image/svg+xml" {
        let Ok(svg) = String::from_utf8(body) else {
            return library_error(StatusCode::UNSUPPORTED_MEDIA_TYPE);
        };
        let Ok(sanitized) = sanitize_svg(&svg, &source.resource_id, &cover_key) else {
            return library_error(StatusCode::UNSUPPORTED_MEDIA_TYPE);
        };
        body = sanitized.into_bytes();
        content_type = "image/svg+xml; charset=utf-8".to_owned();
        content_security_policy =
            Some("default-src 'none'; style-src 'unsafe-inline'; img-src data:");
    }

    secure_image_response(
        request.method(),
        content_type,
        body,
        content_security_policy,
    )
}

fn validated_cover_body(
    media_type: &str,
    body: Vec<u8>,
) -> Result<Vec<u8>, crate::error::AppError> {
    if image_signature_matches(media_type, &body, true) {
        Ok(body)
    } else {
        Err(crate::error::AppError::validation(
            "UNSUPPORTED_MEDIA_TYPE",
            "EPUB 封面内容与声明的媒体类型不一致。",
            "已改用默认封面显示。",
        ))
    }
}

fn library_error(status: StatusCode) -> Response<Vec<u8>> {
    error_response(status, ERROR_BODY)
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use tauri::http::Request;
    use tauri::http::header::CONTENT_TYPE;
    use tempfile::tempdir;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::infrastructure::storage::local_state::{LibraryDocumentRecord, LocalStateStore};

    #[test]
    fn serves_a_validated_library_cover_by_opaque_key() {
        let directory = tempdir().unwrap();
        let epub_path = directory.path().join("cover.epub");
        let mut writer = ZipWriter::new(File::create(&epub_path).unwrap());
        writer
            .start_file(
                "mimetype",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(b"application/epub+zip").unwrap();
        writer
            .start_file(
                "OEBPS/images/cover.png",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(b"\x89PNG\r\n\x1a\nfixture").unwrap();
        writer.finish().unwrap();
        let store = LocalStateStore::open(&directory.path().join("state.sqlite3")).unwrap();
        store
            .record_library_document(LibraryDocumentRecord {
                path: &epub_path,
                document_kind: "epub",
                display_title: "封面测试",
                author: None,
                fingerprint: Some("cover-fingerprint"),
                cover_resource_id: Some("OEBPS/images/cover.png"),
                cover_media_type: Some("image/png"),
            })
            .unwrap();
        let snapshot = store.library_snapshot(10).unwrap();
        let cover_key = snapshot.documents[0].cover_key.as_ref().unwrap();
        let request = Request::builder()
            .method(Method::GET)
            .uri(format!("http://readloom-library.localhost/{cover_key}"))
            .body(Vec::new())
            .unwrap();

        let response =
            handle_library_cover_protocol(&store, ArchiveLimits::default(), "main", request);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/png");
        assert!(response.body().starts_with(b"\x89PNG"));
    }

    #[test]
    fn rejects_non_hex_cover_keys_and_non_main_webviews() {
        assert!(parse_opaque_key("/not-a-cover-key").is_none());
        let directory = tempdir().unwrap();
        let store = LocalStateStore::open(&directory.path().join("state.sqlite3")).unwrap();
        let request = Request::builder()
            .uri(format!(
                "http://readloom-library.localhost/{}",
                "a".repeat(64)
            ))
            .body(Vec::new())
            .unwrap();
        assert_eq!(
            handle_library_cover_protocol(&store, ArchiveLimits::default(), "other", request)
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}
