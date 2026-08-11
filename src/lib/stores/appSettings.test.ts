import { beforeEach, describe, expect, it } from 'vitest';

import {
  defaultAppSettings,
  loadAppSettings,
  normalizeAppSettings,
  persistAppSettings,
} from './appSettings';

describe('appSettings', () => {
  beforeEach(() => localStorage.clear());

  it('normalizes untrusted values and clamps the background strength', () => {
    expect(normalizeAppSettings({
      libraryColumns: 5,
      backgroundOpacity: 8,
      minimizeToTray: true,
      closeAction: 'tray',
    })).toEqual({
      ...defaultAppSettings,
      libraryColumns: 5,
      backgroundOpacity: 1,
      minimizeToTray: true,
      closeAction: 'tray',
    });
    expect(normalizeAppSettings({ backgroundOpacity: Number.NaN })).toEqual(defaultAppSettings);
  });

  it('migrates the former library column preference into unified settings', () => {
    localStorage.setItem('readloom-library-columns', '3');

    expect(loadAppSettings().libraryColumns).toBe(3);
    persistAppSettings({ ...defaultAppSettings, libraryColumns: 5 });

    expect(localStorage.getItem('readloom-library-columns')).toBeNull();
    expect(loadAppSettings().libraryColumns).toBe(5);
  });
});
