export interface AppErrorDto {
  code: string;
  message: string;
  recoverable: boolean;
  suggestedAction: string | null;
}

export interface SystemProbeRequest {
  message: string;
  clientTimestampMs: number;
}

export interface SystemProbeDto {
  appName: string;
  appVersion: string;
  platform: string;
  architecture: string;
  protocolVersion: number;
  echoedMessage: string;
  clientTimestampMs: number;
  serverTimestampMs: number;
}

export interface StartupMetricsDto {
  processId: number;
  mainToFrontendReadyMs: number;
  recordedAtUnixMs: number;
}

export type BackendConnection =
  | { status: 'checking' }
  | { status: 'browser-preview' }
  | {
      status: 'connected';
      probe: SystemProbeDto;
      startup: StartupMetricsDto;
    }
  | { status: 'error'; error: AppErrorDto };

