import type { EpubBridgeMessage, OpenedEpubDocumentDto } from '../../types/epub';

export interface ExpectedBridgeSource {
  source: MessageEventSource | null;
  document: OpenedEpubDocumentDto;
}

export interface InternalEpubTarget {
  resourceId: string;
  fragment: string | null;
}

export interface ExternalEpubTarget {
  href: string;
  domain: string;
}

export function parseEpubBridgeMessage(
  event: MessageEvent<unknown>,
  expected: ExpectedBridgeSource,
): EpubBridgeMessage | null {
  if (event.source !== expected.source || !isRecord(event.data)) return null;
  let serializedLength = Number.POSITIVE_INFINITY;
  try {
    serializedLength = JSON.stringify(event.data).length;
  } catch {
    return null;
  }
  if (serializedLength > 4096) return null;

  const value = event.data;
  if (
    value.source !== 'readloom-epub' ||
    value.version !== 1 ||
    value.documentId !== expected.document.documentId ||
    value.sessionId !== expected.document.sessionId ||
    value.token !== expected.document.bridgeToken ||
    !isRecord(value.payload)
  ) {
    return null;
  }

  if (value.type === 'progress') {
    const progression = value.payload.progression;
    const fragment = value.payload.fragment;
    if (
      typeof progression !== 'number' ||
      !Number.isFinite(progression) ||
      progression < 0 ||
      progression > 1 ||
      (fragment !== null && (typeof fragment !== 'string' || fragment.length > 256))
    ) {
      return null;
    }
    return value as unknown as EpubBridgeMessage;
  }

  if (value.type === 'link') {
    const href = value.payload.href;
    if (typeof href !== 'string' || href.length > 2048) return null;
    return parseInternalEpubHref(href, expected.document.sessionId) || parseExternalEpubHref(href)
      ? (value as unknown as EpubBridgeMessage)
      : null;
  }
  return null;
}

export function parseExternalEpubHref(href: string): ExternalEpubTarget | null {
  const prefix = 'readloom-external:';
  if (!href.startsWith(prefix) || href.length > 4096) return null;
  try {
    const decoded = decodeURIComponent(href.slice(prefix.length));
    const target = new URL(decoded);
    if (
      !['http:', 'https:'].includes(target.protocol) ||
      target.username ||
      target.password
    ) {
      return null;
    }
    return { href: target.href, domain: target.host };
  } catch {
    return null;
  }
}

export function parseInternalEpubHref(
  href: string,
  expectedSessionId: string,
): InternalEpubTarget | null {
  try {
    const url = new URL(href);
    const custom = url.protocol === 'readloom-epub:' && url.hostname === 'localhost';
    const mapped =
      url.protocol === 'http:' && url.hostname.toLowerCase() === 'readloom-epub.localhost';
    if (!custom && !mapped) return null;
    const decodedSegments = url.pathname
      .split('/')
      .filter(Boolean)
      .map((segment) => decodeURIComponent(segment));
    if (decodedSegments.shift() !== expectedSessionId || decodedSegments.length === 0) return null;
    if (
      decodedSegments.some(
        (segment) =>
          !segment ||
          segment === '.' ||
          segment === '..' ||
          segment.includes('\\') ||
          segment.includes('\0') ||
          safeDecode(segment) !== segment,
      )
    ) {
      return null;
    }
    return {
      resourceId: decodedSegments.join('/'),
      fragment: url.hash ? decodeURIComponent(url.hash.slice(1)).slice(0, 256) : null,
    };
  } catch {
    return null;
  }
}

function safeDecode(value: string): string | null {
  try {
    return decodeURIComponent(value);
  } catch {
    return null;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
