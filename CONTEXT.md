# Readloom Document Workspace

Readloom manages local reading and editing artifacts through a shared document workspace while keeping format-specific behavior explicit.

## Language

**Document**:
A local artifact that Readloom can open in the workspace, such as a TXT file or an EPUB publication.
_Avoid_: File, editor buffer

**Document Kind**:
The format family that determines a document's model and capabilities. The supported kinds are TXT and EPUB.
_Avoid_: Extension, mode

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
