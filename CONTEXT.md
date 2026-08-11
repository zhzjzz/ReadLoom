# Readloom Document Workspace

Readloom manages local reading and editing artifacts through a shared document workspace while keeping format-specific behavior explicit.

## Language

**Document**:
A local artifact that Readloom can open in the workspace, such as a TXT file or an EPUB publication.
_Avoid_: File, editor buffer

**Document Kind**:
The format family that determines a document's model and capabilities. The supported kinds are TXT and EPUB.
_Avoid_: Extension, mode

**Library Entry**:
A durable local collection record for a Document. Opening a document adds or refreshes its Library Entry, while removing the entry does not delete the source document or its Recent Document History.
_Avoid_: Recent file, open tab

**Library Cover Resource**:
An EPUB cover referenced by an opaque Library Entry key and served through Readloom's validated, read-only cover protocol. It never grants general local-file access.
_Avoid_: Local file URL, embedded full-library image data

**Library Group**:
A user-named shelf that optionally owns Library Entries for display and filtering. Deleting a Library Group returns its entries to the ungrouped shelf.
_Avoid_: Folder, filesystem directory

**Recent Document History**:
Internal recency metadata updated whenever a Document is opened. It is stored independently from Library Entries and is not presented as a removable navigation panel.
_Avoid_: Library, collection

**Document Capabilities**:
The format-independent description of which user operations a document supports, including reading, editing, saving, searching, chapters, and bookmarks.
_Avoid_: Feature flags, extension checks

**Document Session**:
The runtime association between an open document and the resources Readloom owns for it. A session ends when its tab closes or the application exits.
_Avoid_: File handle, tab

**EPUB Publication**:
A validated EPUB 2 or EPUB 3 publication represented by metadata, a manifest, a spine, and navigation.
_Avoid_: Book archive, ZIP file

**Publication Resource**:
A manifest-declared item inside an EPUB publication, identified only by its canonical archive path.
_Avoid_: Extracted file, local URL

**Spine Item**:
An ordered publication resource that participates in the EPUB reading sequence.
_Avoid_: Page, chapter file

**Locator**:
A versioned, format-specific position that Readloom can persist and later resolve against a document fingerprint.
_Avoid_: Scroll offset, page number

**EPUB Session ID**:
An unguessable runtime identifier that authorizes access to one open EPUB publication without revealing its filesystem path.
_Avoid_: Document path, archive name

**Publication Draft**:
A disposable, runtime-only set of proposed metadata, cover, chapter, and imported-resource changes associated with one open EPUB Document Session.
_Avoid_: Editable EPUB copy, working archive

**Chapter Edit Draft**:
A revisioned runtime overlay for one compatible spine item. It keeps the original XHTML, the last saved XHTML, the accepted safe editor document, and the normalized XHTML independently.
_Avoid_: `innerHTML`, mutable chapter file

**Safe Editor Document**:
The closed JSON vocabulary shared by Rust and the Tiptap/ProseMirror editor for supported block nodes, inline marks, links, and manifest-backed images. Rust is the authority for validation and serialization.
_Avoid_: Arbitrary HTML, browser DOM snapshot

**Chapter Compatibility Level**:
The explicit `full`, `limited`, `read-only`, or `unsupported` result of analyzing a spine item before editing. A lower level is a visible safety boundary, not a silent conversion.
_Avoid_: Best-effort editable flag, implicit fallback

**Modification Overlay**:
The minimal set of replacement OPF, cover, compatible chapter, or newly imported image resources applied while repackaging a Publication Draft over its original EPUB entries.
_Avoid_: Extracted publication, rewritten book tree

**TXT Heading**:
A line recognized from TXT chapter-title syntax and exposed as a navigable outline entry without modifying the document text.
_Avoid_: EPUB chapter, parsed document section

**Workspace Pane**:
A collapsible and resizable supporting region beside the document body, such as navigation or document information.
_Avoid_: Fixed sidebar, document content

**TXT Reading Locator**:
A persisted first-visible character offset and line number used to restore a TXT Document near the last reading position. It is clamped when the source text becomes shorter.
_Avoid_: Cursor selection, bookmark

**Application Background**:
A user-selected PNG, JPEG, or WebP copied into Readloom's application-data directory and served only by opaque key through a validated read-only image protocol.
_Avoid_: Local file URL, arbitrary CSS URL

**Window Close Action**:
The user preference that chooses whether a clean close request exits Readloom or hides the main window while keeping it available from the system tray. Unsaved-change safeguards remain authoritative.
_Avoid_: Forced process exit, background service

**Reading Typography**:
The shared presentation settings for font fallback, relative paragraph spacing, content width, margins, alignment, and columns. TXT structure preprocessing and individual EPUB body-style overrides remain separate options beneath this model.
_Avoid_: Whole-document CSS replacement, page-number state

**TXT Reading Paragraph**:
A display-layer paragraph recognized from source blank lines, headings, and optionally conservative hard-wrap merging. It retains source character offsets so formatting changes do not replace the stable reading locator.
_Avoid_: Treating every newline as a paragraph, destructive text cleanup

**TXT Reading Window**:
The bounded set of at most 600 TXT Reading Paragraphs currently represented in the DOM. Estimated block offsets preserve the full scroll range, while binary source/scroll lookup moves the window for reading, search, and locator restoration. After each window move, rendered block geometry corrects estimation drift so the viewport cannot remain inside an empty spacer.
_Avoid_: Rendering the full TXT as DOM, querying every paragraph during scroll

**Library Import Preview**:
A read-only scan result shown before any library mutation, including each supported document's name, kind, byte size, canonical location, and whether it already belongs to the Library. Only the user's checked importable candidates become Library Entries.
_Avoid_: Import-on-directory-selection, hidden bulk mutation

**Content Backup**:
A versioned `.readloom-backup` archive containing deduplicated TXT and EPUB bytes plus a minimal integrity manifest. It intentionally excludes source paths, bookmarks, reading locators, groups, settings, and history.
_Avoid_: Full application backup, state synchronization
