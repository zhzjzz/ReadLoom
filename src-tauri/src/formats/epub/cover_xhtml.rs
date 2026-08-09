use quick_xml::{
    Reader, Writer, XmlVersion,
    events::{BytesStart, Event},
};

use crate::{error::AppError, infrastructure::archive::safe_zip::SafeArchivePath};

pub(crate) fn patch_cover_reference(
    source: &[u8],
    document_resource_id: &str,
    old_cover_resource_id: &str,
    new_cover_resource_id: &str,
) -> Result<Option<Vec<u8>>, AppError> {
    let mut reader = Reader::from_reader(source);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(source.len() + 128));
    let mut changed = false;
    loop {
        match reader.read_event().map_err(|_| unsafe_cover_document())? {
            Event::Eof => break,
            Event::DocType(_) => return Err(unsafe_cover_document()),
            Event::Start(start) => {
                let (updated, did_change) = patch_attributes(
                    &reader,
                    &start,
                    document_resource_id,
                    old_cover_resource_id,
                    new_cover_resource_id,
                )?;
                changed |= did_change;
                writer
                    .write_event(Event::Start(updated))
                    .map_err(write_failed)?;
            }
            Event::Empty(start) => {
                let (updated, did_change) = patch_attributes(
                    &reader,
                    &start,
                    document_resource_id,
                    old_cover_resource_id,
                    new_cover_resource_id,
                )?;
                changed |= did_change;
                writer
                    .write_event(Event::Empty(updated))
                    .map_err(write_failed)?;
            }
            event => writer
                .write_event(event.into_owned())
                .map_err(write_failed)?,
        }
    }
    Ok(changed.then(|| writer.into_inner()))
}

fn patch_attributes(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    document_resource_id: &str,
    old_cover_resource_id: &str,
    new_cover_resource_id: &str,
) -> Result<(BytesStart<'static>, bool), AppError> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let mut updated = BytesStart::new(name);
    let mut changed = false;
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| unsafe_cover_document())?;
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        let value = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|_| unsafe_cover_document())?
            .into_owned();
        let local = attribute
            .key
            .as_ref()
            .rsplit(|byte| *byte == b':')
            .next()
            .unwrap_or(attribute.key.as_ref());
        let replacement = if matches!(local, b"src" | b"href")
            && resolves_to(document_resource_id, &value, old_cover_resource_id)
        {
            changed = true;
            relative_archive_path(document_resource_id, new_cover_resource_id)?
        } else {
            value
        };
        updated.push_attribute((key.as_str(), replacement.as_str()));
    }
    Ok((updated.into_owned(), changed))
}

fn resolves_to(document: &str, reference: &str, expected: &str) -> bool {
    if reference.contains("//") || reference.starts_with(['/', '#']) {
        return false;
    }
    let clean = reference.split(['#', '?']).next().unwrap_or_default();
    if clean.is_empty() {
        return false;
    }
    let parent = document
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or_default();
    let joined = if parent.is_empty() {
        clean.to_owned()
    } else {
        format!("{parent}/{clean}")
    };
    SafeArchivePath::parse(&joined)
        .ok()
        .is_some_and(|path| path.as_str() == expected)
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
    Ok(parts.join("/"))
}

fn unsafe_cover_document() -> AppError {
    AppError::validation(
        "MANIFEST_UPDATE_FAILED",
        "无法安全更新封面文档中的图片引用。",
        "原 EPUB 和草稿均未改变；请选择其他 EPUB。",
    )
}

fn write_failed(_: std::io::Error) -> AppError {
    AppError::internal("MANIFEST_UPDATE_FAILED", "serialize cover XHTML")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_only_the_internal_image_that_resolves_to_the_old_cover() {
        let source = br#"<html><body><img src="../images/old.png"/><a href="chapter.xhtml">keep</a></body></html>"#;
        let output = patch_cover_reference(
            source,
            "EPUB/text/cover.xhtml",
            "EPUB/images/old.png",
            "EPUB/readloom-assets/new.png",
        )
        .unwrap()
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("src=\"../readloom-assets/new.png\""));
        assert!(output.contains("href=\"chapter.xhtml\""));
    }
}
