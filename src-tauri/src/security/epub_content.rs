use std::borrow::Cow;

use ammonia::{Builder, Url, UrlRelative, UrlRelativeEvaluate};
use cssparser::{ParseError, Parser, ParserInput, Token};
use percent_encoding::{
    AsciiSet, CONTROLS, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode,
};
use quick_xml::{Reader, XmlVersion, events::Event};

use crate::{error::AppError, infrastructure::archive::safe_zip::SafeArchivePath};

const URL_COMPONENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

pub(crate) fn sanitize_xhtml_fragment(
    source: &str,
    current_resource: &str,
    session_id: &str,
) -> Result<String, AppError> {
    if source.len() > 8 * 1024 * 1024 {
        return Err(AppError::validation(
            "UNSAFE_XHTML",
            "EPUB 章节超过安全处理大小。",
            "请选择章节规模正常的 EPUB 文件。",
        ));
    }

    let resolver = InternalUrlResolver {
        current_resource: current_resource.to_owned(),
        session_id: session_id.to_owned(),
    };
    let mut builder = Builder::default();
    builder
        .url_schemes(["http", "https"].into_iter().collect())
        .url_relative(UrlRelative::Custom(Box::new(resolver)))
        .attribute_filter(|element, attribute, value| {
            let external = Url::parse(value)
                .ok()
                .filter(|url| matches!(url.scheme(), "http" | "https"));
            match (element, attribute, external) {
                ("a", "href", Some(_)) => Some(Cow::Owned(format!(
                    "readloom-external:{}",
                    utf8_percent_encode(value, NON_ALPHANUMERIC)
                ))),
                (_, _, Some(_)) => None,
                _ => Some(Cow::Borrowed(value)),
            }
        })
        .link_rel(None)
        .add_generic_attributes(&["id", "lang", "dir", "title"])
        .add_clean_content_tags(&[
            "script", "style", "iframe", "frame", "object", "embed", "applet", "form", "template",
        ])
        .add_tags(&["link"])
        .add_tag_attributes("link", &["rel", "href", "type", "media"]);
    Ok(builder.clean(source).to_string())
}

pub(crate) fn extract_visible_text(source: &str) -> Result<String, AppError> {
    let mut builder = Builder::default();
    builder.add_clean_content_tags(&[
        "script", "style", "iframe", "frame", "object", "embed", "applet", "template",
    ]);
    let cleaned = builder.clean(source).to_string();
    let mut reader = Reader::from_str(&cleaned);
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Text(value)) => {
                let decoded = value
                    .xml_content(XmlVersion::Implicit1_0)
                    .map_err(|_| unsafe_xhtml())?;
                let unescaped =
                    quick_xml::escape::unescape(&decoded).map_err(|_| unsafe_xhtml())?;
                append_text(&mut text, &unescaped);
            }
            Ok(Event::CData(value)) => {
                let decoded = value.decode().map_err(|_| unsafe_xhtml())?;
                append_text(&mut text, &decoded);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(unsafe_xhtml()),
        }
    }
    Ok(text)
}

fn append_text(target: &mut String, value: &str) {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push(' ');
    }
    target.push_str(&value);
}

fn unsafe_xhtml() -> AppError {
    AppError::validation(
        "UNSAFE_XHTML",
        "EPUB 章节包含无法安全解析的内容。",
        "请返回目录并选择其他章节。",
    )
}

pub(crate) fn sanitize_css(
    source: &str,
    current_resource: &str,
    session_id: &str,
) -> Result<String, AppError> {
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    inspect_css_tokens(&mut parser, current_resource, session_id).map_err(|_| unsafe_css())?;
    Ok(source.to_owned())
}

fn inspect_css_tokens<'i, 't>(
    parser: &mut Parser<'i, 't>,
    current_resource: &str,
    session_id: &str,
) -> Result<(), ParseError<'i, ()>> {
    while let Ok(token) = parser.next_including_whitespace_and_comments().cloned() {
        match token {
            Token::AtKeyword(name) if name.eq_ignore_ascii_case("import") => {
                return Err(parser.new_custom_error(()));
            }
            Token::UnquotedUrl(value) => {
                if validate_css_url(current_resource, session_id, &value).is_err() {
                    return Err(parser.new_custom_error(()));
                }
            }
            Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                parser.parse_nested_block(|nested| {
                    let value = nested.expect_string_cloned()?;
                    nested.expect_exhausted()?;
                    if validate_css_url(current_resource, session_id, &value).is_err() {
                        return Err(nested.new_custom_error(()));
                    }
                    Ok(())
                })?;
            }
            Token::Function(name)
                if name.eq_ignore_ascii_case("expression")
                    || name.eq_ignore_ascii_case("-moz-binding") =>
            {
                return Err(parser.new_custom_error(()));
            }
            Token::Function(_)
            | Token::ParenthesisBlock
            | Token::SquareBracketBlock
            | Token::CurlyBracketBlock => {
                parser.parse_nested_block(|nested| {
                    inspect_css_tokens(nested, current_resource, session_id)
                })?;
            }
            Token::BadUrl(_) | Token::BadString(_) => {
                return Err(parser.new_custom_error(()));
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn sanitize_svg(
    source: &str,
    current_resource: &str,
    session_id: &str,
) -> Result<String, AppError> {
    if source.len() > 8 * 1024 * 1024 || source.to_ascii_lowercase().contains("<style") {
        return Err(unsafe_svg());
    }
    let mut reader = Reader::from_str(source);
    reader.config_mut().check_end_names = true;
    let mut depth = 0_usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                depth = depth.checked_add(1).ok_or_else(unsafe_svg)?;
                if depth > 64 {
                    return Err(unsafe_svg());
                }
                validate_svg_element(&reader, &element, current_resource, session_id)?;
            }
            Ok(Event::Empty(element)) => {
                validate_svg_element(&reader, &element, current_resource, session_id)?;
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::DocType(_) | Event::PI(_)) => return Err(unsafe_svg()),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(unsafe_svg()),
        }
    }
    Ok(source.to_owned())
}

fn validate_svg_element(
    reader: &Reader<&[u8]>,
    element: &quick_xml::events::BytesStart<'_>,
    current_resource: &str,
    session_id: &str,
) -> Result<(), AppError> {
    let binding = element.name();
    let local_name = binding
        .as_ref()
        .rsplit(|byte| *byte == b':')
        .next()
        .unwrap_or_default();
    let local_name = String::from_utf8_lossy(local_name).to_ascii_lowercase();
    if matches!(
        local_name.as_str(),
        "script" | "foreignobject" | "iframe" | "frame" | "object" | "embed" | "audio" | "video"
    ) {
        return Err(unsafe_svg());
    }
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute.map_err(|_| unsafe_svg())?;
        let key = String::from_utf8_lossy(attribute.key.as_ref()).to_ascii_lowercase();
        let local_key = key.rsplit(':').next().unwrap_or(&key);
        if local_key.starts_with("on") || local_key == "style" {
            return Err(unsafe_svg());
        }
        if matches!(local_key, "href" | "src") {
            let value = attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                .map_err(|_| unsafe_svg())?;
            if rewrite_internal_url(current_resource, session_id, &value).is_none() {
                return Err(unsafe_svg());
            }
        }
    }
    Ok(())
}

fn validate_css_url(current_resource: &str, session_id: &str, value: &str) -> Result<(), AppError> {
    if rewrite_internal_url(current_resource, session_id, value).is_none() {
        return Err(unsafe_css());
    }
    Ok(())
}

fn unsafe_css() -> AppError {
    AppError::validation(
        "EXTERNAL_RESOURCE_BLOCKED",
        "EPUB 样式表包含外部或不安全资源。",
        "该样式表已被阻止，正文仍可继续阅读。",
    )
}

fn unsafe_svg() -> AppError {
    AppError::validation(
        "UNSAFE_SVG",
        "EPUB SVG 包含脚本、外部引用或不安全结构。",
        "该图片已被阻止，正文仍可继续阅读。",
    )
}

struct InternalUrlResolver {
    current_resource: String,
    session_id: String,
}

impl<'a> UrlRelativeEvaluate<'a> for InternalUrlResolver {
    fn evaluate<'url>(&self, url: &'url str) -> Option<Cow<'url, str>> {
        rewrite_internal_url(&self.current_resource, &self.session_id, url).map(Cow::Owned)
    }
}

fn rewrite_internal_url(current_resource: &str, session_id: &str, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.starts_with("//") || value.contains(['\\', '\0']) {
        return None;
    }
    let scheme_end = value.find(':');
    let path_end = value.find(['/', '#', '?']).unwrap_or(value.len());
    if scheme_end.is_some_and(|index| index < path_end) {
        return None;
    }

    let (without_fragment, fragment) = value
        .split_once('#')
        .map_or((value, None), |(path, fragment)| (path, Some(fragment)));
    let path = without_fragment
        .split_once('?')
        .map_or(without_fragment, |(path, _)| path);
    let decoded = percent_decode_str(path).decode_utf8().ok()?;
    if percent_decode_str(decoded.as_ref())
        .decode_utf8()
        .ok()?
        .as_ref()
        != decoded.as_ref()
    {
        return None;
    }

    let candidate = if decoded.is_empty() {
        current_resource.to_owned()
    } else if let Some(rooted) = decoded.strip_prefix('/') {
        rooted.to_owned()
    } else {
        let parent = current_resource
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent);
        if parent.is_empty() {
            decoded.into_owned()
        } else {
            format!("{parent}/{decoded}")
        }
    };
    let safe_path = SafeArchivePath::parse(&candidate).ok()?;
    let encoded_path = safe_path
        .as_str()
        .split('/')
        .map(|segment| utf8_percent_encode(segment, URL_COMPONENT).to_string())
        .collect::<Vec<_>>()
        .join("/");
    let mut rewritten = format!("http://readloom-epub.localhost/{session_id}/{encoded_path}");
    if let Some(fragment) = fragment.filter(|value| !value.is_empty()) {
        rewritten.push('#');
        rewritten.push_str(&utf8_percent_encode(fragment, URL_COMPONENT).to_string());
    }
    Some(rewritten)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_scripts_and_rewrites_only_internal_resources() {
        let source = r#"
          <main>
            <script>window.evil = true</script>
            <img src="../images/cover.png" onerror="alert('bad')" />
            <img src="https://tracker.invalid/pixel.png" />
            <a href="https://example.com/read?q=1">external</a>
            <a href="javascript:alert('bad')">bad</a>
          </main>
        "#;

        let cleaned =
            sanitize_xhtml_fragment(source, "EPUB/text/chapter.xhtml", "0123456789abcdef")
                .expect("sanitize XHTML");

        assert!(!cleaned.contains("window.evil"));
        assert!(!cleaned.contains("onerror"));
        assert!(!cleaned.contains("javascript:"));
        assert!(!cleaned.contains("tracker.invalid"));
        assert!(cleaned.contains("readloom-external:https%3A%2F%2Fexample%2Ecom%2Fread%3Fq%3D1"));
        assert!(!cleaned.contains("href=\"https://"));
        assert!(
            cleaned
                .contains("http://readloom-epub.localhost/0123456789abcdef/EPUB/images/cover.png")
        );
    }

    #[test]
    fn css_allows_internal_urls_but_blocks_imports_and_network_urls() {
        assert!(sanitize_css(
            "@font-face{src:url('../fonts/book.woff2')} body{background:url(../images/paper.png)}",
            "EPUB/styles/book.css",
            "0123456789abcdef",
        )
        .is_ok());

        for source in [
            "@import 'https://tracker.invalid/theme.css';",
            "body{background:url(https://tracker.invalid/pixel.png)}",
            "body{background:url(data:image/png;base64,AAAA)}",
        ] {
            let error = sanitize_css(source, "EPUB/styles/book.css", "0123456789abcdef")
                .expect_err("external CSS resource must be blocked");
            assert_eq!(error.to_dto().code, "EXTERNAL_RESOURCE_BLOCKED");
        }
    }

    #[test]
    fn svg_rejects_scripts_events_and_external_references() {
        for source in [
            r#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><image onload="alert(1)"/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="https://tracker.invalid/x.png"/></svg>"#,
        ] {
            let error = sanitize_svg(source, "EPUB/images/cover.svg", "0123456789abcdef")
                .expect_err("unsafe SVG must be rejected");
            assert_eq!(error.to_dto().code, "UNSAFE_SVG");
        }

        assert!(
            sanitize_svg(
                r#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>"#,
                "EPUB/images/cover.svg",
                "0123456789abcdef",
            )
            .is_ok()
        );
    }

    #[test]
    fn visible_text_extraction_excludes_scripts_and_styles() {
        let text = extract_visible_text(
            "<html><style>secret-style</style><body><h1>第一章</h1><script>secret-script</script><p>可见 正文</p></body></html>",
        )
        .expect("extract visible text");

        assert_eq!(text, "第一章 可见 正文");
        assert!(!text.contains("secret"));
    }
}
