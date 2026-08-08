import { writable } from 'svelte/store';

export type ThemePreference = 'system' | 'light' | 'dark';
export type ResolvedTheme = 'light' | 'dark';

const STORAGE_KEY = 'readloom.theme';

export const themePreference = writable<ThemePreference>('system');
export const resolvedTheme = writable<ResolvedTheme>('light');

export function resolveTheme(
  preference: ThemePreference,
  systemPrefersDark: boolean,
): ResolvedTheme {
  return preference === 'system' ? (systemPrefersDark ? 'dark' : 'light') : preference;
}

export function setTheme(preference: ThemePreference): void {
  themePreference.set(preference);
}

export function initializeTheme(): () => void {
  if (typeof window === 'undefined' || typeof document === 'undefined') {
    return () => undefined;
  }

  const savedPreference = window.localStorage.getItem(STORAGE_KEY);
  if (isThemePreference(savedPreference)) {
    themePreference.set(savedPreference);
  }

  const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
  let currentPreference: ThemePreference = 'system';

  const apply = () => {
    const nextTheme = resolveTheme(currentPreference, mediaQuery.matches);
    document.documentElement.dataset.theme = nextTheme;
    document.documentElement.style.colorScheme = nextTheme;
    resolvedTheme.set(nextTheme);
  };

  const stopSubscription = themePreference.subscribe((preference) => {
    currentPreference = preference;
    window.localStorage.setItem(STORAGE_KEY, preference);
    apply();
  });
  const handleSystemThemeChange = () => apply();

  mediaQuery.addEventListener('change', handleSystemThemeChange);

  return () => {
    stopSubscription();
    mediaQuery.removeEventListener('change', handleSystemThemeChange);
  };
}

function isThemePreference(value: string | null): value is ThemePreference {
  return value === 'system' || value === 'light' || value === 'dark';
}

