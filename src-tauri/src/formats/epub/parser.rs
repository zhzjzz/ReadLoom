use std::path::Path;

use rbook::{Epub, epub::toc::EpubTocEntry};

use crate::{
    domain::{
        document::{DocumentCapabilities, DocumentKind},
        epub_document::{
            EpubLayout, EpubMetadata, ManifestItem, ParsedEpubDocument, SpineItem, TocNode,
        },
    },
    error::AppError,
    infrastructure::archive::{
        archive_limits::ArchiveLimits,
        safe_zip::{ResourceClass, SafeArchivePath, SafeEpubArchive},
    },
};

pub(crate) fn parse_epub_document(
    path: &Path,
    limits: ArchiveLimits,
) -> Result<ParsedEpubDocument, AppError> {
    let safe_archive = SafeEpubArchive::open(path, limits)?;
    let container_path = SafeArchivePath::parse("META-INF/container.xml")
        .expect("the EPUB container path is static and safe");
    if !safe_archive.contains(&container_path) {
        return Err(missing_container());
    }
    safe_archive.read(&container_path, ResourceClass::Xml)?;

    let epub = Epub::options()
        .strict(false)
        .open(path)
        .map_err(|_| invalid_epub())?;
    let version = epub.package().version_str();
    if !matches!(version.split('.').next(), Some("2" | "3")) {
        return Err(AppError::validation(
            "UNSUPPORTED_EPUB_VERSION",
            "Readloom 仅支持 EPUB 2 和 EPUB 3。",
            "请先将此书转换为 EPUB 2 或 EPUB 3。",
        ));
    }

    let metadata_view = epub.metadata();
    let metadata = EpubMetadata {
        title: metadata_view
            .title()
            .map(|value| value.value().trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "未命名 EPUB".to_owned()),
        creators: metadata_view
            .creators()
            .map(|value| value.value().trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect(),
        languages: metadata_view
            .languages()
            .map(|value| value.value().trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect(),
        publisher: metadata_view
            .publishers()
            .map(|value| value.value().trim().to_owned())
            .find(|value| !value.is_empty()),
        description: metadata_view
            .description()
            .map(|value| value.value().trim().to_owned())
            .filter(|value| !value.is_empty()),
        identifier: metadata_view
            .identifier()
            .map(|value| value.value().trim().to_owned())
            .filter(|value| !value.is_empty()),
        publication_date: metadata_view
            .published_entry()
            .map(|value| value.value().trim().to_owned())
            .filter(|value| !value.is_empty()),
        modified_date: metadata_view
            .modified_entry()
            .map(|value| value.value().trim().to_owned())
            .filter(|value| !value.is_empty()),
        rights: metadata_view
            .rights()
            .map(|value| value.value().trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect(),
        subjects: metadata_view
            .tags()
            .map(|value| value.value().trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect(),
    };

    let manifest_view = epub.manifest();
    let mut manifest = Vec::with_capacity(manifest_view.len());
    for entry in manifest_view.iter() {
        let resource_id = resource_id(entry.href().path().decode().as_ref())?;
        if !safe_archive.contains(&SafeArchivePath::parse(&resource_id)?) {
            return Err(AppError::validation(
                "MISSING_MANIFEST_RESOURCE",
                "EPUB 清单引用了不存在的资源。",
                "请选择结构完整的 EPUB 文件。",
            ));
        }
        manifest.push(ManifestItem {
            id: entry.id().to_owned(),
            resource_id,
            media_type: entry.media_type().to_owned(),
            properties: entry.properties().iter().map(str::to_owned).collect(),
        });
    }

    let mut spine = Vec::with_capacity(epub.spine().len());
    for entry in epub.spine().iter() {
        let manifest_entry = entry.manifest_entry().ok_or_else(invalid_spine)?;
        spine.push(SpineItem {
            index: entry.order(),
            idref: entry.idref().to_owned(),
            resource_id: resource_id(manifest_entry.href().path().decode().as_ref())?,
            media_type: manifest_entry.media_type().to_owned(),
            linear: entry.is_linear(),
            properties: entry.properties().iter().map(str::to_owned).collect(),
        });
    }
    if spine.is_empty() {
        return Err(invalid_spine());
    }

    let toc = epub
        .toc()
        .contents()
        .map(|root| {
            root.iter()
                .enumerate()
                .map(|(index, entry)| toc_node(entry, 1, index))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_else(|| {
            spine
                .iter()
                .map(|item| TocNode {
                    id: format!("spine-{}", item.index),
                    label: format!("第 {} 章", item.index + 1),
                    resource_id: Some(item.resource_id.clone()),
                    fragment: None,
                    children: Vec::new(),
                })
                .collect()
        });

    let cover_resource_id = manifest_view
        .cover_image()
        .map(|entry| resource_id(entry.href().path().decode().as_ref()))
        .transpose()?;

    let publication_id = metadata
        .identifier
        .clone()
        .unwrap_or_else(|| epub.package().unique_identifier().to_owned());
    let package_resource = resource_id(epub.package().location().decode().as_ref())?;
    let package_bytes = safe_archive.read(
        &SafeArchivePath::parse(&package_resource)?,
        ResourceClass::Xml,
    )?;
    let package_source = String::from_utf8(package_bytes).map_err(|_| invalid_epub())?;
    let layout = detect_layout(&package_source, &spine);

    Ok(ParsedEpubDocument {
        kind: DocumentKind::Epub,
        publication_id,
        version: version.to_owned(),
        metadata,
        cover_resource_id,
        manifest,
        spine,
        toc,
        layout,
        capabilities: DocumentCapabilities::epub(),
    })
}

fn detect_layout(package_source: &str, spine: &[SpineItem]) -> EpubLayout {
    let package = package_source.to_ascii_lowercase();
    let package_fixed = package.contains("rendition:layout")
        && (package.contains("pre-paginated") || package.contains("fixed"));
    let spine_fixed = spine.iter().any(|item| {
        item.properties.iter().any(|property| {
            let property = property.to_ascii_lowercase();
            property.contains("pre-paginated") || property.contains("layout-fixed")
        })
    });
    if package_fixed || spine_fixed {
        EpubLayout::Fixed
    } else {
        EpubLayout::Reflowable
    }
}

fn toc_node(entry: EpubTocEntry<'_>, depth: usize, sibling: usize) -> Result<TocNode, AppError> {
    if depth > 32 {
        return Err(AppError::validation(
            "INVALID_NAVIGATION",
            "EPUB 目录嵌套过深。",
            "请选择目录结构正常的 EPUB 文件。",
        ));
    }
    let href = entry.href();
    let resource = href
        .map(|value| resource_id(value.path().decode().as_ref()))
        .transpose()?;
    let fragment = href.and_then(|value| value.fragment()).map(str::to_owned);
    let children = entry
        .iter()
        .enumerate()
        .map(|(index, child)| toc_node(child, depth + 1, index))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TocNode {
        id: entry
            .id()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("toc-{depth}-{sibling}")),
        label: entry.label().trim().to_owned(),
        resource_id: resource,
        fragment,
        children,
    })
}

fn resource_id(value: &str) -> Result<String, AppError> {
    let normalized = value.strip_prefix('/').unwrap_or(value);
    SafeArchivePath::parse(normalized).map(|path| path.as_str().to_owned())
}

fn invalid_epub() -> AppError {
    AppError::validation(
        "INVALID_EPUB",
        "无法解析 EPUB 的核心结构。",
        "请选择有效且未加密的 EPUB 2 或 EPUB 3 文件。",
    )
}

fn missing_container() -> AppError {
    AppError::validation(
        "MISSING_CONTAINER",
        "EPUB 缺少 META-INF/container.xml。",
        "请选择结构完整的 EPUB 文件。",
    )
}

fn invalid_spine() -> AppError {
    AppError::validation(
        "INVALID_SPINE",
        "EPUB 没有有效的阅读顺序。",
        "请选择包含有效 spine 的 EPUB 文件。",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epub_test_fixtures::{
        epub_without_container, minimal_epub2, minimal_epub3, unsupported_epub_version,
    };

    #[test]
    fn parses_epub3_metadata_manifest_and_spine() {
        let fixture = minimal_epub3();

        let parsed = parse_epub_document(fixture.path(), ArchiveLimits::default())
            .expect("minimal EPUB 3 should open");

        assert_eq!(parsed.metadata.title, "阅织 EPUB 3 测试");
        assert_eq!(parsed.metadata.creators, ["Readloom 测试作者"]);
        assert_eq!(parsed.publication_id, "urn:readloom:test:epub3");
        assert_eq!(parsed.spine.len(), 1);
        assert_eq!(parsed.spine[0].resource_id, "EPUB/chapter.xhtml");
        assert!(parsed.spine[0].linear);
        assert!(parsed.capabilities.can_read);
        assert!(!parsed.capabilities.can_edit_text);
    }

    #[test]
    fn parses_epub2_ncx_cover_metadata_and_spine() {
        let fixture = minimal_epub2();

        let parsed = parse_epub_document(fixture.path(), ArchiveLimits::default())
            .expect("minimal EPUB 2 should open");

        assert_eq!(parsed.version, "2.0");
        assert_eq!(parsed.metadata.title, "阅织 EPUB 2 测试");
        assert_eq!(parsed.metadata.creators, ["第二版作者"]);
        assert_eq!(
            parsed.cover_resource_id.as_deref(),
            Some("OEBPS/images/cover.png")
        );
        assert_eq!(parsed.toc[0].label, "旧版第一章");
        assert_eq!(parsed.toc[0].fragment.as_deref(), Some("start"));
        assert_eq!(parsed.spine[0].resource_id, "OEBPS/text/chapter.xhtml");
    }

    #[test]
    fn recognizes_fixed_layout_metadata_without_claiming_full_support() {
        assert_eq!(
            detect_layout(
                r#"<meta property="rendition:layout">pre-paginated</meta>"#,
                &[],
            ),
            EpubLayout::Fixed,
        );
        assert_eq!(detect_layout("<package/>", &[]), EpubLayout::Reflowable);
    }

    #[test]
    fn reports_a_missing_container_with_a_stable_error_code() {
        let fixture = epub_without_container();

        let error = parse_epub_document(fixture.path(), ArchiveLimits::default())
            .expect_err("container.xml is required by EPUB");

        assert_eq!(error.to_dto().code, "MISSING_CONTAINER");
    }

    #[test]
    fn rejects_versions_outside_epub2_and_epub3() {
        let fixture = unsupported_epub_version();

        let error = parse_epub_document(fixture.path(), ArchiveLimits::default())
            .expect_err("EPUB 4 is outside the supported compatibility contract");

        assert_eq!(error.to_dto().code, "UNSUPPORTED_EPUB_VERSION");
    }
}
