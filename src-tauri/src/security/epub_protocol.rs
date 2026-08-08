use percent_encoding::percent_decode_str;
use tauri::http::{
    Method, Request, Response, StatusCode,
    header::{
        CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderName,
        HeaderValue, REFERRER_POLICY,
    },
};

use crate::application::epub_document_service::EpubDocumentService;

const NOSNIFF: HeaderName = HeaderName::from_static("x-content-type-options");
const CORP: HeaderName = HeaderName::from_static("cross-origin-resource-policy");

pub(crate) fn handle_epub_protocol(
    service: &EpubDocumentService,
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

    match service.resource(&session_id, &resource_id) {
        Ok(resource) => {
            let content_length = resource.body.len();
            let is_head = request.method() == Method::HEAD;
            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, resource.content_type)
                .header(CONTENT_LENGTH, content_length.to_string())
                .header(CACHE_CONTROL, "private, no-store")
                .header(REFERRER_POLICY, "no-referrer")
                .header(NOSNIFF, HeaderValue::from_static("nosniff"))
                .header(CORP, HeaderValue::from_static("same-origin"));
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
    use super::*;

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
}
