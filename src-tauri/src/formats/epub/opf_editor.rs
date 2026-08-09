use std::collections::{HashMap, HashSet};

use quick_xml::{
    Reader, Writer, XmlVersion,
    events::{BytesEnd, BytesStart, BytesText, Event},
};

use crate::{
    domain::epub_edit::EpubMetadataDraft, error::AppError,
    infrastructure::archive::safe_zip::SafeArchivePath,
};

const MAXIMUM_XML_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoverManifestChange {
    pub item_id: String,
    pub resource_id: String,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestAddition {
    pub item_id: String,
    pub resource_id: String,
    pub media_type: String,
}

#[allow(dead_code)]
pub(crate) fn patch_opf(
    source: &[u8],
    original: &EpubMetadataDraft,
    current: &EpubMetadataDraft,
    opf_resource_id: &str,
    epub_version: &str,
    cover: Option<&CoverManifestChange>,
    modified_at: &str,
) -> Result<Vec<u8>, AppError> {
    patch_opf_with_resources(
        source,
        original,
        current,
        opf_resource_id,
        epub_version,
        cover,
        &[],
        modified_at,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn patch_opf_with_resources(
    source: &[u8],
    original: &EpubMetadataDraft,
    current: &EpubMetadataDraft,
    opf_resource_id: &str,
    epub_version: &str,
    cover: Option<&CoverManifestChange>,
    additions: &[ManifestAddition],
    modified_at: &str,
) -> Result<Vec<u8>, AppError> {
    let changed = changed_fields(original, current);
    if changed.is_empty() && cover.is_none() && additions.is_empty() {
        validate_xml(source)?;
        return Ok(source.to_vec());
    }

    let mut reader = Reader::from_reader(source);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(source.len() + 1024));
    let mut depth = 0_usize;
    let mut metadata_depth = None;
    let mut manifest_depth = None;
    let mut active_text: Option<(usize, String, bool)> = None;
    let mut skip_depth = None;
    let mut counters = HashMap::<String, usize>::new();
    let mut unique_identifier = None;
    let mut identifier_patched = false;
    let mut modified_seen = false;
    let mut epub2_cover_seen = false;

    loop {
        let event = reader.read_event().map_err(|_| unsafe_opf())?;
        match event {
            Event::Eof => break,
            Event::DocType(_) => return Err(unsafe_opf()),
            Event::Start(start) => {
                depth += 1;
                if depth > MAXIMUM_XML_DEPTH {
                    return Err(unsafe_opf());
                }
                if skip_depth.is_some() {
                    continue;
                }
                let local = local_name(start.name().as_ref()).to_owned();
                if local == b"package" {
                    unique_identifier = attribute_value(&reader, &start, b"unique-identifier")?;
                }
                if local == b"metadata" {
                    metadata_depth = Some(depth);
                } else if local == b"manifest" {
                    manifest_depth = Some(depth);
                }

                if metadata_depth.is_some_and(|base| depth == base + 1) {
                    if local == b"meta" {
                        let property = attribute_value(&reader, &start, b"property")?;
                        let name = attribute_value(&reader, &start, b"name")?;
                        if epub_version.starts_with('3')
                            && property.as_deref() == Some("dcterms:modified")
                        {
                            modified_seen = true;
                            writer
                                .write_event(Event::Start(start.into_owned()))
                                .map_err(write_failed)?;
                            active_text = Some((depth, modified_at.to_owned(), false));
                            continue;
                        }
                        if epub_version.starts_with('2')
                            && name.as_deref() == Some("cover")
                            && let Some(cover) = cover
                        {
                            epub2_cover_seen = true;
                            let updated =
                                replace_attribute(&reader, &start, b"content", &cover.item_id)?;
                            writer
                                .write_event(Event::Start(updated))
                                .map_err(write_failed)?;
                            continue;
                        }
                    }

                    let field = metadata_field(&local);
                    if let Some(field) = field
                        && changed.contains(field)
                    {
                        let replacement = values_for_field(current, field);
                        let index = counters.entry(field.to_owned()).or_default();
                        let patch_this_identifier = field != "identifier"
                            || identifier_matches(&reader, &start, unique_identifier.as_deref())?;
                        if patch_this_identifier {
                            if let Some(value) = replacement.get(*index) {
                                writer
                                    .write_event(Event::Start(start.into_owned()))
                                    .map_err(write_failed)?;
                                active_text = Some((depth, value.clone(), false));
                                *index += 1;
                                if field == "identifier" {
                                    identifier_patched = true;
                                }
                            } else {
                                skip_depth = Some(depth);
                            }
                            continue;
                        }
                    }
                }

                if manifest_depth.is_some_and(|base| depth == base + 1)
                    && local == b"item"
                    && cover.is_some()
                {
                    let updated = remove_property(&reader, &start, "cover-image")?;
                    writer
                        .write_event(Event::Start(updated))
                        .map_err(write_failed)?;
                    continue;
                }
                writer
                    .write_event(Event::Start(start.into_owned()))
                    .map_err(write_failed)?;
            }
            Event::Empty(start) => {
                if skip_depth.is_some() {
                    continue;
                }
                let event_depth = depth + 1;
                let local = local_name(start.name().as_ref()).to_owned();
                if metadata_depth.is_some_and(|base| event_depth == base + 1)
                    && local == b"meta"
                    && epub_version.starts_with('2')
                    && attribute_value(&reader, &start, b"name")?.as_deref() == Some("cover")
                    && let Some(cover) = cover
                {
                    epub2_cover_seen = true;
                    let updated = replace_attribute(&reader, &start, b"content", &cover.item_id)?;
                    writer
                        .write_event(Event::Empty(updated))
                        .map_err(write_failed)?;
                    continue;
                }
                if manifest_depth.is_some_and(|base| event_depth == base + 1)
                    && local == b"item"
                    && cover.is_some()
                {
                    let updated = remove_property(&reader, &start, "cover-image")?;
                    writer
                        .write_event(Event::Empty(updated))
                        .map_err(write_failed)?;
                    continue;
                }
                writer
                    .write_event(Event::Empty(start.into_owned()))
                    .map_err(write_failed)?;
            }
            Event::Text(text) => {
                if skip_depth.is_some() {
                    continue;
                }
                if let Some((active_depth, value, written)) = active_text.as_mut()
                    && *active_depth == depth
                {
                    if !*written {
                        writer
                            .write_event(Event::Text(BytesText::new(value)))
                            .map_err(write_failed)?;
                        *written = true;
                    }
                    continue;
                }
                writer
                    .write_event(Event::Text(text.into_owned()))
                    .map_err(write_failed)?;
            }
            Event::CData(_) if active_text.is_some() => return Err(unsafe_opf()),
            Event::End(end) => {
                if skip_depth == Some(depth) {
                    skip_depth = None;
                    depth = depth.saturating_sub(1);
                    continue;
                }
                if skip_depth.is_some() {
                    depth = depth.saturating_sub(1);
                    continue;
                }
                if let Some((active_depth, value, written)) = active_text.as_mut()
                    && *active_depth == depth
                {
                    if !*written {
                        writer
                            .write_event(Event::Text(BytesText::new(value)))
                            .map_err(write_failed)?;
                    }
                    active_text = None;
                }
                if metadata_depth == Some(depth) {
                    append_missing_metadata(
                        &mut writer,
                        &changed,
                        &mut counters,
                        current,
                        identifier_patched,
                    )?;
                    if epub_version.starts_with('3') && !modified_seen {
                        let mut meta = BytesStart::new("meta");
                        meta.push_attribute(("property", "dcterms:modified"));
                        write_text_element(&mut writer, meta, modified_at)?;
                    }
                    if epub_version.starts_with('2')
                        && !epub2_cover_seen
                        && let Some(cover) = cover
                    {
                        let mut meta = BytesStart::new("meta");
                        meta.push_attribute(("name", "cover"));
                        meta.push_attribute(("content", cover.item_id.as_str()));
                        writer
                            .write_event(Event::Empty(meta))
                            .map_err(write_failed)?;
                    }
                    metadata_depth = None;
                }
                if manifest_depth == Some(depth) {
                    if let Some(cover) = cover {
                        let mut item = BytesStart::new("item");
                        item.push_attribute(("id", cover.item_id.as_str()));
                        let href = relative_archive_path(opf_resource_id, &cover.resource_id)?;
                        item.push_attribute(("href", href.as_str()));
                        item.push_attribute(("media-type", cover.media_type.as_str()));
                        if epub_version.starts_with('3') {
                            item.push_attribute(("properties", "cover-image"));
                        }
                        writer
                            .write_event(Event::Empty(item))
                            .map_err(write_failed)?;
                    }
                    for addition in additions {
                        let mut item = BytesStart::new("item");
                        item.push_attribute(("id", addition.item_id.as_str()));
                        let href = relative_archive_path(opf_resource_id, &addition.resource_id)?;
                        item.push_attribute(("href", href.as_str()));
                        item.push_attribute(("media-type", addition.media_type.as_str()));
                        writer
                            .write_event(Event::Empty(item))
                            .map_err(write_failed)?;
                    }
                    manifest_depth = None;
                }
                writer
                    .write_event(Event::End(end.into_owned()))
                    .map_err(write_failed)?;
                depth = depth.saturating_sub(1);
            }
            other => {
                if skip_depth.is_none() {
                    writer
                        .write_event(other.into_owned())
                        .map_err(write_failed)?;
                }
            }
        }
    }
    if depth != 0 || metadata_depth.is_some() || manifest_depth.is_some() {
        return Err(unsafe_opf());
    }
    if changed.contains("identifier") && !identifier_patched {
        return Err(AppError::validation(
            "INVALID_IDENTIFIER",
            "无法定位 package unique-identifier 引用的标识符。",
            "请选择结构完整的 EPUB，或保持 identifier 不变。",
        ));
    }
    let output = writer.into_inner();
    validate_xml(&output)?;
    Ok(output)
}

pub(crate) fn validate_xml(source: &[u8]) -> Result<(), AppError> {
    let mut reader = Reader::from_reader(source);
    reader.config_mut().trim_text(false);
    let mut depth = 0_usize;
    loop {
        match reader.read_event().map_err(|_| unsafe_opf())? {
            Event::Eof => break,
            Event::DocType(_) => return Err(unsafe_opf()),
            Event::Start(_) => {
                depth += 1;
                if depth > MAXIMUM_XML_DEPTH {
                    return Err(unsafe_opf());
                }
            }
            Event::End(_) => depth = depth.checked_sub(1).ok_or_else(unsafe_opf)?,
            _ => {}
        }
    }
    if depth == 0 {
        Ok(())
    } else {
        Err(unsafe_opf())
    }
}

pub(crate) fn changed_fields(
    original: &EpubMetadataDraft,
    current: &EpubMetadataDraft,
) -> HashSet<&'static str> {
    let mut fields = HashSet::new();
    if original.title != current.title {
        fields.insert("title");
    }
    if original.creators != current.creators {
        fields.insert("creators");
    }
    if original.contributors != current.contributors {
        fields.insert("contributors");
    }
    if original.language != current.language {
        fields.insert("language");
    }
    if original.publisher != current.publisher {
        fields.insert("publisher");
    }
    if original.description != current.description {
        fields.insert("description");
    }
    if original.identifier != current.identifier {
        fields.insert("identifier");
    }
    if original.publication_date != current.publication_date {
        fields.insert("publicationDate");
    }
    if original.subjects != current.subjects {
        fields.insert("subjects");
    }
    if original.rights != current.rights {
        fields.insert("rights");
    }
    fields
}

fn append_missing_metadata(
    writer: &mut Writer<Vec<u8>>,
    changed: &HashSet<&str>,
    counters: &mut HashMap<String, usize>,
    current: &EpubMetadataDraft,
    identifier_patched: bool,
) -> Result<(), AppError> {
    for field in [
        "title",
        "creators",
        "contributors",
        "language",
        "publisher",
        "description",
        "identifier",
        "publicationDate",
        "subjects",
        "rights",
    ] {
        if !changed.contains(field) || (field == "identifier" && identifier_patched) {
            continue;
        }
        let values = values_for_field(current, field);
        let start = *counters.get(field).unwrap_or(&0);
        for value in values.iter().skip(start) {
            let name = match field {
                "creators" => "dc:creator",
                "contributors" => "dc:contributor",
                "publicationDate" => "dc:date",
                "subjects" => "dc:subject",
                "rights" => "dc:rights",
                value => match value {
                    "title" => "dc:title",
                    "language" => "dc:language",
                    "publisher" => "dc:publisher",
                    "description" => "dc:description",
                    "identifier" => "dc:identifier",
                    _ => unreachable!("known metadata field"),
                },
            };
            write_text_element(writer, BytesStart::new(name), value)?;
        }
    }
    Ok(())
}

fn write_text_element(
    writer: &mut Writer<Vec<u8>>,
    start: BytesStart<'_>,
    value: &str,
) -> Result<(), AppError> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let end = BytesEnd::new(name);
    writer
        .write_event(Event::Start(start))
        .map_err(write_failed)?;
    writer
        .write_event(Event::Text(BytesText::new(value)))
        .map_err(write_failed)?;
    writer.write_event(Event::End(end)).map_err(write_failed)
}

fn values_for_field(metadata: &EpubMetadataDraft, field: &str) -> Vec<String> {
    match field {
        "title" => vec![metadata.title.clone()],
        "creators" => metadata.creators.clone(),
        "contributors" => metadata.contributors.clone(),
        "language" => vec![metadata.language.clone()],
        "publisher" => metadata.publisher.iter().cloned().collect(),
        "description" => metadata.description.iter().cloned().collect(),
        "identifier" => vec![metadata.identifier.clone()],
        "publicationDate" => metadata.publication_date.iter().cloned().collect(),
        "subjects" => metadata.subjects.clone(),
        "rights" => metadata.rights.clone(),
        _ => Vec::new(),
    }
}

fn metadata_field(local: &[u8]) -> Option<&'static str> {
    match local {
        b"title" => Some("title"),
        b"creator" => Some("creators"),
        b"contributor" => Some("contributors"),
        b"language" => Some("language"),
        b"publisher" => Some("publisher"),
        b"description" => Some("description"),
        b"identifier" => Some("identifier"),
        b"date" => Some("publicationDate"),
        b"subject" => Some("subjects"),
        b"rights" => Some("rights"),
        _ => None,
    }
}

fn identifier_matches(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    unique_identifier: Option<&str>,
) -> Result<bool, AppError> {
    let Some(unique_identifier) = unique_identifier else {
        return Ok(false);
    };
    Ok(attribute_value(reader, start, b"id")?.as_deref() == Some(unique_identifier))
}

fn attribute_value(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, AppError> {
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| unsafe_opf())?;
        if local_name(attribute.key.as_ref()) == name {
            return attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(|_| unsafe_opf());
        }
    }
    Ok(None)
}

fn replace_attribute(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    name: &[u8],
    value: &str,
) -> Result<BytesStart<'static>, AppError> {
    let mut updated = BytesStart::new(String::from_utf8_lossy(start.name().as_ref()).into_owned());
    let mut replaced = false;
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| unsafe_opf())?;
        if local_name(attribute.key.as_ref()) == name {
            updated.push_attribute((
                String::from_utf8_lossy(attribute.key.as_ref()).as_ref(),
                value,
            ));
            replaced = true;
        } else {
            let decoded = attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                .map_err(|_| unsafe_opf())?;
            updated.push_attribute((
                String::from_utf8_lossy(attribute.key.as_ref()).as_ref(),
                decoded.as_ref(),
            ));
        }
    }
    if !replaced {
        updated.push_attribute((String::from_utf8_lossy(name).as_ref(), value));
    }
    Ok(updated.into_owned())
}

fn remove_property(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    removed: &str,
) -> Result<BytesStart<'static>, AppError> {
    let mut updated = BytesStart::new(String::from_utf8_lossy(start.name().as_ref()).into_owned());
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| unsafe_opf())?;
        let decoded = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|_| unsafe_opf())?;
        let value = if local_name(attribute.key.as_ref()) == b"properties" {
            decoded
                .split_ascii_whitespace()
                .filter(|value| *value != removed)
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            decoded.into_owned()
        };
        if !value.is_empty() {
            updated.push_attribute((
                String::from_utf8_lossy(attribute.key.as_ref()).as_ref(),
                value.as_str(),
            ));
        }
    }
    Ok(updated.into_owned())
}

fn relative_archive_path(from_file: &str, target: &str) -> Result<String, AppError> {
    SafeArchivePath::parse(from_file)?;
    SafeArchivePath::parse(target)?;
    let mut from = from_file.split('/').collect::<Vec<_>>();
    from.pop();
    let target = target.split('/').collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(target.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = vec![".."; from.len().saturating_sub(common)];
    parts.extend_from_slice(&target[common..]);
    if parts.is_empty() {
        return Err(unsafe_opf());
    }
    Ok(parts.join("/"))
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn unsafe_opf() -> AppError {
    AppError::validation(
        "PACKAGE_UPDATE_FAILED",
        "EPUB package 文档不安全或无法可靠修改。",
        "请选择结构完整且不含 DTD/外部实体的 EPUB。",
    )
}

fn write_failed(_: std::io::Error) -> AppError {
    AppError::internal("PACKAGE_UPDATE_FAILED", "serialize patched OPF")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> EpubMetadataDraft {
        EpubMetadataDraft {
            title: "原书名".to_owned(),
            creators: vec!["作者 & 一".to_owned(), "作者二".to_owned()],
            contributors: Vec::new(),
            language: "zh-CN".to_owned(),
            publisher: None,
            description: Some("原简介".to_owned()),
            identifier: "urn:old".to_owned(),
            publication_date: None,
            subjects: vec!["测试".to_owned()],
            rights: Vec::new(),
        }
    }

    fn source() -> &'static [u8] {
        r##"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0" unique-identifier="pub-id"><metadata><dc:identifier id="pub-id">urn:old</dc:identifier><dc:title id="title">原书名</dc:title><meta refines="#title" property="title-type">main</meta><dc:creator id="creator">作者 &amp; 一</dc:creator><meta refines="#creator" property="role" scheme="marc:relators">aut</meta><dc:creator>作者二</dc:creator><dc:language>zh-CN</dc:language><dc:description>原简介</dc:description><dc:subject>测试</dc:subject><meta property="custom:unknown">保留</meta><meta property="dcterms:modified">2026-01-01T00:00:00Z</meta></metadata><manifest><item id="old-cover" href="cover.png" media-type="image/png" properties="cover-image"/><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"##.as_bytes()
    }

    #[test]
    fn patches_known_metadata_as_xml_text_and_preserves_unknown_nodes_and_refinements() {
        let original = metadata();
        let mut current = original.clone();
        current.title = "新书名 <安全> & 文本".to_owned();
        current.creators[0] = "新作者".to_owned();
        let output = patch_opf(
            source(),
            &original,
            &current,
            "EPUB/package.opf",
            "3.0",
            None,
            "2026-08-08T10:00:00Z",
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("新书名 &lt;安全&gt; &amp; 文本"));
        assert!(output.contains("refines=\"#title\""));
        assert!(output.contains("refines=\"#creator\""));
        assert!(output.contains("custom:unknown"));
        assert!(output.contains("2026-08-08T10:00:00Z"));
    }

    #[test]
    fn rejects_doctype_before_modifying_untrusted_xml() {
        let error = patch_opf(
            br#"<!DOCTYPE package SYSTEM "https://example.com/evil.dtd"><package/>"#,
            &metadata(),
            &metadata(),
            "package.opf",
            "3.0",
            None,
            "2026-08-08T10:00:00Z",
        )
        .unwrap_err();
        assert_eq!(error.to_dto().code, "PACKAGE_UPDATE_FAILED");
    }

    #[test]
    fn replaces_cover_markers_without_removing_the_old_resource() {
        let cover = CoverManifestChange {
            item_id: "readloom-cover".to_owned(),
            resource_id: "EPUB/readloom-assets/cover.png".to_owned(),
            media_type: "image/png".to_owned(),
        };
        let output = patch_opf(
            source(),
            &metadata(),
            &metadata(),
            "EPUB/package.opf",
            "3.0",
            Some(&cover),
            "2026-08-08T10:00:00Z",
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("id=\"old-cover\""));
        assert!(output.contains("id=\"readloom-cover\""));
        assert!(output.contains("href=\"readloom-assets/cover.png\""));
        assert_eq!(output.matches("cover-image").count(), 1);
    }
}
