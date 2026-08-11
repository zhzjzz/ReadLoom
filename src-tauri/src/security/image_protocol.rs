use percent_encoding::percent_decode_str;
use tauri::http::{
    Method, Response, StatusCode,
    header::{
        CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderName,
        HeaderValue, REFERRER_POLICY,
    },
};

const NOSNIFF: HeaderName = HeaderName::from_static("x-content-type-options");
const CORP: HeaderName = HeaderName::from_static("cross-origin-resource-policy");

pub(crate) fn parse_opaque_key(path: &str) -> Option<String> {
    let raw = path.strip_prefix('/')?;
    let decoded = percent_decode_str(raw).decode_utf8().ok()?;
    if decoded.len() != 64
        || !decoded
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return None;
    }
    Some(decoded.to_ascii_lowercase())
}

pub(crate) fn is_supported_image_type(media_type: &str, allow_svg: bool) -> bool {
    matches!(
        media_type,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    ) || (allow_svg && media_type == "image/svg+xml")
}

pub(crate) fn image_signature_matches(media_type: &str, body: &[u8], allow_svg: bool) -> bool {
    match media_type {
        "image/png" => body.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => body.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => body.starts_with(b"GIF87a") || body.starts_with(b"GIF89a"),
        "image/webp" => body.len() >= 12 && body.starts_with(b"RIFF") && &body[8..12] == b"WEBP",
        "image/svg+xml" if allow_svg => std::str::from_utf8(body)
            .ok()
            .is_some_and(|source| source.to_ascii_lowercase().contains("<svg")),
        _ => false,
    }
}

pub(crate) fn secure_image_response(
    method: &Method,
    content_type: String,
    body: Vec<u8>,
    content_security_policy: Option<&str>,
) -> Response<Vec<u8>> {
    let length = body.len();
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, length.to_string())
        .header(CACHE_CONTROL, "private, no-store")
        .header(REFERRER_POLICY, "no-referrer")
        .header(NOSNIFF, HeaderValue::from_static("nosniff"))
        .header(CORP, HeaderValue::from_static("cross-origin"));
    if let Some(policy) = content_security_policy {
        builder = builder.header(CONTENT_SECURITY_POLICY, policy);
    }
    builder
        .body(if method == Method::HEAD {
            Vec::new()
        } else {
            body
        })
        .unwrap_or_else(|_| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                b"Readloom image unavailable",
            )
        })
}

pub(crate) fn error_response(status: StatusCode, message: &'static [u8]) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(CACHE_CONTROL, "no-store")
        .header(REFERRER_POLICY, "no-referrer")
        .header(NOSNIFF, HeaderValue::from_static("nosniff"))
        .body(message.to_vec())
        .expect("static protocol error response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_keys_and_raster_signatures_are_strict() {
        assert_eq!(
            parse_opaque_key(&format!("/{}", "A".repeat(64))),
            Some("a".repeat(64))
        );
        assert!(parse_opaque_key("/not-a-key").is_none());
        assert!(image_signature_matches(
            "image/png",
            b"\x89PNG\r\n\x1a\nrest",
            false
        ));
        assert!(!image_signature_matches("image/png", b"not png", false));
    }
}
