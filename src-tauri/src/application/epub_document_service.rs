use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    domain::epub_document::{
        EpubLocator, EpubSearchRequest, EpubSearchResult, OpenedEpubDocument, ParsedEpubDocument,
        TocNode,
    },
    error::AppError,
    formats::epub::parser::parse_epub_document,
    infrastructure::{
        archive::{
            archive_limits::ArchiveLimits,
            safe_zip::{ResourceClass, SafeArchivePath, SafeEpubArchive},
        },
        filesystem::fingerprint_file,
    },
    security::epub_content::{
        extract_visible_text, sanitize_css, sanitize_svg, sanitize_xhtml_fragment,
    },
};
use base64::Engine as _;
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub(crate) struct EpubDocumentService {
    limits: ArchiveLimits,
    next_document_id: Arc<AtomicU64>,
    sessions: Arc<Mutex<HashMap<String, EpubSession>>>,
    search_requests: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Clone)]
struct EpubSession {
    path: PathBuf,
    opened: OpenedEpubDocument,
    #[allow(dead_code)]
    archive: SafeEpubArchive,
    #[allow(dead_code)]
    parsed: ParsedEpubDocument,
}

#[derive(Debug, Clone)]
pub(crate) struct EpubSessionContext {
    pub path: PathBuf,
    pub document_id: String,
    pub file_fingerprint: String,
    pub parsed: ParsedEpubDocument,
}

#[derive(Debug)]
pub(crate) struct EpubResourceResponse {
    pub body: Vec<u8>,
    pub content_type: String,
    pub content_security_policy: Option<String>,
}

impl EpubDocumentService {
    pub(crate) fn new(limits: ArchiveLimits) -> Self {
        Self {
            limits,
            next_document_id: Arc::new(AtomicU64::new(1)),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            search_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn open(&self, path: &Path) -> Result<OpenedEpubDocument, AppError> {
        validate_epub_extension(path)?;
        let canonical_path = fs::canonicalize(path).map_err(|_| invalid_epub_path())?;
        let metadata = fs::metadata(&canonical_path).map_err(|_| invalid_epub_path())?;
        if !metadata.is_file() {
            return Err(invalid_epub_path());
        }

        if let Some(opened) = self
            .sessions
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock EPUB sessions"))?
            .values()
            .find(|session| session.path == canonical_path)
            .map(|session| session.opened.clone())
        {
            return Ok(opened);
        }

        let archive = SafeEpubArchive::open(&canonical_path, self.limits)?;
        let parsed = parse_epub_document(&canonical_path, self.limits)?;
        let fingerprint = fingerprint_file(&canonical_path)
            .map_err(|_| AppError::internal("INTERNAL", "fingerprint EPUB"))?;
        let file_name = canonical_path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .ok_or_else(invalid_epub_path)?;
        let document_number = self.next_document_id.fetch_add(1, Ordering::Relaxed);
        let opened = OpenedEpubDocument {
            document_id: format!("epub-{document_number:016x}"),
            session_id: random_token()?,
            bridge_token: random_token()?,
            file_name,
            display_path: canonical_path.display().to_string(),
            file_fingerprint: fingerprint.blake3,
            document: parsed.clone(),
            initial_locator: None,
            bookmarks: Vec::new(),
        };
        self.sessions
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock EPUB sessions"))?
            .insert(
                opened.document_id.clone(),
                EpubSession {
                    path: canonical_path,
                    opened: opened.clone(),
                    archive,
                    parsed,
                },
            );
        Ok(opened)
    }

    pub(crate) fn resource(
        &self,
        session_id: &str,
        resource_id: &str,
    ) -> Result<EpubResourceResponse, AppError> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock EPUB sessions"))?;
        let session = sessions
            .values()
            .find(|session| session.opened.session_id == session_id)
            .ok_or_else(epub_session_expired)?;
        let manifest_item = session
            .parsed
            .manifest
            .iter()
            .find(|item| item.resource_id == resource_id)
            .ok_or_else(resource_blocked)?;
        let path = SafeArchivePath::parse(resource_id)?;
        let class = resource_class(&manifest_item.media_type)?;
        let body = session.archive.read(&path, class)?;
        validate_resource_body(&manifest_item.media_type, &body)?;

        if class == ResourceClass::Xhtml {
            let source = String::from_utf8(body).map_err(|_| {
                AppError::validation(
                    "UNSAFE_XHTML",
                    "EPUB 章节不是有效的 UTF-8 XHTML。",
                    "请选择编码正确的 EPUB 文件。",
                )
            })?;
            let fragment = sanitize_xhtml_fragment(&source, resource_id, session_id)?;
            let script = bridge_script(
                &session.opened.document_id,
                &session.opened.session_id,
                &session.opened.bridge_token,
            );
            let document = wrap_xhtml(&fragment, &script);
            return Ok(EpubResourceResponse {
                body: document.into_bytes(),
                content_type: "text/html; charset=utf-8".to_owned(),
                content_security_policy: Some(epub_csp(&script)),
            });
        }

        if class == ResourceClass::Css {
            let source = String::from_utf8(body).map_err(|_| mime_mismatch())?;
            let cleaned = sanitize_css(&source, resource_id, session_id)?;
            return Ok(EpubResourceResponse {
                body: cleaned.into_bytes(),
                content_type: "text/css; charset=utf-8".to_owned(),
                content_security_policy: None,
            });
        }

        if manifest_item.media_type == "image/svg+xml" {
            let source = String::from_utf8(body).map_err(|_| mime_mismatch())?;
            let cleaned = sanitize_svg(&source, resource_id, session_id)?;
            return Ok(EpubResourceResponse {
                body: cleaned.into_bytes(),
                content_type: "image/svg+xml; charset=utf-8".to_owned(),
                content_security_policy: Some(epub_asset_csp().to_owned()),
            });
        }

        Ok(EpubResourceResponse {
            body,
            content_type: manifest_item.media_type.clone(),
            content_security_policy: None,
        })
    }

    pub(crate) fn close(&self, document_id: &str) -> Result<(), AppError> {
        let removed = self
            .sessions
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock EPUB sessions"))?
            .remove(document_id);
        if removed.is_none() {
            return Err(epub_session_expired());
        }
        self.search_requests
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock EPUB search requests"))?
            .remove(document_id);
        Ok(())
    }

    pub(crate) fn session_context(
        &self,
        document_id: &str,
    ) -> Result<EpubSessionContext, AppError> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock EPUB sessions"))?;
        let session = sessions.get(document_id).ok_or_else(epub_session_expired)?;
        Ok(EpubSessionContext {
            path: session.path.clone(),
            document_id: session.opened.document_id.clone(),
            file_fingerprint: session.opened.file_fingerprint.clone(),
            parsed: session.parsed.clone(),
        })
    }

    pub(crate) fn validate_locator(
        &self,
        mut locator: EpubLocator,
    ) -> Result<(PathBuf, EpubLocator), AppError> {
        let context = self.session_context(&locator.document_id)?;
        if locator.document_fingerprint != context.file_fingerprint {
            return Err(invalid_locator());
        }
        let resolved_index = context
            .parsed
            .spine
            .iter()
            .position(|item| item.resource_id == locator.spine_href)
            .or_else(|| {
                (locator.spine_index < context.parsed.spine.len()).then_some(locator.spine_index)
            })
            .ok_or_else(invalid_locator)?;
        locator.spine_index = resolved_index;
        locator.spine_href = context.parsed.spine[resolved_index].resource_id.clone();
        locator = locator.normalized();
        Ok((context.path, locator))
    }

    pub(crate) fn search(
        &self,
        request: EpubSearchRequest,
    ) -> Result<Vec<EpubSearchResult>, AppError> {
        let query = request.query.trim();
        if query.is_empty() || query.chars().count() > 256 || request.request_id.len() > 128 {
            return Err(AppError::validation(
                "INVALID_SEARCH_QUERY",
                "搜索词为空或过长。",
                "请输入 1 到 256 个字符后重试。",
            ));
        }
        let maximum_results = request.maximum_results.unwrap_or(100).clamp(1, 500);
        self.search_requests
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock EPUB search requests"))?
            .insert(request.document_id.clone(), request.request_id.clone());

        let snapshot = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| AppError::internal("INTERNAL", "lock EPUB sessions"))?;
            sessions
                .get(&request.document_id)
                .cloned()
                .ok_or_else(epub_session_expired)?
        };

        let mut results = Vec::new();
        for spine in snapshot.parsed.spine.iter().filter(|item| item.linear) {
            self.ensure_search_active(&request.document_id, &request.request_id)?;
            let path = SafeArchivePath::parse(&spine.resource_id)?;
            let body = snapshot.archive.read(&path, ResourceClass::Xhtml)?;
            let source = String::from_utf8(body).map_err(|_| mime_mismatch())?;
            let visible_text = extract_visible_text(&source)?;
            let chapter_title = toc_label(&snapshot.parsed.toc, &spine.resource_id)
                .unwrap_or_else(|| format!("第 {} 章", spine.index + 1));
            append_search_results(
                &mut results,
                &request,
                spine.index,
                &spine.resource_id,
                &chapter_title,
                &visible_text,
                maximum_results,
            );
            if results.len() >= maximum_results {
                break;
            }
        }
        self.ensure_search_active(&request.document_id, &request.request_id)?;
        Ok(results)
    }

    pub(crate) fn cancel_search(&self, document_id: &str, request_id: &str) {
        if let Ok(mut searches) = self.search_requests.lock()
            && searches
                .get(document_id)
                .is_some_and(|active| active == request_id)
        {
            searches.remove(document_id);
        }
    }

    fn ensure_search_active(&self, document_id: &str, request_id: &str) -> Result<(), AppError> {
        let is_active = self
            .search_requests
            .lock()
            .map_err(|_| AppError::internal("INTERNAL", "lock EPUB search requests"))?
            .get(document_id)
            .is_some_and(|active| active == request_id);
        if is_active {
            Ok(())
        } else {
            Err(search_cancelled())
        }
    }
}

fn toc_label(nodes: &[TocNode], resource_id: &str) -> Option<String> {
    for node in nodes {
        if node.resource_id.as_deref() == Some(resource_id) && !node.label.trim().is_empty() {
            return Some(node.label.clone());
        }
        if let Some(label) = toc_label(&node.children, resource_id) {
            return Some(label);
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn append_search_results(
    results: &mut Vec<EpubSearchResult>,
    request: &EpubSearchRequest,
    spine_index: usize,
    spine_href: &str,
    chapter_title: &str,
    text: &str,
    maximum_results: usize,
) {
    let searchable = if request.case_sensitive {
        text.to_owned()
    } else {
        text.to_lowercase()
    };
    let needle = if request.case_sensitive {
        request.query.trim().to_owned()
    } else {
        request.query.trim().to_lowercase()
    };
    let original_characters = text.chars().collect::<Vec<_>>();
    for (byte_offset, matched) in searchable.match_indices(&needle) {
        if results.len() >= maximum_results {
            break;
        }
        let character_offset = searchable[..byte_offset]
            .chars()
            .count()
            .min(original_characters.len());
        let match_characters = matched
            .chars()
            .count()
            .min(original_characters.len().saturating_sub(character_offset));
        if match_characters == 0 {
            continue;
        }
        if request.whole_word
            && !is_whole_word(&original_characters, character_offset, match_characters)
        {
            continue;
        }
        let snippet_start = character_offset.saturating_sub(36);
        let snippet_end = (character_offset + match_characters + 60).min(original_characters.len());
        let temporary_snippet = original_characters[snippet_start..snippet_end]
            .iter()
            .collect::<String>();
        results.push(EpubSearchResult {
            request_id: request.request_id.clone(),
            spine_index,
            spine_href: spine_href.to_owned(),
            chapter_title: chapter_title.to_owned(),
            character_offset,
            temporary_snippet,
            match_start: character_offset - snippet_start,
            match_end: character_offset - snippet_start + match_characters,
        });
    }
}

fn is_whole_word(characters: &[char], start: usize, length: usize) -> bool {
    let before = start.checked_sub(1).and_then(|index| characters.get(index));
    let after = characters.get(start.saturating_add(length));
    before.is_none_or(|value| !value.is_alphanumeric() && *value != '_')
        && after.is_none_or(|value| !value.is_alphanumeric() && *value != '_')
}

fn resource_class(media_type: &str) -> Result<ResourceClass, AppError> {
    match media_type {
        "application/xhtml+xml" | "text/html" => Ok(ResourceClass::Xhtml),
        "text/css" => Ok(ResourceClass::Css),
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/svg+xml" => {
            Ok(ResourceClass::Image)
        }
        "font/ttf"
        | "font/otf"
        | "font/woff"
        | "font/woff2"
        | "application/vnd.ms-opentype"
        | "application/font-sfnt" => Ok(ResourceClass::Font),
        _ => Err(resource_blocked()),
    }
}

fn validate_resource_body(media_type: &str, body: &[u8]) -> Result<(), AppError> {
    let matches = match media_type {
        "application/xhtml+xml" | "text/html" | "text/css" => std::str::from_utf8(body).is_ok(),
        "image/png" => body.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => body.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => body.starts_with(b"GIF87a") || body.starts_with(b"GIF89a"),
        "image/webp" => body.len() >= 12 && body.starts_with(b"RIFF") && &body[8..12] == b"WEBP",
        "image/svg+xml" => std::str::from_utf8(body)
            .ok()
            .is_some_and(|source| source.to_ascii_lowercase().contains("<svg")),
        "font/woff" => body.starts_with(b"wOFF"),
        "font/woff2" => body.starts_with(b"wOF2"),
        "font/otf" => body.starts_with(b"OTTO"),
        "font/ttf" | "application/vnd.ms-opentype" | "application/font-sfnt" => {
            body.starts_with(b"\0\x01\0\0")
                || body.starts_with(b"true")
                || body.starts_with(b"typ1")
                || body.starts_with(b"OTTO")
        }
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(mime_mismatch())
    }
}

fn mime_mismatch() -> AppError {
    AppError::validation(
        "UNSUPPORTED_MEDIA_TYPE",
        "EPUB 资源内容与声明的媒体类型不一致。",
        "该资源已被阻止，正文仍可继续阅读。",
    )
}

fn wrap_xhtml(fragment: &str, bridge_script: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><style>html{{color-scheme:light dark}}body{{box-sizing:border-box;margin:0 auto;max-width:var(--r-width,52rem);padding:var(--r-margin,48px);font-family:var(--r-font,system-ui,sans-serif);font-size:var(--r-size,18px);line-height:var(--r-line,1.8);text-align:var(--r-align,start);overflow-wrap:anywhere}}img,svg{{height:auto;max-width:var(--r-image,100%)}}a{{color:#2855b8}}@media(prefers-color-scheme:dark){{body{{background:#17191d;color:#e7e9ee}}a{{color:#9db8ff}}}}</style></head><body>{fragment}<script>{bridge_script}</script></body></html>"
    )
}

fn bridge_script(document_id: &str, session_id: &str, token: &str) -> String {
    format!(
        r#"(()=>{{"use strict";const c={{documentId:"{document_id}",sessionId:"{session_id}",token:"{token}"}};let last=0;const send=(type,payload)=>parent.postMessage({{source:"readloom-epub",version:1,type,...c,payload}},"*");const progress=()=>{{const now=Date.now();if(now-last<400)return;last=now;const root=document.documentElement;const span=Math.max(1,root.scrollHeight-innerHeight);send("progress",{{progression:Math.max(0,Math.min(1,scrollY/span)),fragment:location.hash.slice(1,257)||null}})}};const finite=(value,min,max,fallback)=>typeof value==="number"&&Number.isFinite(value)?Math.max(min,Math.min(max,value)):fallback;addEventListener("message",event=>{{const data=event.data;if(!data||data.source!=="readloom-host"||data.version!==1||data.documentId!==c.documentId||data.sessionId!==c.sessionId||data.token!==c.token)return;if(data.type==="restore"){{const value=finite(data.payload?.progression,0,1,0);requestAnimationFrame(()=>scrollTo(0,value*Math.max(0,document.documentElement.scrollHeight-innerHeight)));return}}if(data.type!=="settings")return;const p=data.payload||{{}};const root=document.documentElement;const family=p.fontFamily==="serif"?"Georgia,'Noto Serif SC',serif":p.fontFamily==="sans"?"system-ui,'Microsoft YaHei UI',sans-serif":"system-ui,'Microsoft YaHei UI',sans-serif";root.style.setProperty("--r-font",family);root.style.setProperty("--r-size",finite(p.fontSize,12,32,18)+"px");root.style.setProperty("--r-line",finite(p.lineHeight,1.2,2.4,1.8));root.style.setProperty("--r-width",finite(p.contentWidth,480,1200,832)+"px");root.style.setProperty("--r-margin",finite(p.pageMargin,8,96,48)+"px");root.style.setProperty("--r-image",finite(p.imageMaximumWidth,50,100,100)+"%");root.style.setProperty("--r-align",p.textAlign==="justify"?"justify":"start");const override=document.getElementById("readloom-overrides")||document.head.appendChild(Object.assign(document.createElement("style"),{{id:"readloom-overrides"}}));override.textContent=(p.publisherStyles==="ignore"?"link[rel~='stylesheet']{{display:none}}":"")+(p.ignorePublisherFonts||!p.allowInternalFonts?"*{{font-family:var(--r-font)!important}}":"")+(p.ignorePublisherColors?"body,*{{color:inherit!important;background-color:transparent!important}}":"")}});addEventListener("scroll",progress,{{passive:true}});addEventListener("load",progress);document.addEventListener("click",event=>{{const target=event.target;const anchor=target instanceof Element?target.closest("a[href]"):null;if(!anchor)return;const href=anchor.getAttribute("href");if(!href||href.length>2048)return;event.preventDefault();send("link",{{href}})}})}})();"#
    )
}

fn epub_csp(script: &str) -> String {
    let digest = Sha256::digest(script.as_bytes());
    let encoded = base64::engine::general_purpose::STANDARD.encode(digest);
    format!(
        "default-src 'none'; base-uri 'none'; object-src 'none'; frame-src 'none'; connect-src 'none'; media-src 'none'; worker-src 'none'; form-action 'none'; img-src 'self'; font-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'sha256-{encoded}'"
    )
}

fn epub_asset_csp() -> &'static str {
    "default-src 'none'; base-uri 'none'; object-src 'none'; frame-src 'none'; connect-src 'none'; script-src 'none'; style-src 'none'; img-src 'self'"
}

fn random_token() -> Result<String, AppError> {
    let mut bytes = [0_u8; 24];
    getrandom::fill(&mut bytes)
        .map_err(|_| AppError::internal("INTERNAL", "generate EPUB session token"))?;
    let mut token = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        token.push(HEX[(byte >> 4) as usize] as char);
        token.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(token)
}

fn validate_epub_extension(path: &Path) -> Result<(), AppError> {
    let is_epub = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("epub"));
    if is_epub {
        Ok(())
    } else {
        Err(invalid_epub_path())
    }
}

fn invalid_epub_path() -> AppError {
    AppError::validation(
        "INVALID_EPUB",
        "所选路径不是有效的 EPUB 文件。",
        "请通过文件选择器选择 .epub 文件。",
    )
}

fn epub_session_expired() -> AppError {
    AppError::validation(
        "EPUB_SESSION_EXPIRED",
        "EPUB 阅读会话已经关闭或失效。",
        "请重新打开这本 EPUB。",
    )
}

fn resource_blocked() -> AppError {
    AppError::validation(
        "RESOURCE_BLOCKED",
        "EPUB 资源未在安全清单中或类型不受支持。",
        "返回目录并选择其他章节。",
    )
}

fn invalid_locator() -> AppError {
    AppError::validation(
        "INVALID_INTERNAL_LINK",
        "EPUB 阅读位置已经失效。",
        "Readloom 将回到可用章节。",
    )
}

fn search_cancelled() -> AppError {
    AppError::validation(
        "SEARCH_CANCELLED",
        "EPUB 搜索已取消。",
        "输入新的搜索词即可重新搜索。",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epub_test_fixtures::minimal_epub3;

    #[test]
    fn opening_an_epub_creates_an_unguessable_read_only_session() {
        let fixture = minimal_epub3();
        let service = EpubDocumentService::new(ArchiveLimits::default());

        let opened = service.open(fixture.path()).expect("open EPUB session");

        assert!(opened.document_id.starts_with("epub-"));
        assert_eq!(opened.session_id.len(), 48);
        assert!(
            opened
                .session_id
                .chars()
                .all(|value| value.is_ascii_hexdigit())
        );
        assert_eq!(opened.document.metadata.title, "阅织 EPUB 3 测试");
        assert!(opened.document.capabilities.can_read);
        assert!(!opened.document.capabilities.can_save);
    }

    #[test]
    fn closing_an_epub_invalidates_its_resource_session() {
        let fixture = minimal_epub3();
        let service = EpubDocumentService::new(ArchiveLimits::default());
        let opened = service.open(fixture.path()).expect("open EPUB session");

        let resource = service
            .resource(&opened.session_id, "EPUB/chapter.xhtml")
            .expect("read active chapter");
        assert_eq!(resource.content_type, "text/html; charset=utf-8");
        let body = String::from_utf8(resource.body).unwrap();
        assert!(body.contains("你好，Readloom。"));
        assert!(body.contains("source:\"readloom-epub\""));
        let csp = resource.content_security_policy.expect("chapter CSP");
        assert!(csp.contains("script-src 'sha256-"));
        assert!(!csp.contains("'unsafe-eval'"));
        assert!(!csp.contains("script-src 'self'"));

        service
            .close(&opened.document_id)
            .expect("close EPUB session");
        let error = service
            .resource(&opened.session_id, "EPUB/chapter.xhtml")
            .expect_err("closed session resource must expire");
        assert_eq!(error.to_dto().code, "EPUB_SESSION_EXPIRED");
    }

    #[test]
    fn media_signatures_reject_extension_and_manifest_spoofing() {
        assert!(validate_resource_body("image/png", b"<script>alert(1)</script>").is_err());
        assert!(validate_resource_body("image/png", b"\x89PNG\r\n\x1a\nfixture").is_ok());
        assert!(validate_resource_body("font/woff2", b"not-a-font").is_err());
        assert!(validate_resource_body("font/woff2", b"wOF2fixture").is_ok());
    }

    #[test]
    fn searches_only_visible_spine_text_and_returns_temporary_snippets() {
        let fixture = minimal_epub3();
        let service = EpubDocumentService::new(ArchiveLimits::default());
        let opened = service.open(fixture.path()).expect("open EPUB session");

        let results = service
            .search(EpubSearchRequest {
                document_id: opened.document_id,
                request_id: "search-1".to_owned(),
                query: "Readloom".to_owned(),
                case_sensitive: false,
                whole_word: true,
                maximum_results: Some(10),
            })
            .expect("search EPUB");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].spine_href, "EPUB/chapter.xhtml");
        assert!(results[0].temporary_snippet.contains("你好，Readloom"));
        assert_eq!(results[0].request_id, "search-1");
    }
}
