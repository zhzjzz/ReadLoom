# Slint migration module seams

This inventory is the extraction map for the native desktop migration. The external seam is `ReadloomCore`; both presentation adapters call it, while format and persistence details remain local to its implementation.

| Existing area | Dependency category | Migration treatment |
| --- | --- | --- |
| `domain/*` | In-process | Move shared document, locator, publication, and draft language into `readloom-core` without UI DTOs. |
| `formats/txt/*` | In-process | Reuse decoding, line-ending, and encoding behavior; add native reading blocks, chapters, search, and locator resolution. |
| `formats/epub/*` | In-process plus bounded archive I/O | Move after the TXT slice; expose a closed `DocumentLayoutModel`, never publisher HTML/CSS. |
| `application/*` | In-process plus local-substitutable I/O | Deepen around document/session operations. UI adapters should not coordinate fingerprints, revisions, or safe saves. |
| `infrastructure/storage/local_state.rs` | Local-substitutable SQLite | Keep the existing schema compatible and test through the core interface with temporary SQLite files. |
| `infrastructure/filesystem/*` | Local-substitutable filesystem | Retain atomic save and fingerprint behavior behind core operations; tests use real temporary files. |
| `infrastructure/archive/*` | Local-substitutable archive/file I/O | Retain SafeZIP and archive limits inside the future EPUB core module. |
| `security/epub_content.rs` | In-process | Reuse sanitization when producing the native `DocumentLayoutModel`. |
| `commands/*` | Tauri adapter | Keep only as the legacy adapter; Slint calls Rust interfaces directly. |
| `security/*_protocol.rs` | Tauri/WebView adapter | Do not migrate. Replace URLs with validated bytes, images, or structured models passed in-process. |
| `lib.rs` window/tray/protocol setup | Tauri adapter | Preserve as the old executable baseline until final cut-over. |

## Current native vertical-slice interface

`ReadloomCore` provides a deliberately small interface:

1. Open the existing local-state database and return a library snapshot.
2. Open, search and locate TXT through `ReaderDocument`; save through `ReadloomCore::save_txt` or `save_txt_as`, which own encoding, line endings, file fingerprints and replacement safety.
3. Open validated EPUB 2/3 through `EpubDocument`; expose only headings, visible paragraphs, chapters, search hits and versioned locators.
4. Save backward-compatible TXT and EPUB reading locators without exposing SQLite.
5. Load, normalize and save the complete native `AppSettings` value through the existing `app_preferences` table.

The Slint adapter owns window lifecycle, native file dialogs and presentation models only. It never encodes TXT, opens ZIP entries, parses XHTML, compares fingerprints or writes SQL. Slint `ListView` is the visual virtualization adapter: estimated block heights seed long jumps, while visible row geometry reports the resolved first-visible paragraph back to the core locator.
