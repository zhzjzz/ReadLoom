import { describe, expect, it } from 'vitest';

import { normalizeAppError } from './backend';

describe('normalizeAppError', () => {
  it('preserves a valid Rust error DTO', () => {
    expect(
      normalizeAppError({
        code: 'INPUT_EMPTY',
        message: '通信测试内容不能为空。',
        recoverable: true,
        suggestedAction: '请输入测试内容后重试。',
      }),
    ).toEqual({
      code: 'INPUT_EMPTY',
      message: '通信测试内容不能为空。',
      recoverable: true,
      suggestedAction: '请输入测试内容后重试。',
    });
  });

  it('replaces unknown rejection values with an actionable fallback', () => {
    expect(normalizeAppError('transport failed')).toEqual({
      code: 'IPC_UNKNOWN',
      message: '无法连接 Readloom 核心。',
      recoverable: true,
      suggestedAction: '请确认应用核心仍在运行，然后重试。',
    });
  });
});

