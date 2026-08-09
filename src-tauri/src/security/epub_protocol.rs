use percent_encoding::percent_decode_str;
use tauri::http::{
    Method, Request, Response, StatusCode,
    header::{
        ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY,
        CONTENT_TYPE, HeaderName, HeaderValue, REFERRER_POLICY,
    },
};

use crate::application::{
    epub_document_service::EpubDocumentService, epub_edit_service::EpubEditService,
};

const NOSNIFF: HeaderName = HeaderName::from_static("x-content-type-options");
const CORP: HeaderName = HeaderName::from_static("cross-origin-resource-policy");

pub(crate) fn handle_epub_protocol(
    service: &EpubDocumentService,
    edits: &EpubEditService,
    webview_label: &str,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    if webview_label != "main" {
        return error_response(StatusCode::FORBIDDEN);
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return error_response(StatusCode::METHOD_NOT_ALLOWED);
    }
    let Some((session_id, resource_id)) = parse_request_path(request.uri().path()) else {
        return error_response(StatusCode::BAD_REQUEST);
    };

    let resource = edits
        .cover_resource(&session_id, &resource_id)
        .and_then(|cover| {
            if cover.is_some() {
                return Ok(cover);
            }
            edits.chapter_image_resource(&session_id, &resource_id)
        })
        .and_then(|direct| {
            if let Some(direct) = direct {
                return Ok(direct);
            }
            let body_override = edits.chapter_override(&session_id, &resource_id)?;
            service.resource_with_override(&session_id, &resource_id, body_override)
        });
    match resource {
        Ok(resource) => {
            let content_length = resource.body.len();
            let is_head = request.method() == Method::HEAD;
            let is_font = is_font_content_type(&resource.content_type);
            let cross_origin_resource_policy =
                if resource.content_type.starts_with("image/") || is_font {
                    "cross-origin"
                } else {
                    "same-origin"
                };
            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, resource.content_type)
                .header(CONTENT_LENGTH, content_length.to_string())
                .header(CACHE_CONTROL, "private, no-store")
                .header(REFERRER_POLICY, "no-referrer")
                .header(NOSNIFF, HeaderValue::from_static("nosniff"))
                .header(CORP, cross_origin_resource_policy);
            if is_font {
                builder = builder.header(ACCESS_CONTROL_ALLOW_ORIGIN, "*");
            }
            if let Some(csp) = resource.content_security_policy {
                builder = builder.header(CONTENT_SECURITY_POLICY, csp);
            }
            builder
                .body(if is_head { Vec::new() } else { resource.body })
                .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR))
        }
        Err(error) => {
            let code = error.to_dto().code;
            let status = match code {
                "EPUB_SESSION_EXPIRED" => StatusCode::GONE,
                "RESOURCE_NOT_FOUND" => StatusCode::NOT_FOUND,
                "RESOURCE_BLOCKED" | "UNSAFE_ARCHIVE_PATH" => StatusCode::FORBIDDEN,
                _ => StatusCode::BAD_REQUEST,
            };
            error_response(status)
        }
    }
}

fn is_font_content_type(content_type: &str) -> bool {
    matches!(
        content_type,
        "font/ttf"
            | "font/otf"
            | "font/woff"
            | "font/woff2"
            | "application/vnd.ms-opentype"
            | "application/font-sfnt"
    )
}

fn parse_request_path(path: &str) -> Option<(String, String)> {
    let raw = path.strip_prefix('/')?;
    let decoded = percent_decode_str(raw).decode_utf8().ok()?;
    if percent_decode_str(decoded.as_ref())
        .decode_utf8()
        .ok()?
        .as_ref()
        != decoded.as_ref()
    {
        return None;
    }
    let (session_id, resource_id) = decoded.split_once('/')?;
    if session_id.len() != 48
        || !session_id.chars().all(|value| value.is_ascii_hexdigit())
        || resource_id.is_empty()
    {
        return None;
    }
    Some((session_id.to_owned(), resource_id.to_owned()))
}

fn error_response(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(CACHE_CONTROL, "no-store")
        .header(REFERRER_POLICY, "no-referrer")
        .header(NOSNIFF, HeaderValue::from_static("nosniff"))
        .body(b"Readloom EPUB resource unavailable".to_vec())
        .expect("static protocol error response")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::{
        application::{
            epub_document_service::EpubDocumentService, epub_edit_service::EpubEditService,
        },
        epub_test_fixtures::minimal_epub3,
        infrastructure::archive::archive_limits::ArchiveLimits,
    };

    #[test]
    fn protocol_path_never_double_decodes_or_accepts_short_sessions() {
        assert!(parse_request_path("/short/EPUB/chapter.xhtml").is_none());
        assert!(
            parse_request_path(
                "/0123456789abcdef0123456789abcdef0123456789abcdef/EPUB/%252e%252e/private"
            )
            .is_none()
        );
    }

    #[test]
    fn draft_cover_images_can_render_in_the_tauri_host_without_relaxing_xhtml() {
        let fixture = minimal_epub3();
        let documents = EpubDocumentService::new(ArchiveLimits::default());
        let edits = EpubEditService::new(ArchiveLimits::default(), documents.clone());
        let opened = documents.open(fixture.path()).unwrap();
        let draft = edits.begin(&opened.document_id).unwrap();
        let directory = tempdir().unwrap();
        let cover_path = directory.path().join("preview.png");
        let mut cover = vec![0_u8; 33];
        cover[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        cover[12..16].copy_from_slice(b"IHDR");
        cover[16..20].copy_from_slice(&120_u32.to_be_bytes());
        cover[20..24].copy_from_slice(&180_u32.to_be_bytes());
        fs::write(&cover_path, cover).unwrap();
        let changed = edits
            .replace_cover(&draft.edit_session_id, draft.revision, &cover_path)
            .unwrap();
        let preview = changed.cover.preview_resource_id.unwrap();

        let cover_request = Request::builder()
            .method(Method::GET)
            .uri(format!(
                "http://readloom-epub.localhost/{}/{}",
                opened.session_id, preview
            ))
            .body(Vec::new())
            .unwrap();
        let cover_response = handle_epub_protocol(&documents, &edits, "main", cover_request);
        assert_eq!(cover_response.status(), StatusCode::OK);
        assert_eq!(cover_response.headers().get(&CORP).unwrap(), "cross-origin");

        let chapter_request = Request::builder()
            .method(Method::GET)
            .uri(format!(
                "http://readloom-epub.localhost/{}/EPUB/chapter.xhtml",
                opened.session_id
            ))
            .body(Vec::new())
            .unwrap();
        let chapter_response = handle_epub_protocol(&documents, &edits, "main", chapter_request);
        assert_eq!(chapter_response.status(), StatusCode::OK);
        assert_eq!(
            chapter_response.headers().get(&CORP).unwrap(),
            "same-origin"
        );
    }

    #[test]
    fn only_validated_font_media_types_receive_font_cors_policy() {
        for media_type in [
            "font/ttf",
            "font/otf",
            "font/woff",
            "font/woff2",
            "application/vnd.ms-opentype",
            "application/font-sfnt",
        ] {
            assert!(is_font_content_type(media_type));
        }
        for media_type in ["application/xhtml+xml", "text/css", "image/png"] {
            assert!(!is_font_content_type(media_type));
        }
    }
}
