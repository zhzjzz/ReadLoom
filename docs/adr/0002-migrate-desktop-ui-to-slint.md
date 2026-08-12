# ADR 0002: Migrate the desktop UI to Slint

Readloom will replace its Tauri/Svelte/WebView2 presentation adapter with a native Slint adapter while retaining the current application as a regression baseline until feature parity is demonstrated. Framework-independent format, storage, safety, and document behavior will move behind a `readloom-core` interface; Tauri commands and custom protocols remain legacy adapters. The first accepted slice is library → native TXT reading → search/navigation → compatible reading-locator persistence, and it must start without a `msedgewebview2.exe` descendant. Native EPUB support will consume a closed `Document Layout Model` produced by Rust rather than interpreting arbitrary publisher HTML or CSS.

## Consequences

- The old Tauri/Svelte application remains buildable during migration.
- TXT locators retain legacy absolute character and line fields while adding chapter and paragraph anchors.
- EPUB read-only rendering precedes structured editing; unsupported round trips stay explicitly read-only.
- Slint is pinned to the newest release compatible with the repository's declared Rust 1.88 baseline until that baseline is intentionally raised.
