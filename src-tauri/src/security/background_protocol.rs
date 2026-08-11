use std::fs;

use tauri::http::{Method, Request, Response, StatusCode};

use crate::{
    infrastructure::storage::local_state::LocalStateStore,
    security::image_protocol::{
        error_response, image_signature_matches, is_supported_image_type, parse_opaque_key,
        secure_image_response,
    },
};

const MAX_BACKGROUND_BYTES: u64 = 20 * 1024 * 1024;
const ERROR_BODY: &[u8] = b"Readloom background unavailable";

pub(crate) fn handle_background_protocol(
    local_state: &LocalStateStore,
    webview_label: &str,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    if webview_label != "main" {
        return error_response(StatusCode::FORBIDDEN, ERROR_BODY);
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return error_response(StatusCode::METHOD_NOT_ALLOWED, ERROR_BODY);
    }
    let Some(key) = parse_opaque_key(request.uri().path()) else {
        return error_response(StatusCode::BAD_REQUEST, ERROR_BODY);
    };
    let Ok(Some(source)) = local_state.background_image_source(&key) else {
        return error_response(StatusCode::NOT_FOUND, ERROR_BODY);
    };
    if !matches!(
        source.media_type.as_str(),
        "image/png" | "image/jpeg" | "image/webp"
    ) || !is_supported_image_type(&source.media_type, false)
    {
        return error_response(StatusCode::UNSUPPORTED_MEDIA_TYPE, ERROR_BODY);
    }
    let Ok(metadata) = fs::metadata(&source.path) else {
        return error_response(StatusCode::NOT_FOUND, ERROR_BODY);
    };
    if metadata.len() == 0 || metadata.len() > MAX_BACKGROUND_BYTES {
        return error_response(StatusCode::UNSUPPORTED_MEDIA_TYPE, ERROR_BODY);
    }
    let Ok(body) = fs::read(&source.path) else {
        return error_response(StatusCode::NOT_FOUND, ERROR_BODY);
    };
    if !image_signature_matches(&source.media_type, &body, false) {
        return error_response(StatusCode::UNSUPPORTED_MEDIA_TYPE, ERROR_BODY);
    }
    secure_image_response(request.method(), source.media_type, body, None)
}
