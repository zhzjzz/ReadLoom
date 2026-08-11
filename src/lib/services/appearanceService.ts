import type { AppSettings, BackgroundImageDto } from '../types/settings';
import { invokeChecked } from './backend';

export async function getBackgroundImage(): Promise<BackgroundImageDto | null> {
  return invokeChecked('get_background_image');
}

export async function setBackgroundImage(path: string): Promise<BackgroundImageDto> {
  return invokeChecked('set_background_image', { request: { path } });
}

export async function clearBackgroundImage(): Promise<void> {
  await invokeChecked('clear_background_image');
}

export async function applyWindowBehavior(settings: AppSettings): Promise<void> {
  await invokeChecked('apply_window_behavior', {
    request: {
      trayVisible: settings.minimizeToTray || settings.closeAction === 'tray',
      minimizeToTray: settings.minimizeToTray,
    },
  });
}

export function backgroundImageUrl(key: string): string {
  return `http://readloom-background.localhost/${encodeURIComponent(key)}`;
}
