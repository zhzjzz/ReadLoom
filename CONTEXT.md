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
