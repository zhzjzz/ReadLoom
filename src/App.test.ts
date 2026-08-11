import { render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import App from './App.svelte';
import { documentStore } from './lib/stores/documentStore';

describe('App error feedback', () => {
  it('opens on the library and shows actionable Rust errors without clearing it', async () => {
    render(App);
    await screen.findByRole('heading', { name: '我的书库' });
    expect(screen.getByRole('button', { name: '书库' }).getAttribute('aria-current')).toBe('page');

    documentStore.failed({
      code: 'EXTERNAL_MODIFICATION',
      message: '文件已被其他程序修改，Readloom 没有覆盖它。',
      recoverable: true,
      suggestedAction: '请选择另存为、重新加载或取消。',
    });

    await waitFor(() => {
      expect(screen.getByRole('alert').textContent).toContain('Readloom 没有覆盖它');
      expect(screen.getByRole('alert').textContent).toContain('请选择另存为');
    });
    expect(screen.getByRole('heading', { name: '书库还是空的' })).toBeTruthy();
  });
});
