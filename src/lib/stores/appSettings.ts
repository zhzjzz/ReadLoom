import type {
  AppSettings,
  EpubTypographySettings,
  LibraryColumns,
  ReadingColumns,
  ReadingFont,
  ReadingTextAlign,
  ReadingTypographySettings,
  ShortcutActionId,
  ShortcutSettings,
  TxtBlankLines,
  TxtChapterTitleStyle,
  TxtLeadingIndent,
} from '../types/settings';

const STORAGE_KEY = 'readloom.app-settings.v2';
const LEGACY_STORAGE_KEY = 'readloom.app-settings.v1';

export const shortcutActionIds: readonly ShortcutActionId[] = [
  'open',
  'save',
  'saveAs',
  'close',
  'toggleEdit',
  'previousChapter',
  'nextChapter',
  'bookmark',
  'showLibrary',
  'showSettings',
] as const;

export const defaultReadingTypographySettings: ReadingTypographySettings = {
  fontFamily: 'source-han-serif',
  fontSize: 19,
  fontWeight: 400,
  letterSpacing: 0,
  firstLineIndent: 2,
  lineHeight: 1.7,
  paragraphSpacing: 0.15,
  textAlign: 'justify',
  contentWidth: 780,
  horizontalMargin: 40,
  verticalMargin: 30,
  columns: 1,
  txt: {
    leadingIndent: 'clean',
    blankLines: 'single',
    mergeWrappedLines: false,
    chapterTitleStyle: 'prominent',
  },
  epub: {
    usePublisherStyles: true,
    overrideFont: false,
    overrideFontSize: true,
    overrideIndent: false,
    overrideLineHeight: true,
    overrideParagraphSpacing: false,
    useEmbeddedFonts: true,
  },
};

export const defaultShortcutSettings: ShortcutSettings = shortcutActionIds.reduce(
  (shortcuts, action) => ({ ...shortcuts, [action]: null }),
  {} as ShortcutSettings,
);

export const defaultAppSettings: AppSettings = {
  libraryColumns: 4,
  backgroundOpacity: 0.2,
  minimizeToTray: false,
  closeAction: 'exit',
  reading: defaultReadingTypographySettings,
  shortcuts: defaultShortcutSettings,
};

export function normalizeAppSettings(value: unknown): AppSettings {
  const candidate = isRecord(value) ? value : {};
  return {
    libraryColumns: oneOf(candidate.libraryColumns, [3, 4, 5] as const, 4) as LibraryColumns,
    backgroundOpacity: finite(candidate.backgroundOpacity, 0, 1, defaultAppSettings.backgroundOpacity),
    minimizeToTray: candidate.minimizeToTray === true,
    closeAction: candidate.closeAction === 'tray' ? 'tray' : 'exit',
    reading: normalizeReadingTypographySettings(candidate.reading),
    shortcuts: normalizeShortcutSettings(candidate.shortcuts),
  };
}

export function normalizeReadingTypographySettings(value: unknown): ReadingTypographySettings {
  const candidate = isRecord(value) ? value : {};
  const txt = isRecord(candidate.txt) ? candidate.txt : {};
  const epub = isRecord(candidate.epub) ? candidate.epub : {};
  return {
    fontFamily: oneOf(candidate.fontFamily, [
      'system',
      'source-han-serif',
      'noto-serif-cjk',
      'source-han-sans',
      'noto-sans-cjk',
      'lxgw-wenkai',
    ] as const, defaultReadingTypographySettings.fontFamily) as ReadingFont,
    fontSize: finite(candidate.fontSize, 12, 36, defaultReadingTypographySettings.fontSize),
    fontWeight: finite(candidate.fontWeight, 300, 700, defaultReadingTypographySettings.fontWeight),
    letterSpacing: finite(candidate.letterSpacing, -0.05, 0.3, defaultReadingTypographySettings.letterSpacing),
    firstLineIndent: finite(candidate.firstLineIndent, 0, 4, defaultReadingTypographySettings.firstLineIndent),
    lineHeight: finite(candidate.lineHeight, 1.2, 2.4, defaultReadingTypographySettings.lineHeight),
    paragraphSpacing: finite(candidate.paragraphSpacing, 0, 1.5, defaultReadingTypographySettings.paragraphSpacing),
    textAlign: oneOf(candidate.textAlign, ['start', 'justify'] as const, defaultReadingTypographySettings.textAlign) as ReadingTextAlign,
    contentWidth: finite(candidate.contentWidth, 480, 1280, defaultReadingTypographySettings.contentWidth),
    horizontalMargin: finite(candidate.horizontalMargin, 8, 160, defaultReadingTypographySettings.horizontalMargin),
    verticalMargin: finite(candidate.verticalMargin, 8, 120, defaultReadingTypographySettings.verticalMargin),
    columns: oneOf(candidate.columns, [1, 2] as const, defaultReadingTypographySettings.columns) as ReadingColumns,
    txt: {
      leadingIndent: oneOf(txt.leadingIndent, ['clean', 'preserve'] as const, defaultReadingTypographySettings.txt.leadingIndent) as TxtLeadingIndent,
      blankLines: oneOf(txt.blankLines, ['preserve', 'single', 'remove'] as const, defaultReadingTypographySettings.txt.blankLines) as TxtBlankLines,
      mergeWrappedLines: txt.mergeWrappedLines === true,
      chapterTitleStyle: oneOf(txt.chapterTitleStyle, ['prominent', 'compact', 'plain'] as const, defaultReadingTypographySettings.txt.chapterTitleStyle) as TxtChapterTitleStyle,
    },
    epub: normalizeEpubTypographySettings(epub),
  };
}

function normalizeEpubTypographySettings(value: Record<string, unknown>): EpubTypographySettings {
  return {
    usePublisherStyles: value.usePublisherStyles !== false,
    overrideFont: value.overrideFont === true,
    overrideFontSize: value.overrideFontSize !== false,
    overrideIndent: value.overrideIndent === true,
    overrideLineHeight: value.overrideLineHeight !== false,
    overrideParagraphSpacing: value.overrideParagraphSpacing === true,
    useEmbeddedFonts: value.useEmbeddedFonts !== false,
  };
}

function normalizeShortcutSettings(value: unknown): ShortcutSettings {
  const candidate = isRecord(value) ? value : {};
  return shortcutActionIds.reduce((shortcuts, action) => {
    const shortcut = candidate[action];
    shortcuts[action] = typeof shortcut === 'string' && shortcut.trim()
      ? shortcut.trim().slice(0, 48)
      : null;
    return shortcuts;
  }, { ...defaultShortcutSettings });
}

export function loadAppSettings(): AppSettings {
  try {
    const current = localStorage.getItem(STORAGE_KEY);
    if (current) return normalizeAppSettings(JSON.parse(current));
    const legacy = localStorage.getItem(LEGACY_STORAGE_KEY);
    if (legacy) return normalizeAppSettings(JSON.parse(legacy));
    const legacyColumns = Number(localStorage.getItem('readloom-library-columns'));
    return normalizeAppSettings({ libraryColumns: legacyColumns });
  } catch {
    return normalizeAppSettings(null);
  }
}

export function persistAppSettings(settings: AppSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(normalizeAppSettings(settings)));
    localStorage.removeItem(LEGACY_STORAGE_KEY);
    localStorage.removeItem('readloom-library-columns');
  } catch {
    // Settings remain active for this process when browser storage is unavailable.
  }
}

function finite(value: unknown, minimum: number, maximum: number, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(maximum, Math.max(minimum, value))
    : fallback;
}

function oneOf<T>(value: unknown, values: readonly T[], fallback: T): T {
  return values.includes(value as T) ? value as T : fallback;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
