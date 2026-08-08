import type { EpubReadingSettings } from '../../types/epub';

const STORAGE_KEY = 'readloom.epub.reading-settings.v1';

export const defaultEpubReadingSettings: EpubReadingSettings = {
  fontFamily: 'system',
  fontSize: 18,
  lineHeight: 1.8,
  contentWidth: 832,
  pageMargin: 48,
  textAlign: 'start',
  publisherStyles: 'partial',
  ignorePublisherFonts: false,
  ignorePublisherColors: false,
  allowInternalFonts: true,
  imageMaximumWidth: 100,
};

export function normalizeEpubReadingSettings(value: unknown): EpubReadingSettings {
  const source = isRecord(value) ? value : {};
  return {
    fontFamily: oneOf(source.fontFamily, ['system', 'serif', 'sans'], 'system'),
    fontSize: finite(source.fontSize, 12, 32, 18),
    lineHeight: finite(source.lineHeight, 1.2, 2.4, 1.8),
    contentWidth: finite(source.contentWidth, 480, 1200, 832),
    pageMargin: finite(source.pageMargin, 8, 96, 48),
    textAlign: oneOf(source.textAlign, ['start', 'justify'], 'start'),
    publisherStyles: oneOf(source.publisherStyles, ['use', 'partial', 'ignore'], 'partial'),
    ignorePublisherFonts: source.ignorePublisherFonts === true,
    ignorePublisherColors: source.ignorePublisherColors === true,
    allowInternalFonts: source.allowInternalFonts !== false,
    imageMaximumWidth: finite(source.imageMaximumWidth, 50, 100, 100),
  };
}

export function loadEpubReadingSettings(): EpubReadingSettings {
  try {
    return normalizeEpubReadingSettings(JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null'));
  } catch {
    return { ...defaultEpubReadingSettings };
  }
}

export function persistEpubReadingSettings(settings: EpubReadingSettings): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(normalizeEpubReadingSettings(settings)));
}

function finite(value: unknown, minimum: number, maximum: number, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(maximum, Math.max(minimum, value))
    : fallback;
}

function oneOf<T extends string>(value: unknown, values: readonly T[], fallback: T): T {
  return typeof value === 'string' && values.includes(value as T) ? (value as T) : fallback;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
