export type TextEditorModules = Awaited<ReturnType<typeof importTextEditorModules>>;

let modulesPromise: Promise<TextEditorModules> | null = null;

export function loadTextEditorModules(): Promise<TextEditorModules> {
  modulesPromise ??= importTextEditorModules();
  return modulesPromise;
}

export function createLazyTextEditorLoader<T>(importer: () => Promise<T>) {
  let promise: Promise<T> | null = null;
  return {
    load(): Promise<T> {
      promise ??= importer();
      return promise;
    },
  };
}

async function importTextEditorModules() {
  const [state, view, commands, search] = await Promise.all([
    import('@codemirror/state'),
    import('@codemirror/view'),
    import('@codemirror/commands'),
    import('@codemirror/search'),
  ]);
  return { state, view, commands, search };
}
