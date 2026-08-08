import { invoke, isTauri } from '@tauri-apps/api/core';

import type {
  AppErrorDto,
  StartupMetricsDto,
  SystemProbeDto,
  SystemProbeRequest,
} from '../types/ipc';

const UNKNOWN_ERROR: AppErrorDto = {
  code: 'IPC_UNKNOWN',
  message: '无法连接 Readloom 核心。',
  recoverable: true,
  suggestedAction: '请确认应用核心仍在运行，然后重试。',
};

export function hasTauriRuntime(): boolean {
  return isTauri();
}

export async function probeBackend(message: string): Promise<SystemProbeDto> {
  const request: SystemProbeRequest = {
    message,
    clientTimestampMs: Date.now(),
  };

  try {
    return await invoke<SystemProbeDto>('system_probe', { request });
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export async function reportFrontendReady(): Promise<StartupMetricsDto> {
  try {
    return await invoke<StartupMetricsDto>('frontend_ready');
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export function normalizeAppError(error: unknown): AppErrorDto {
  if (!error || typeof error !== 'object') {
    return UNKNOWN_ERROR;
  }

  const candidate = error as Partial<AppErrorDto>;
  if (
    typeof candidate.code !== 'string' ||
    typeof candidate.message !== 'string' ||
    typeof candidate.recoverable !== 'boolean'
  ) {
    return UNKNOWN_ERROR;
  }

  return {
    code: candidate.code,
    message: candidate.message,
    recoverable: candidate.recoverable,
    suggestedAction:
      typeof candidate.suggestedAction === 'string' ? candidate.suggestedAction : null,
  };
}

