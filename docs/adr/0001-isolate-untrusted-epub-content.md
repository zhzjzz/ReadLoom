# ADR 0001: Isolate untrusted EPUB content

## Status

Accepted and validated on Windows WebView2 release build.

## Context

EPUB XHTML, CSS, SVG, images, and fonts are publisher-controlled input. Rendering that input in the application DOM or exposing the original file through `file://` would give publication content unnecessary access to Readloom's UI and local environment. Sending every resource through Base64 IPC would avoid `file://`, but would create large copies and a broad command interface.

## Decision

Readloom will validate EPUB archives in Rust and expose only manifest-approved resources through a session-scoped custom protocol. Sanitized chapters will render in an iframe without `allow-same-origin`. EPUB scripts are removed. A strict response CSP allows only resources from the active EPUB session and an optional fixed-hash Readloom bridge script.

The protocol handler is an authorization seam: it validates the WebView label, unguessable session ID, canonical resource path, manifest membership, MIME class, and resource limits for every request. Closing a document invalidates its session before best-effort cleanup.

## Consequences

- Publication content cannot directly share the application DOM or Tauri interface.
- Relative CSS, images, fonts, and fragments can load without exposing a filesystem path.
- Windows WebView2 uses Tauri's mapped `http://readloom-epub.localhost` origin; release validation covers CSP, sandbox OOPIF rendering, internal resources, script removal, zero external resource requests, and session shutdown.
- Compatibility is intentionally reduced for scripted, encrypted, DRM-protected, malformed, or externally hosted publications.
- The application must keep protocol responses and caches bounded and must never extract an entire publication by default.
