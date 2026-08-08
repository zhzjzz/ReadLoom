import { describe, expect, it } from 'vitest';

import type { OpenedEpubDocumentDto } from '../../types/epub';
import {
  parseEpubBridgeMessage,
  parseExternalEpubHref,
  parseInternalEpubHref,
} from './epubBridge';

const sessionId = '0123456789abcdef0123456789abcdef0123456789abcdef';
const source = {} as MessageEventSource;
const document = {
  documentId: 'doc-0000000000000001',
  sessionId,
  bridgeToken: 'abcdef0123456789abcdef0123456789abcdef0123456789',
} as OpenedEpubDocumentDto;

describe('EPUB bridge boundary', () => {
  it('accepts only the active iframe source and all session tokens', () => {
    const data = {
      source: 'readloom-epub',
      version: 1,
      type: 'progress',
      documentId: document.documentId,
      sessionId,
      token: document.bridgeToken,
      payload: { progression: 0.5, fragment: 'middle' },
    };
    const valid = new MessageEvent('message', { data, source });
    const stale = new MessageEvent('message', { data, source: {} as MessageEventSource });

    expect(parseEpubBridgeMessage(valid, { source, document })).not.toBeNull();
    expect(parseEpubBridgeMessage(stale, { source, document })).toBeNull();
    expect(
      parseEpubBridgeMessage(
        new MessageEvent('message', { data: { ...data, token: 'stale' }, source }),
        { source, document },
      ),
    ).toBeNull();
  });

  it('allows only the current EPUB session and rejects double decoding or network links', () => {
    expect(
      parseInternalEpubHref(
        `readloom-epub://localhost/${sessionId}/EPUB/text/chapter.xhtml#note`,
        sessionId,
      ),
    ).toEqual({ resourceId: 'EPUB/text/chapter.xhtml', fragment: 'note' });
    expect(parseInternalEpubHref('https://tracker.invalid/chapter', sessionId)).toBeNull();
    expect(
      parseInternalEpubHref(
        `readloom-epub://localhost/${sessionId}/EPUB/%252e%252e/private`,
        sessionId,
      ),
    ).toBeNull();
  });

  it('decodes only inert http/https external-link placeholders', () => {
    expect(
      parseExternalEpubHref(
        'readloom-external:https%3A%2F%2Fexample%2Ecom%2Fread%3Fq%3D1',
      ),
    ).toEqual({ href: 'https://example.com/read?q=1', domain: 'example.com' });
    expect(parseExternalEpubHref('https://example.com/read')).toBeNull();
    expect(parseExternalEpubHref('readloom-external:file%3A%2F%2FC%3A%2Fsecret')).toBeNull();
    expect(parseExternalEpubHref('readloom-external:https%253A%252F%252Fevil.invalid')).toBeNull();
  });
});
