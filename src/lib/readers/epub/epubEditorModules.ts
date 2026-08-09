let modulePromise: Promise<Awaited<ReturnType<typeof importEditorModules>>> | null = null;

async function importEditorModules() {
  const [
    core,
    document,
    text,
    paragraph,
    heading,
    hardBreak,
    bold,
    italic,
    strike,
    underline,
    blockquote,
    lists,
    horizontalRule,
    link,
    image,
    textAlign,
    subscript,
    superscript,
    extensions,
  ] = await Promise.all([
    import('@tiptap/core'),
    import('@tiptap/extension-document'),
    import('@tiptap/extension-text'),
    import('@tiptap/extension-paragraph'),
    import('@tiptap/extension-heading'),
    import('@tiptap/extension-hard-break'),
    import('@tiptap/extension-bold'),
    import('@tiptap/extension-italic'),
    import('@tiptap/extension-strike'),
    import('@tiptap/extension-underline'),
    import('@tiptap/extension-blockquote'),
    import('@tiptap/extension-list'),
    import('@tiptap/extension-horizontal-rule'),
    import('@tiptap/extension-link'),
    import('@tiptap/extension-image'),
    import('@tiptap/extension-text-align'),
    import('@tiptap/extension-subscript'),
    import('@tiptap/extension-superscript'),
    import('@tiptap/extensions/undo-redo'),
  ]);
  return {
    Editor: core.Editor,
    Extension: core.Extension,
    Mark: core.Mark,
    Document: document.default,
    Text: text.default,
    Paragraph: paragraph.default,
    Heading: heading.default,
    HardBreak: hardBreak.default,
    Bold: bold.default,
    Italic: italic.default,
    Strike: strike.default,
    Underline: underline.default,
    Blockquote: blockquote.default,
    BulletList: lists.BulletList,
    OrderedList: lists.OrderedList,
    ListItem: lists.ListItem,
    ListKeymap: lists.ListKeymap,
    HorizontalRule: horizontalRule.default,
    Link: link.default,
    Image: image.default,
    TextAlign: textAlign.default,
    Subscript: subscript.default,
    Superscript: superscript.default,
    UndoRedo: extensions.UndoRedo,
  };
}

export function loadEpubEditorModules() {
  modulePromise ??= importEditorModules();
  return modulePromise;
}

export function editorModulesLoadedForTest(): boolean {
  return modulePromise !== null;
}
