export type LibraryColumns = 3 | 4 | 5;
export type WindowCloseAction = 'exit' | 'tray';

export type ReadingFont =
  | 'system'
  | 'source-han-serif'
  | 'noto-serif-cjk'
  | 'source-han-sans'
  | 'noto-sans-cjk'
  | 'lxgw-wenkai';
export type ReadingTextAlign = 'start' | 'justify';
export type ReadingColumns = 1 | 2;
export type TxtLeadingIndent = 'clean' | 'preserve';
export type TxtBlankLines = 'preserve' | 'single' | 'remove';
export type TxtChapterTitleStyle = 'prominent' | 'compact' | 'plain';

export interface TxtTypographySettings {
  leadingIndent: TxtLeadingIndent;
  blankLines: TxtBlankLines;
  mergeWrappedLines: boolean;
  chapterTitleStyle: TxtChapterTitleStyle;
}

export interface EpubTypographySettings {
  usePublisherStyles: boolean;
  overrideFont: boolean;
  overrideFontSize: boolean;
  overrideIndent: boolean;
  overrideLineHeight: boolean;
  overrideParagraphSpacing: boolean;
  useEmbeddedFonts: boolean;
}

export interface ReadingTypographySettings {
  fontFamily: ReadingFont;
  fontSize: number;
  fontWeight: number;
  letterSpacing: number;
  firstLineIndent: number;
  lineHeight: number;
  paragraphSpacing: number;
  textAlign: ReadingTextAlign;
  contentWidth: number;
  horizontalMargin: number;
  verticalMargin: number;
  columns: ReadingColumns;
  txt: TxtTypographySettings;
  epub: EpubTypographySettings;
}

export type ShortcutActionId =
  | 'open'
  | 'save'
  | 'saveAs'
  | 'close'
  | 'toggleEdit'
  | 'previousChapter'
  | 'nextChapter'
  | 'bookmark'
  | 'showLibrary'
  | 'showSettings';

export type ShortcutSettings = Record<ShortcutActionId, string | null>;

export interface AppSettings {
  libraryColumns: LibraryColumns;
  backgroundOpacity: number;
  minimizeToTray: boolean;
  closeAction: WindowCloseAction;
  reading: ReadingTypographySettings;
  shortcuts: ShortcutSettings;
}

export interface BackgroundImageDto {
  key: string;
  mediaType: 'image/png' | 'image/jpeg' | 'image/webp';
}

export interface ReadingFontOption {
  id: ReadingFont;
  label: string;
  stack: string;
  license: string;
}

export const readingFontOptions: readonly ReadingFontOption[] = [
  {
    id: 'system',
    label: '系统默认',
    stack: "system-ui, 'Microsoft YaHei UI', 'Microsoft YaHei', sans-serif",
    license: '随 Windows 提供',
  },
  {
    id: 'source-han-serif',
    label: '思源宋体',
    stack: "'Source Han Serif SC', 'Noto Serif CJK SC', 'Songti SC', SimSun, serif",
    license: 'SIL Open Font License',
  },
  {
    id: 'noto-serif-cjk',
    label: 'Noto Serif CJK',
    stack: "'Noto Serif CJK SC', 'Source Han Serif SC', 'Songti SC', SimSun, serif",
    license: 'SIL Open Font License',
  },
  {
    id: 'source-han-sans',
    label: '思源黑体',
    stack: "'Source Han Sans SC', 'Noto Sans CJK SC', 'Microsoft YaHei UI', sans-serif",
    license: 'SIL Open Font License',
  },
  {
    id: 'noto-sans-cjk',
    label: 'Noto Sans CJK',
    stack: "'Noto Sans CJK SC', 'Source Han Sans SC', 'Microsoft YaHei UI', sans-serif",
    license: 'SIL Open Font License',
  },
  {
    id: 'lxgw-wenkai',
    label: '霞鹜文楷',
    stack: "'LXGW WenKai', '霞鹜文楷', 'KaiTi', 'Source Han Serif SC', serif",
    license: 'SIL Open Font License',
  },
] as const;

export function readingFontStack(font: ReadingFont): string {
  return readingFontOptions.find((option) => option.id === font)?.stack
    ?? readingFontOptions[0].stack;
}
