use std::collections::{BTreeMap, HashSet};

use percent_encoding::percent_decode_str;
use quick_xml::{
    Reader, Writer, XmlVersion,
    events::{BytesEnd, BytesStart, BytesText, Event},
};
use serde_json::{Map, Value, json};

use crate::{
    config::EpubChapterEditLimits,
    domain::epub_edit::{ChapterCompatibilityLevel, ChapterEditWarning, ChapterValidationState},
    error::AppError,
    infrastructure::archive::safe_zip::SafeArchivePath,
};

#[derive(Debug, Clone)]
pub(crate) struct PreservedOuterDocument {
    prefix_through_body_start: Vec<u8>,
    suffix_from_body_end: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct AnalyzedChapter {
    pub original_xhtml: Vec<u8>,
    pub preserved_outer_document: Option<PreservedOuterDocument>,
    pub editor_document: Value,
    pub compatibility_level: ChapterCompatibilityLevel,
    pub warnings: Vec<ChapterEditWarning>,
    pub original_resource_hash: String,
    pub validation_state: ChapterValidationState,
}

#[derive(Debug, Clone)]
enum XmlChild {
    Element(XmlElement),
    Text(String),
}

#[derive(Debug, Clone)]
struct XmlElement {
    name: String,
    attributes: BTreeMap<String, String>,
    children: Vec<XmlChild>,
}

struct ParsedBody {
    children: Vec<XmlChild>,
    outer: PreservedOuterDocument,
    nodes: usize,
    text_characters: usize,
    images: usize,
}

pub(crate) fn analyze_chapter(
    source: &[u8],
    chapter_resource: &str,
    reading_session_id: &str,
    manifest_resources: &HashSet<String>,
    fixed_layout: bool,
    limits: EpubChapterEditLimits,
) -> AnalyzedChapter {
    let original_resource_hash = blake3::hash(source).to_hex().to_string();
    if source.len() > limits.maximum_xhtml_bytes {
        return unavailable_analysis(
            source,
            original_resource_hash,
            ChapterCompatibilityLevel::ReadOnly,
            "CHAPTER_TOO_LARGE",
            "章节 XHTML 超过 2 MiB 的可视化编辑上限，仍可安全阅读。",
        );
    }
    let parsed = match parse_body(source, limits) {
        Ok(parsed) => parsed,
        Err(_) => {
            return unavailable_analysis(
                source,
                original_resource_hash,
                ChapterCompatibilityLevel::Unsupported,
                "CHAPTER_PARSE_FAILED",
                "章节不是可安全往返的 UTF-8 XHTML，已禁用可视化编辑。",
            );
        }
    };
    if parsed.nodes > limits.maximum_nodes
        || parsed.text_characters > limits.maximum_text_characters
        || parsed.images > limits.maximum_images
    {
        return AnalyzedChapter {
            original_xhtml: source.to_vec(),
            preserved_outer_document: Some(parsed.outer),
            editor_document: empty_document(),
            compatibility_level: ChapterCompatibilityLevel::ReadOnly,
            warnings: vec![warning(
                "CHAPTER_TOO_COMPLEX",
                "章节节点、文本或图片数量超过集中安全限制，仍可安全阅读。",
            )],
            original_resource_hash,
            validation_state: ChapterValidationState::Warning,
        };
    }

    let mut compatibility = if fixed_layout {
        ChapterCompatibilityLevel::ReadOnly
    } else {
        ChapterCompatibilityLevel::Full
    };
    let mut warnings = Vec::new();
    if fixed_layout {
        warnings.push(warning(
            "FIXED_LAYOUT_CHAPTER",
            "固定布局章节无法保证可视化编辑往返，已保持只读。",
        ));
    }
    inspect_children(&parsed.children, &mut compatibility, &mut warnings);
    deduplicate_warnings(&mut warnings);

    let editor_document = if matches!(
        compatibility,
        ChapterCompatibilityLevel::Full | ChapterCompatibilityLevel::Limited
    ) {
        match body_to_editor_document(
            &parsed.children,
            chapter_resource,
            reading_session_id,
            manifest_resources,
        ) {
            Ok(document) => document,
            Err(message) => {
                compatibility = ChapterCompatibilityLevel::ReadOnly;
                warnings.push(warning("UNSUPPORTED_CHAPTER_ELEMENT", message));
                empty_document()
            }
        }
    } else {
        empty_document()
    };
    let validation_state = if warnings.is_empty() {
        ChapterValidationState::Valid
    } else {
        ChapterValidationState::Warning
    };
    AnalyzedChapter {
        original_xhtml: source.to_vec(),
        preserved_outer_document: Some(parsed.outer),
        editor_document,
        compatibility_level: compatibility,
        warnings,
        original_resource_hash,
        validation_state,
    }
}

pub(crate) fn serialize_editor_document(
    outer: &PreservedOuterDocument,
    editor_document: &Value,
    chapter_resource: &str,
    reading_session_id: &str,
    manifest_resources: &HashSet<String>,
    limits: EpubChapterEditLimits,
) -> Result<Vec<u8>, AppError> {
    let serialized_json = serde_json::to_vec(editor_document).map_err(|_| invalid_draft())?;
    if serialized_json.len() > limits.maximum_sync_bytes {
        return Err(chapter_too_large());
    }
    let mut writer = Writer::new(Vec::with_capacity(serialized_json.len()));
    let mut budget = SerializationBudget::default();
    let root = editor_document.as_object().ok_or_else(invalid_draft)?;
    if string_field(root, "type")? != "doc" {
        return Err(invalid_draft());
    }
    for node in content_field(root)? {
        write_node(
            &mut writer,
            node,
            chapter_resource,
            reading_session_id,
            manifest_resources,
            limits,
            &mut budget,
            1,
        )?;
    }
    let body = writer.into_inner();
    let total_len = outer
        .prefix_through_body_start
        .len()
        .saturating_add(body.len())
        .saturating_add(outer.suffix_from_body_end.len());
    if total_len > limits.maximum_xhtml_bytes {
        return Err(chapter_too_large());
    }
    let mut output = Vec::with_capacity(total_len);
    output.extend_from_slice(&outer.prefix_through_body_start);
    output.extend_from_slice(&body);
    output.extend_from_slice(&outer.suffix_from_body_end);
    validate_normalized_xhtml(&output, limits)?;
    Ok(output)
}

fn parse_body(source: &[u8], limits: EpubChapterEditLimits) -> Result<ParsedBody, AppError> {
    let mut reader = Reader::from_reader(source);
    reader.config_mut().trim_text(false);
    reader.config_mut().check_end_names = true;
    let mut depth = 0_usize;
    let mut body_depth = None;
    let mut body_inner_start = None;
    let mut body_inner_end = None;
    let mut roots = Vec::new();
    let mut stack = Vec::<XmlElement>::new();
    let mut nodes = 0_usize;
    let mut text_characters = 0_usize;
    let mut images = 0_usize;
    loop {
        let before = usize::try_from(reader.buffer_position()).map_err(|_| parse_failed())?;
        let event = reader.read_event().map_err(|_| parse_failed())?;
        let after = usize::try_from(reader.buffer_position()).map_err(|_| parse_failed())?;
        match event {
            Event::Eof => break,
            Event::DocType(value) => {
                let doctype = value.decode().map_err(|_| parse_failed())?;
                if !doctype.trim().eq_ignore_ascii_case("html") {
                    return Err(parse_failed());
                }
            }
            Event::PI(_) => return Err(parse_failed()),
            Event::Start(start) => {
                depth = depth.checked_add(1).ok_or_else(parse_failed)?;
                if depth > limits.maximum_depth {
                    return Err(chapter_too_complex());
                }
                let name = decoded_name(start.name().as_ref())?;
                if body_depth.is_none() && local_name(&name) == "body" {
                    body_depth = Some(depth);
                    body_inner_start = Some(after);
                    continue;
                }
                if body_depth.is_some_and(|body| depth > body) {
                    nodes = nodes.checked_add(1).ok_or_else(chapter_too_complex)?;
                    if local_name(&name) == "img" {
                        images = images.checked_add(1).ok_or_else(chapter_too_complex)?;
                    }
                    stack.push(XmlElement {
                        name,
                        attributes: attributes(&reader, &start)?,
                        children: Vec::new(),
                    });
                }
            }
            Event::Empty(start) => {
                let name = decoded_name(start.name().as_ref())?;
                if body_depth.is_none() && local_name(&name) == "body" {
                    return Err(parse_failed());
                }
                if body_depth.is_some_and(|body| depth >= body) {
                    nodes = nodes.checked_add(1).ok_or_else(chapter_too_complex)?;
                    if local_name(&name) == "img" {
                        images = images.checked_add(1).ok_or_else(chapter_too_complex)?;
                    }
                    push_child(
                        &mut roots,
                        &mut stack,
                        XmlChild::Element(XmlElement {
                            name,
                            attributes: attributes(&reader, &start)?,
                            children: Vec::new(),
                        }),
                    );
                }
            }
            Event::Text(value) => {
                if body_depth.is_some_and(|body| depth >= body) {
                    let decoded = value
                        .xml_content(XmlVersion::Implicit1_0)
                        .map_err(|_| parse_failed())?;
                    let text = quick_xml::escape::unescape_with(&decoded, html_entity)
                        .map_err(|_| parse_failed())?
                        .into_owned();
                    text_characters = text_characters
                        .checked_add(text.chars().count())
                        .ok_or_else(chapter_too_complex)?;
                    push_child(&mut roots, &mut stack, XmlChild::Text(text));
                }
            }
            Event::CData(value) => {
                if body_depth.is_some_and(|body| depth >= body) {
                    let text = value.decode().map_err(|_| parse_failed())?.into_owned();
                    text_characters = text_characters
                        .checked_add(text.chars().count())
                        .ok_or_else(chapter_too_complex)?;
                    push_child(&mut roots, &mut stack, XmlChild::Text(text));
                }
            }
            Event::GeneralRef(value) => {
                if body_depth.is_some_and(|body| depth >= body) {
                    let name = value.decode().map_err(|_| parse_failed())?;
                    let encoded = format!("&{name};");
                    let text = quick_xml::escape::unescape_with(&encoded, html_entity)
                        .map_err(|_| parse_failed())?
                        .into_owned();
                    text_characters = text_characters
                        .checked_add(text.chars().count())
                        .ok_or_else(chapter_too_complex)?;
                    push_child(&mut roots, &mut stack, XmlChild::Text(text));
                }
            }
            Event::End(end) => {
                let name = decoded_name(end.name().as_ref())?;
                if body_depth == Some(depth) && local_name(&name) == "body" {
                    if !stack.is_empty() {
                        return Err(parse_failed());
                    }
                    body_inner_end = Some(before);
                } else if body_depth.is_some_and(|body| depth > body) {
                    let element = stack.pop().ok_or_else(parse_failed)?;
                    if element.name != name {
                        return Err(parse_failed());
                    }
                    push_child(&mut roots, &mut stack, XmlChild::Element(element));
                }
                depth = depth.checked_sub(1).ok_or_else(parse_failed)?;
            }
            Event::Decl(_) | Event::Comment(_) => {}
        }
    }
    if depth != 0 || body_depth.is_none() || !stack.is_empty() {
        return Err(parse_failed());
    }
    let start = body_inner_start.ok_or_else(parse_failed)?;
    let end = body_inner_end.ok_or_else(parse_failed)?;
    if start > end || end > source.len() {
        return Err(parse_failed());
    }
    Ok(ParsedBody {
        children: roots,
        outer: PreservedOuterDocument {
            prefix_through_body_start: source[..start].to_vec(),
            suffix_from_body_end: source[end..].to_vec(),
        },
        nodes,
        text_characters,
        images,
    })
}

fn push_child(roots: &mut Vec<XmlChild>, stack: &mut [XmlElement], child: XmlChild) {
    if let Some(parent) = stack.last_mut() {
        push_or_merge_text(&mut parent.children, child);
    } else {
        push_or_merge_text(roots, child);
    }
}

fn push_or_merge_text(children: &mut Vec<XmlChild>, child: XmlChild) {
    if let XmlChild::Text(text) = &child
        && let Some(XmlChild::Text(previous)) = children.last_mut()
    {
        previous.push_str(text);
        return;
    }
    children.push(child);
}

fn attributes(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
) -> Result<BTreeMap<String, String>, AppError> {
    let mut output = BTreeMap::new();
    for attribute in start.attributes().with_checks(true) {
        let attribute = attribute.map_err(|_| parse_failed())?;
        let name = decoded_name(attribute.key.as_ref())?;
        let value = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|_| parse_failed())?
            .into_owned();
        if output.insert(name, value).is_some() {
            return Err(parse_failed());
        }
    }
    Ok(output)
}

fn decoded_name(bytes: &[u8]) -> Result<String, AppError> {
    std::str::from_utf8(bytes)
        .map(|value| value.to_ascii_lowercase())
        .map_err(|_| parse_failed())
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn html_entity(name: &str) -> Option<&'static str> {
    match name {
        "amp" => Some("&"),
        "lt" => Some("<"),
        "gt" => Some(">"),
        "quot" => Some("\""),
        "apos" => Some("'"),
        "nbsp" => Some("\u{00a0}"),
        "copy" => Some("©"),
        "reg" => Some("®"),
        "trade" => Some("™"),
        "hellip" => Some("…"),
        "mdash" => Some("—"),
        "ndash" => Some("–"),
        "laquo" => Some("«"),
        "raquo" => Some("»"),
        _ => None,
    }
}

fn inspect_children(
    children: &[XmlChild],
    compatibility: &mut ChapterCompatibilityLevel,
    warnings: &mut Vec<ChapterEditWarning>,
) {
    for child in children {
        let XmlChild::Element(element) = child else {
            continue;
        };
        let name = local_name(&element.name);
        if matches!(
            name,
            "script" | "iframe" | "object" | "embed" | "form" | "input" | "button" | "canvas"
        ) {
            *compatibility = ChapterCompatibilityLevel::ReadOnly;
            warnings.push(warning(
                "UNSAFE_CHAPTER_ELEMENT",
                "章节包含脚本、表单或嵌入式活动内容，已保持只读。",
            ));
        } else if matches!(name, "math" | "svg" | "table" | "ruby" | "audio" | "video") {
            *compatibility = ChapterCompatibilityLevel::ReadOnly;
            warnings.push(warning(
                "UNSUPPORTED_CHAPTER_ELEMENT",
                "章节包含 MathML、SVG、表格、ruby 或媒体结构，无法保证无损往返。",
            ));
        } else if !matches!(
            name,
            "p" | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "br"
                | "strong"
                | "b"
                | "em"
                | "i"
                | "s"
                | "strike"
                | "del"
                | "u"
                | "sub"
                | "sup"
                | "blockquote"
                | "ul"
                | "ol"
                | "li"
                | "hr"
                | "a"
                | "img"
                | "span"
        ) || element.name.contains(':')
        {
            *compatibility = ChapterCompatibilityLevel::ReadOnly;
            warnings.push(warning(
                "UNSUPPORTED_CHAPTER_ELEMENT",
                "章节包含编辑器无法可靠保留的自定义或容器元素，已保持只读。",
            ));
        }
        if element
            .attributes
            .keys()
            .any(|key| local_name(key).starts_with("on"))
        {
            *compatibility = ChapterCompatibilityLevel::ReadOnly;
            warnings.push(warning(
                "UNSAFE_CHAPTER_ATTRIBUTE",
                "章节包含事件处理属性，已保持只读且不会进入编辑器。",
            ));
        }
        let allowed = allowed_attributes(name);
        if element
            .attributes
            .keys()
            .any(|key| !allowed.contains(&key.as_str()))
        {
            *compatibility = ChapterCompatibilityLevel::ReadOnly;
            warnings.push(warning(
                "UNSUPPORTED_CHAPTER_ATTRIBUTE",
                "章节包含无法可靠保留的元素属性，已保持只读。",
            ));
        }
        if let Some(style) = element.attributes.get("style")
            && parse_text_align(style).is_none()
        {
            if *compatibility == ChapterCompatibilityLevel::Full {
                *compatibility = ChapterCompatibilityLevel::Limited;
            }
            warnings.push(warning(
                "LIMITED_INLINE_STYLE",
                "章节的非对齐内联样式不会进入编辑模型；原始正文保持可恢复。",
            ));
        }
        inspect_children(&element.children, compatibility, warnings);
    }
}

fn allowed_attributes(name: &str) -> &'static [&'static str] {
    match name {
        "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" => &[
            "id",
            "class",
            "lang",
            "xml:lang",
            "dir",
            "epub:type",
            "title",
            "style",
        ],
        "ol" => &["start"],
        "a" => &["href", "title"],
        "img" => &["src", "alt", "title", "width", "height", "id", "class"],
        "span" => &[
            "id",
            "class",
            "role",
            "lang",
            "xml:lang",
            "dir",
            "epub:type",
            "title",
        ],
        _ => &[],
    }
}

fn body_to_editor_document(
    children: &[XmlChild],
    chapter_resource: &str,
    reading_session_id: &str,
    manifest_resources: &HashSet<String>,
) -> Result<Value, String> {
    let mut content = Vec::new();
    for child in children {
        match child {
            XmlChild::Text(text) if text.trim().is_empty() => {}
            XmlChild::Text(text) => content.push(json!({
                "type": "paragraph",
                "content": [{"type": "text", "text": text}],
            })),
            XmlChild::Element(element) => {
                content.push(block_to_editor_node(
                    element,
                    chapter_resource,
                    reading_session_id,
                    manifest_resources,
                )?);
            }
        }
    }
    if content.is_empty() {
        content.push(json!({"type": "paragraph"}));
    }
    Ok(json!({"type": "doc", "content": content}))
}

fn block_to_editor_node(
    element: &XmlElement,
    chapter_resource: &str,
    reading_session_id: &str,
    manifest_resources: &HashSet<String>,
) -> Result<Value, String> {
    let name = local_name(&element.name);
    match name {
        "p" => Ok(editor_container(
            "paragraph",
            common_attrs(element),
            inline_children(
                &element.children,
                &[],
                chapter_resource,
                reading_session_id,
                manifest_resources,
            )?,
        )),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let mut attrs = common_attrs(element);
            attrs.insert(
                "level".to_owned(),
                Value::from(name[1..].parse::<u64>().map_err(|_| "标题级别无效。")?),
            );
            Ok(editor_container(
                "heading",
                attrs,
                inline_children(
                    &element.children,
                    &[],
                    chapter_resource,
                    reading_session_id,
                    manifest_resources,
                )?,
            ))
        }
        "blockquote" => Ok(editor_container(
            "blockquote",
            common_attrs(element),
            block_children(
                &element.children,
                chapter_resource,
                reading_session_id,
                manifest_resources,
            )?,
        )),
        "ul" | "ol" => {
            let mut attrs = Map::new();
            if name == "ol"
                && let Some(start) = element.attributes.get("start")
            {
                attrs.insert(
                    "start".to_owned(),
                    Value::from(start.parse::<u64>().map_err(|_| "有序列表起始值无效。")?),
                );
            }
            Ok(editor_container(
                if name == "ul" {
                    "bulletList"
                } else {
                    "orderedList"
                },
                attrs,
                block_children(
                    &element.children,
                    chapter_resource,
                    reading_session_id,
                    manifest_resources,
                )?,
            ))
        }
        "li" => {
            let mut content = block_children(
                &element.children,
                chapter_resource,
                reading_session_id,
                manifest_resources,
            )?;
            if content.is_empty() {
                content.push(json!({"type":"paragraph"}));
            }
            Ok(editor_container("listItem", Map::new(), content))
        }
        "hr" => Ok(json!({"type": "horizontalRule"})),
        "img" => Ok(editor_container(
            "paragraph",
            Map::new(),
            vec![image_node(
                element,
                chapter_resource,
                reading_session_id,
                manifest_resources,
            )?],
        )),
        _ => Err(format!("元素 <{name}> 不在安全编辑 schema 中。")),
    }
}

fn block_children(
    children: &[XmlChild],
    chapter_resource: &str,
    reading_session_id: &str,
    manifest_resources: &HashSet<String>,
) -> Result<Vec<Value>, String> {
    let mut output = Vec::new();
    let mut inline = Vec::new();
    for child in children {
        match child {
            XmlChild::Text(text) if text.trim().is_empty() => {}
            XmlChild::Text(_) => inline.push(child.clone()),
            XmlChild::Element(element)
                if matches!(
                    local_name(&element.name),
                    "strong"
                        | "b"
                        | "em"
                        | "i"
                        | "s"
                        | "strike"
                        | "del"
                        | "u"
                        | "sub"
                        | "sup"
                        | "a"
                        | "br"
                        | "img"
                        | "span"
                ) =>
            {
                inline.push(child.clone())
            }
            XmlChild::Element(element) => {
                if !inline.is_empty() {
                    output.push(editor_container(
                        "paragraph",
                        Map::new(),
                        inline_children(
                            &inline,
                            &[],
                            chapter_resource,
                            reading_session_id,
                            manifest_resources,
                        )?,
                    ));
                    inline.clear();
                }
                output.push(block_to_editor_node(
                    element,
                    chapter_resource,
                    reading_session_id,
                    manifest_resources,
                )?);
            }
        }
    }
    if !inline.is_empty() {
        output.push(editor_container(
            "paragraph",
            Map::new(),
            inline_children(
                &inline,
                &[],
                chapter_resource,
                reading_session_id,
                manifest_resources,
            )?,
        ));
    }
    Ok(output)
}

fn inline_children(
    children: &[XmlChild],
    marks: &[Value],
    chapter_resource: &str,
    reading_session_id: &str,
    manifest_resources: &HashSet<String>,
) -> Result<Vec<Value>, String> {
    let mut output = Vec::new();
    for child in children {
        match child {
            XmlChild::Text(text) => {
                if !text.is_empty() {
                    let mut node = Map::new();
                    node.insert("type".to_owned(), Value::String("text".to_owned()));
                    node.insert("text".to_owned(), Value::String(text.clone()));
                    if !marks.is_empty() {
                        node.insert("marks".to_owned(), Value::Array(marks.to_vec()));
                    }
                    output.push(Value::Object(node));
                }
            }
            XmlChild::Element(element) => {
                let name = local_name(&element.name);
                if name == "br" {
                    output.push(json!({"type":"hardBreak"}));
                    continue;
                }
                if name == "img" {
                    let mut image = image_node(
                        element,
                        chapter_resource,
                        reading_session_id,
                        manifest_resources,
                    )?;
                    if !marks.is_empty() {
                        image
                            .as_object_mut()
                            .expect("image_node always returns an object")
                            .insert("marks".to_owned(), Value::Array(marks.to_vec()));
                    }
                    output.push(image);
                    continue;
                }
                let mut nested_marks = marks.to_vec();
                let mark = match name {
                    "strong" | "b" => json!({"type":"bold"}),
                    "em" | "i" => json!({"type":"italic"}),
                    "s" | "strike" | "del" => json!({"type":"strike"}),
                    "u" => json!({"type":"underline"}),
                    "sub" => json!({"type":"subscript"}),
                    "sup" => json!({"type":"superscript"}),
                    "a" => {
                        let href = element
                            .attributes
                            .get("href")
                            .ok_or_else(|| "链接缺少 href。".to_owned())?;
                        let href = normalize_href(
                            href,
                            chapter_resource,
                            reading_session_id,
                            manifest_resources,
                            false,
                        )?;
                        let mut attrs = Map::new();
                        attrs.insert("href".to_owned(), Value::String(href));
                        if let Some(title) = element.attributes.get("title") {
                            attrs.insert("title".to_owned(), Value::String(title.clone()));
                        }
                        json!({"type":"link", "attrs":attrs})
                    }
                    "span" => json!({
                        "type": "publisherSpan",
                        "attrs": publisher_span_attrs(element),
                    }),
                    _ => return Err(format!("内联元素 <{name}> 不受支持。")),
                };
                nested_marks.push(mark);
                output.extend(inline_children(
                    &element.children,
                    &nested_marks,
                    chapter_resource,
                    reading_session_id,
                    manifest_resources,
                )?);
            }
        }
    }
    Ok(output)
}

fn image_node(
    element: &XmlElement,
    chapter_resource: &str,
    reading_session_id: &str,
    manifest_resources: &HashSet<String>,
) -> Result<Value, String> {
    let src = element
        .attributes
        .get("src")
        .ok_or_else(|| "图片缺少 src。".to_owned())?;
    let target = resolve_internal_target(src, chapter_resource)?;
    if !manifest_resources.contains(&target) {
        return Err("图片不在 EPUB manifest 中。".to_owned());
    }
    let encoded = target
        .split('/')
        .map(|segment| {
            percent_encoding::utf8_percent_encode(segment, percent_encoding::NON_ALPHANUMERIC)
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("/");
    let mut attrs = Map::new();
    attrs.insert(
        "src".to_owned(),
        Value::String(format!(
            "http://readloom-epub.localhost/{reading_session_id}/{encoded}"
        )),
    );
    attrs.insert(
        "alt".to_owned(),
        Value::String(element.attributes.get("alt").cloned().unwrap_or_default()),
    );
    for key in ["title", "width", "height", "id", "class"] {
        if let Some(value) = element.attributes.get(key) {
            attrs.insert(key.to_owned(), Value::String(value.clone()));
        }
    }
    Ok(json!({"type":"image", "attrs":attrs}))
}

fn publisher_span_attrs(element: &XmlElement) -> Map<String, Value> {
    let mut attrs = Map::new();
    for (source, target) in [
        ("id", "id"),
        ("class", "class"),
        ("role", "role"),
        ("lang", "lang"),
        ("xml:lang", "xmlLang"),
        ("dir", "dir"),
        ("epub:type", "epubType"),
        ("title", "title"),
    ] {
        if let Some(value) = element.attributes.get(source) {
            attrs.insert(target.to_owned(), Value::String(value.clone()));
        }
    }
    attrs
}

fn common_attrs(element: &XmlElement) -> Map<String, Value> {
    let mut attrs = Map::new();
    for (source, target) in [
        ("id", "id"),
        ("class", "class"),
        ("lang", "lang"),
        ("xml:lang", "xmlLang"),
        ("dir", "dir"),
        ("epub:type", "epubType"),
        ("title", "title"),
    ] {
        if let Some(value) = element.attributes.get(source) {
            attrs.insert(target.to_owned(), Value::String(value.clone()));
        }
    }
    if let Some(align) = element
        .attributes
        .get("style")
        .and_then(|value| parse_text_align(value))
    {
        attrs.insert("textAlign".to_owned(), Value::String(align.to_owned()));
    }
    attrs
}

fn parse_text_align(style: &str) -> Option<&'static str> {
    let mut found = None;
    for declaration in style.split(';').filter(|value| !value.trim().is_empty()) {
        let (name, value) = declaration.split_once(':')?;
        if !name.trim().eq_ignore_ascii_case("text-align") || found.is_some() {
            return None;
        }
        found = match value.trim().to_ascii_lowercase().as_str() {
            "left" => Some("left"),
            "center" => Some("center"),
            "right" => Some("right"),
            "justify" => Some("justify"),
            _ => return None,
        };
    }
    found
}

fn editor_container(kind: &str, attrs: Map<String, Value>, content: Vec<Value>) -> Value {
    let mut node = Map::new();
    node.insert("type".to_owned(), Value::String(kind.to_owned()));
    if !attrs.is_empty() {
        node.insert("attrs".to_owned(), Value::Object(attrs));
    }
    if !content.is_empty() {
        node.insert("content".to_owned(), Value::Array(content));
    }
    Value::Object(node)
}

#[derive(Default)]
struct SerializationBudget {
    nodes: usize,
    text_characters: usize,
    images: usize,
}

#[allow(clippy::too_many_arguments)]
fn write_node(
    writer: &mut Writer<Vec<u8>>,
    node: &Value,
    chapter_resource: &str,
    reading_session_id: &str,
    manifest_resources: &HashSet<String>,
    limits: EpubChapterEditLimits,
    budget: &mut SerializationBudget,
    depth: usize,
) -> Result<(), AppError> {
    if depth > limits.maximum_depth {
        return Err(chapter_too_complex());
    }
    budget.nodes = budget
        .nodes
        .checked_add(1)
        .ok_or_else(chapter_too_complex)?;
    if budget.nodes > limits.maximum_nodes {
        return Err(chapter_too_complex());
    }
    let object = node.as_object().ok_or_else(invalid_draft)?;
    let kind = string_field(object, "type")?;
    match kind {
        "text" => write_text_node(
            writer,
            object,
            chapter_resource,
            reading_session_id,
            manifest_resources,
            budget,
            limits,
        ),
        "hardBreak" => write_empty(writer, "br", &[]),
        "horizontalRule" => write_empty(writer, "hr", &[]),
        "image" => {
            budget.images = budget
                .images
                .checked_add(1)
                .ok_or_else(chapter_too_complex)?;
            if budget.images > limits.maximum_images {
                return Err(chapter_too_complex());
            }
            let attrs = object
                .get("attrs")
                .and_then(Value::as_object)
                .ok_or_else(invalid_draft)?;
            let src = string_field(attrs, "src")?;
            let normalized = normalize_href(
                src,
                chapter_resource,
                reading_session_id,
                manifest_resources,
                true,
            )
            .map_err(|_| unsafe_resource())?;
            let mut xml_attrs = vec![("src", normalized)];
            let alt = attrs.get("alt").and_then(Value::as_str).unwrap_or("");
            xml_attrs.push(("alt", alt.to_owned()));
            for key in ["title", "width", "height", "id", "class"] {
                if let Some(value) = attrs.get(key).and_then(Value::as_str) {
                    if matches!(key, "width" | "height") {
                        if value.len() > 512 || !value.chars().all(|c| c.is_ascii_digit()) {
                            return Err(invalid_draft());
                        }
                    } else {
                        validate_plain_attribute(value)?;
                    }
                    if value.is_empty() && matches!(key, "id" | "class") {
                        return Err(invalid_draft());
                    }
                    xml_attrs.push((key, value.to_owned()));
                }
            }
            let opened = write_inline_mark_starts(
                writer,
                inline_marks(object)?,
                chapter_resource,
                reading_session_id,
                manifest_resources,
            )?;
            write_empty_owned(writer, "img", &xml_attrs)?;
            write_inline_mark_ends(writer, opened)
        }
        "paragraph" | "heading" | "blockquote" | "bulletList" | "orderedList" | "listItem" => {
            let tag = match kind {
                "paragraph" => "p".to_owned(),
                "heading" => {
                    let level = object
                        .get("attrs")
                        .and_then(Value::as_object)
                        .and_then(|attrs| attrs.get("level"))
                        .and_then(Value::as_u64)
                        .filter(|value| (1..=6).contains(value))
                        .ok_or_else(invalid_draft)?;
                    format!("h{level}")
                }
                "blockquote" => "blockquote".to_owned(),
                "bulletList" => "ul".to_owned(),
                "orderedList" => "ol".to_owned(),
                _ => "li".to_owned(),
            };
            let xml_attrs = node_attributes(kind, object)?;
            write_start_owned(writer, &tag, &xml_attrs)?;
            for child in content_field(object)? {
                write_node(
                    writer,
                    child,
                    chapter_resource,
                    reading_session_id,
                    manifest_resources,
                    limits,
                    budget,
                    depth + 1,
                )?;
            }
            writer
                .write_event(Event::End(BytesEnd::new(&tag)))
                .map_err(|_| serialization_failed())?;
            Ok(())
        }
        _ => Err(invalid_draft()),
    }
}

fn write_text_node(
    writer: &mut Writer<Vec<u8>>,
    object: &Map<String, Value>,
    chapter_resource: &str,
    reading_session_id: &str,
    manifest_resources: &HashSet<String>,
    budget: &mut SerializationBudget,
    limits: EpubChapterEditLimits,
) -> Result<(), AppError> {
    let text = string_field(object, "text")?;
    budget.text_characters = budget
        .text_characters
        .checked_add(text.chars().count())
        .ok_or_else(chapter_too_complex)?;
    if budget.text_characters > limits.maximum_text_characters {
        return Err(chapter_too_large());
    }
    let opened = write_inline_mark_starts(
        writer,
        inline_marks(object)?,
        chapter_resource,
        reading_session_id,
        manifest_resources,
    )?;
    writer
        .write_event(Event::Text(BytesText::new(text)))
        .map_err(|_| serialization_failed())?;
    write_inline_mark_ends(writer, opened)
}

fn inline_marks(object: &Map<String, Value>) -> Result<&[Value], AppError> {
    object
        .get("marks")
        .map(|value| {
            value
                .as_array()
                .map(Vec::as_slice)
                .ok_or_else(invalid_draft)
        })
        .transpose()
        .map(|marks| marks.unwrap_or(&[]))
}

fn write_inline_mark_starts(
    writer: &mut Writer<Vec<u8>>,
    marks: &[Value],
    chapter_resource: &str,
    reading_session_id: &str,
    manifest_resources: &HashSet<String>,
) -> Result<Vec<&'static str>, AppError> {
    let mut opened = Vec::new();
    for mark in marks {
        let mark = mark.as_object().ok_or_else(invalid_draft)?;
        let kind = string_field(mark, "type")?;
        let tag = match kind {
            "bold" => "strong",
            "italic" => "em",
            "strike" => "s",
            "underline" => "u",
            "subscript" => "sub",
            "superscript" => "sup",
            "link" => {
                let attrs = mark
                    .get("attrs")
                    .and_then(Value::as_object)
                    .ok_or_else(invalid_draft)?;
                let href = string_field(attrs, "href")?;
                if href.len() > 2048 {
                    return Err(unsafe_link());
                }
                let normalized = normalize_href(
                    href,
                    chapter_resource,
                    reading_session_id,
                    manifest_resources,
                    false,
                )
                .map_err(|_| unsafe_link())?;
                let mut start = BytesStart::new("a");
                start.push_attribute(("href", normalized.as_str()));
                if let Some(title) = attrs.get("title").and_then(Value::as_str) {
                    start.push_attribute(("title", title));
                }
                writer
                    .write_event(Event::Start(start))
                    .map_err(|_| serialization_failed())?;
                opened.push("a");
                continue;
            }
            "publisherSpan" => {
                let attrs = mark
                    .get("attrs")
                    .and_then(Value::as_object)
                    .ok_or_else(invalid_draft)?;
                let mut start = BytesStart::new("span");
                for (json_name, xml_name) in [
                    ("id", "id"),
                    ("class", "class"),
                    ("role", "role"),
                    ("lang", "lang"),
                    ("xmlLang", "xml:lang"),
                    ("dir", "dir"),
                    ("epubType", "epub:type"),
                    ("title", "title"),
                ] {
                    if let Some(value) = attrs.get(json_name).and_then(Value::as_str) {
                        validate_plain_attribute(value)?;
                        if value.is_empty() && matches!(json_name, "id" | "class" | "role") {
                            return Err(invalid_draft());
                        }
                        start.push_attribute((xml_name, value));
                    }
                }
                writer
                    .write_event(Event::Start(start))
                    .map_err(|_| serialization_failed())?;
                opened.push("span");
                continue;
            }
            _ => return Err(invalid_draft()),
        };
        writer
            .write_event(Event::Start(BytesStart::new(tag)))
            .map_err(|_| serialization_failed())?;
        opened.push(tag);
    }
    Ok(opened)
}

fn write_inline_mark_ends(
    writer: &mut Writer<Vec<u8>>,
    opened: Vec<&'static str>,
) -> Result<(), AppError> {
    for tag in opened.into_iter().rev() {
        writer
            .write_event(Event::End(BytesEnd::new(tag)))
            .map_err(|_| serialization_failed())?;
    }
    Ok(())
}

fn node_attributes(
    kind: &str,
    object: &Map<String, Value>,
) -> Result<Vec<(&'static str, String)>, AppError> {
    let attrs = object.get("attrs").and_then(Value::as_object);
    let mut output = Vec::new();
    if matches!(kind, "paragraph" | "heading" | "blockquote") {
        for (json_name, xml_name) in [
            ("id", "id"),
            ("class", "class"),
            ("lang", "lang"),
            ("xmlLang", "xml:lang"),
            ("dir", "dir"),
            ("epubType", "epub:type"),
            ("title", "title"),
        ] {
            if let Some(value) = attrs
                .and_then(|attrs| attrs.get(json_name))
                .and_then(Value::as_str)
            {
                validate_plain_attribute(value)?;
                output.push((xml_name, value.to_owned()));
            }
        }
        if let Some(align) = attrs
            .and_then(|attrs| attrs.get("textAlign"))
            .and_then(Value::as_str)
        {
            if !matches!(align, "left" | "center" | "right" | "justify") {
                return Err(invalid_draft());
            }
            output.push(("style", format!("text-align: {align}")));
        }
    }
    if kind == "orderedList"
        && let Some(start) = attrs
            .and_then(|attrs| attrs.get("start"))
            .and_then(Value::as_u64)
    {
        output.push(("start", start.to_string()));
    }
    Ok(output)
}

fn validate_plain_attribute(value: &str) -> Result<(), AppError> {
    if value.len() <= 1024 && !value.chars().any(|character| character == '\0') {
        Ok(())
    } else {
        Err(invalid_draft())
    }
}

fn write_start_owned(
    writer: &mut Writer<Vec<u8>>,
    tag: &str,
    attrs: &[(&str, String)],
) -> Result<(), AppError> {
    let mut start = BytesStart::new(tag);
    for (key, value) in attrs {
        start.push_attribute((*key, value.as_str()));
    }
    writer
        .write_event(Event::Start(start))
        .map_err(|_| serialization_failed())
}

fn write_empty_owned(
    writer: &mut Writer<Vec<u8>>,
    tag: &str,
    attrs: &[(&str, String)],
) -> Result<(), AppError> {
    let mut start = BytesStart::new(tag);
    for (key, value) in attrs {
        start.push_attribute((*key, value.as_str()));
    }
    writer
        .write_event(Event::Empty(start))
        .map_err(|_| serialization_failed())
}

fn write_empty(
    writer: &mut Writer<Vec<u8>>,
    tag: &str,
    attrs: &[(&str, &str)],
) -> Result<(), AppError> {
    let mut start = BytesStart::new(tag);
    for (key, value) in attrs {
        start.push_attribute((*key, *value));
    }
    writer
        .write_event(Event::Empty(start))
        .map_err(|_| serialization_failed())
}

fn normalize_href(
    href: &str,
    chapter_resource: &str,
    reading_session_id: &str,
    manifest_resources: &HashSet<String>,
    image: bool,
) -> Result<String, String> {
    let href = href.trim();
    if href.is_empty() || href.len() > 2048 || href.contains(['\\', '\0']) || href.starts_with("//")
    {
        return Err("链接为空或包含不安全路径。".to_owned());
    }
    let protocol_prefix = format!("http://readloom-epub.localhost/{reading_session_id}/");
    if let Some(path) = href.strip_prefix(&protocol_prefix) {
        let decoded = percent_decode_str(path)
            .decode_utf8()
            .map_err(|_| "资源 URL 编码无效。".to_owned())?;
        let safe =
            SafeArchivePath::parse(decoded.as_ref()).map_err(|_| "资源路径无效。".to_owned())?;
        if !manifest_resources.contains(safe.as_str()) {
            return Err("资源不在 EPUB manifest 中。".to_owned());
        }
        return Ok(relative_href(chapter_resource, safe.as_str()));
    }
    if href.starts_with("http://") || href.starts_with("https://") {
        return if image {
            Err("外部图片不允许进入章节草稿。".to_owned())
        } else {
            Ok(href.to_owned())
        };
    }
    if href.starts_with('#') {
        return if image {
            Err("图片不能只引用 fragment。".to_owned())
        } else {
            Ok(href.to_owned())
        };
    }
    let target = resolve_internal_target(href, chapter_resource)?;
    if !manifest_resources.contains(&target) {
        return Err("内部链接目标不在 EPUB manifest 中。".to_owned());
    }
    let fragment = href.split_once('#').map(|(_, fragment)| fragment);
    let mut normalized = relative_href(chapter_resource, &target);
    if let Some(fragment) = fragment.filter(|value| !value.is_empty()) {
        normalized.push('#');
        normalized.push_str(fragment);
    }
    Ok(normalized)
}

fn resolve_internal_target(href: &str, chapter_resource: &str) -> Result<String, String> {
    if href.contains('?') {
        return Err("EPUB 内部链接不支持查询参数。".to_owned());
    }
    let path = href.split('#').next().unwrap_or("");
    if path.is_empty() {
        return Ok(chapter_resource.to_owned());
    }
    let decoded = percent_decode_str(path)
        .decode_utf8()
        .map_err(|_| "内部路径编码无效。".to_owned())?;
    if percent_decode_str(decoded.as_ref())
        .decode_utf8()
        .ok()
        .is_some_and(|twice| twice.as_ref() != decoded.as_ref())
    {
        return Err("内部路径不能进行二次解码。".to_owned());
    }
    let mut parts = if decoded.starts_with('/') {
        Vec::new()
    } else {
        chapter_resource
            .split('/')
            .take(chapter_resource.split('/').count().saturating_sub(1))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    for part in decoded.trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("内部路径试图逃逸 EPUB 根目录。".to_owned());
                }
            }
            value if value.contains(':') => return Err("内部路径包含主机路径或协议。".to_owned()),
            value => parts.push(value.to_owned()),
        }
    }
    let joined = parts.join("/");
    SafeArchivePath::parse(&joined)
        .map(|safe| safe.as_str().to_owned())
        .map_err(|_| "内部路径无效。".to_owned())
}

fn relative_href(from: &str, target: &str) -> String {
    let from_parts = from.split('/').collect::<Vec<_>>();
    let target_parts = target.split('/').collect::<Vec<_>>();
    let from_dir = &from_parts[..from_parts.len().saturating_sub(1)];
    let mut common = 0;
    while common < from_dir.len()
        && common < target_parts.len()
        && from_dir[common] == target_parts[common]
    {
        common += 1;
    }
    let mut parts = vec![".."; from_dir.len().saturating_sub(common)];
    parts.extend_from_slice(&target_parts[common..]);
    if parts.is_empty() {
        target_parts.last().copied().unwrap_or("").to_owned()
    } else {
        parts.join("/")
    }
}

fn validate_normalized_xhtml(source: &[u8], limits: EpubChapterEditLimits) -> Result<(), AppError> {
    let parsed = parse_body(source, limits)?;
    if parsed.nodes > limits.maximum_nodes
        || parsed.text_characters > limits.maximum_text_characters
    {
        return Err(chapter_too_complex());
    }
    Ok(())
}

fn content_field(object: &Map<String, Value>) -> Result<&[Value], AppError> {
    Ok(object
        .get("content")
        .map(|value| value.as_array().ok_or_else(invalid_draft))
        .transpose()?
        .map(Vec::as_slice)
        .unwrap_or(&[]))
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, AppError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(invalid_draft)
}

fn empty_document() -> Value {
    json!({"type":"doc", "content":[{"type":"paragraph"}]})
}

fn unavailable_analysis(
    source: &[u8],
    original_resource_hash: String,
    compatibility_level: ChapterCompatibilityLevel,
    code: &str,
    message: &str,
) -> AnalyzedChapter {
    AnalyzedChapter {
        original_xhtml: source.to_vec(),
        preserved_outer_document: None,
        editor_document: empty_document(),
        compatibility_level,
        warnings: vec![warning(code, message)],
        original_resource_hash,
        validation_state: ChapterValidationState::Invalid,
    }
}

fn warning(code: &str, message: impl Into<String>) -> ChapterEditWarning {
    ChapterEditWarning {
        code: code.to_owned(),
        message: message.into(),
    }
}

fn deduplicate_warnings(warnings: &mut Vec<ChapterEditWarning>) {
    let mut seen = HashSet::new();
    warnings.retain(|warning| seen.insert(warning.code.clone()));
}

fn parse_failed() -> AppError {
    AppError::validation(
        "CHAPTER_PARSE_FAILED",
        "章节 XHTML 无法安全解析。",
        "该章节仍可阅读，但不能进入可视化编辑。",
    )
}

fn invalid_draft() -> AppError {
    AppError::validation(
        "INVALID_CHAPTER_DRAFT",
        "章节编辑草稿不符合受控文档模型。",
        "当前编辑内容仍保留在编辑器中；请撤销最近操作后重试同步。",
    )
}

fn serialization_failed() -> AppError {
    AppError::validation(
        "CHAPTER_SERIALIZATION_FAILED",
        "章节草稿无法生成有效 XHTML。",
        "当前编辑内容仍保留在编辑器中；请撤销最近操作后重试。",
    )
}

fn chapter_too_large() -> AppError {
    AppError::validation(
        "CHAPTER_TOO_LARGE",
        "章节草稿超过可视化编辑大小限制。",
        "减少本次粘贴或图片数量后重试。",
    )
}

fn chapter_too_complex() -> AppError {
    AppError::validation(
        "CHAPTER_TOO_COMPLEX",
        "章节结构超过节点数或嵌套深度限制。",
        "该章节仍可阅读，但不能继续可视化编辑。",
    )
}

fn unsafe_link() -> AppError {
    AppError::validation(
        "UNSAFE_CHAPTER_LINK",
        "章节包含不安全或无法解析的链接。",
        "仅使用章节 fragment、EPUB 内部路径或 http/https 链接。",
    )
}

fn unsafe_resource() -> AppError {
    AppError::validation(
        "INVALID_INTERNAL_RESOURCE",
        "章节图片不在当前 EPUB 的受控资源清单中。",
        "请使用“导入图片”添加本地 PNG、JPEG 或 WebP。",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resources() -> HashSet<String> {
        [
            "EPUB/text/chapter.xhtml".to_owned(),
            "EPUB/text/other.xhtml".to_owned(),
            "EPUB/images/picture.png".to_owned(),
            "EPUB/styles/book.css".to_owned(),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn round_trips_semantic_xhtml_without_rebuilding_the_head() {
        let source = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="zh-CN"><head><title>原题</title><link rel="stylesheet" href="../styles/book.css" /></head><body class="chapter"><h1 epub:type="title">第一章</h1><p>中文 😀 e&#769; <strong>粗体</strong> <em>斜体</em> <a href="other.xhtml#x">链接</a></p><ol start="2"><li><p>项目</p></li></ol></body></html>"#;
        let analysis = analyze_chapter(
            source.as_bytes(),
            "EPUB/text/chapter.xhtml",
            "0123456789abcdef0123456789abcdef0123456789abcdef",
            &resources(),
            false,
            EpubChapterEditLimits::default(),
        );
        assert_eq!(
            analysis.compatibility_level,
            ChapterCompatibilityLevel::Full
        );
        let generated = serialize_editor_document(
            analysis.preserved_outer_document.as_ref().unwrap(),
            &analysis.editor_document,
            "EPUB/text/chapter.xhtml",
            "0123456789abcdef0123456789abcdef0123456789abcdef",
            &resources(),
            EpubChapterEditLimits::default(),
        )
        .unwrap();
        let generated = String::from_utf8(generated).unwrap();
        assert!(generated.contains("<title>原题</title>"));
        assert!(generated.contains("../styles/book.css"));
        assert!(generated.contains("epub:type=\"title\""));
        assert!(generated.contains("中文 😀 é"));
        assert!(generated.contains("<strong>粗体</strong>"));
        assert!(generated.contains("href=\"other.xhtml#x\""));
        assert!(generated.contains("<ol start=\"2\">"));
    }

    #[test]
    fn preserves_safe_publisher_role_and_image_identity_attributes() {
        let source = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:ibooks="http://www.apple.com/2011/iBooks" xmlns:m="http://www.w3.org/1998/Math/MathML" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="zh-Hant" xmlns="http://www.w3.org/1999/xhtml"><head><meta charset="utf-8"/><title>Untitled</title><link rel="stylesheet" type="text/css" href="assets/css/content5.css"/></head><body><p id="p1" class="s1"><img class="s2 s3" width="500" height="28" src="assets/images/Chapter_Section_Rule_new.jpg" id="image-26" alt=""/></p><p id="p2" class="s4"><span role="heading">Let's&amp;go 章节标题</span></p><p id="p3" class="s5"><span class="c1"><img class="s2" width="500" height="28" src="assets/images/Chapter_Section_Rule_new.jpg" id="image-27" alt=""/></span></p></body></html>"#;
        let resources = [
            "OPS/content5.xhtml".to_owned(),
            "OPS/assets/css/content5.css".to_owned(),
            "OPS/assets/images/Chapter_Section_Rule_new.jpg".to_owned(),
        ]
        .into_iter()
        .collect();
        let analysis = analyze_chapter(
            source.as_bytes(),
            "OPS/content5.xhtml",
            "0123456789abcdef0123456789abcdef0123456789abcdef",
            &resources,
            false,
            EpubChapterEditLimits::default(),
        );

        assert_eq!(
            analysis.compatibility_level,
            ChapterCompatibilityLevel::Full,
            "safe publisher attributes should remain editable: {:?}",
            analysis.warnings
        );
        let generated = serialize_editor_document(
            analysis.preserved_outer_document.as_ref().unwrap(),
            &analysis.editor_document,
            "OPS/content5.xhtml",
            "0123456789abcdef0123456789abcdef0123456789abcdef",
            &resources,
            EpubChapterEditLimits::default(),
        )
        .unwrap();
        let generated = String::from_utf8(generated).unwrap();
        assert!(
            generated.contains("<span role=\"heading\">Let&apos;s&amp;go 章节标题</span>"),
            "generated XHTML: {generated}"
        );
        assert!(generated.contains("id=\"image-26\""));
        assert!(generated.contains("class=\"s2 s3\""));
        assert!(generated.contains("<span class=\"c1\"><img"));
        assert!(generated.contains("id=\"image-27\""));
    }

    #[test]
    fn dangerous_or_lossy_structures_are_read_only() {
        for (source, code) in [
            (
                "<html><body><script>alert(1)</script></body></html>",
                "UNSAFE_CHAPTER_ELEMENT",
            ),
            (
                "<html><body><math><mi>x</mi></math></body></html>",
                "UNSUPPORTED_CHAPTER_ELEMENT",
            ),
            (
                "<html><body><svg><path /></svg></body></html>",
                "UNSUPPORTED_CHAPTER_ELEMENT",
            ),
            (
                "<html><body><table><tr><td>x</td></tr></table></body></html>",
                "UNSUPPORTED_CHAPTER_ELEMENT",
            ),
        ] {
            let analysis = analyze_chapter(
                source.as_bytes(),
                "EPUB/text/chapter.xhtml",
                "0123456789abcdef0123456789abcdef0123456789abcdef",
                &resources(),
                false,
                EpubChapterEditLimits::default(),
            );
            assert_eq!(
                analysis.compatibility_level,
                ChapterCompatibilityLevel::ReadOnly
            );
            assert!(analysis.warnings.iter().any(|warning| warning.code == code));
        }
    }

    #[test]
    fn serializer_rejects_scripts_external_images_and_javascript_links() {
        let source = b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>x</title></head><body><p>x</p></body></html>";
        let analysis = analyze_chapter(
            source,
            "EPUB/text/chapter.xhtml",
            "0123456789abcdef0123456789abcdef0123456789abcdef",
            &resources(),
            false,
            EpubChapterEditLimits::default(),
        );
        let outer = analysis.preserved_outer_document.as_ref().unwrap();
        for document in [
            json!({"type":"doc","content":[{"type":"script"}]}),
            json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"image","attrs":{"src":"https://tracker.invalid/pixel.png","alt":""}}]}]}),
            json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"bad","marks":[{"type":"link","attrs":{"href":"javascript:alert(1)"}}]}]}]}),
        ] {
            assert!(
                serialize_editor_document(
                    outer,
                    &document,
                    "EPUB/text/chapter.xhtml",
                    "0123456789abcdef0123456789abcdef0123456789abcdef",
                    &resources(),
                    EpubChapterEditLimits::default(),
                )
                .is_err()
            );
        }
    }
}
