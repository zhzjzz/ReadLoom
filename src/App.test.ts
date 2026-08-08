import { render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import App from './App.svelte';
import { documentStore } from './lib/stores/documentStore';

describe('App error feedback', () => {
  it('shows actionable Rust errors without clearing the workspace', async () => {
    render(App);
    await screen.findByText('打开一本书，开始阅读或编织');

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
    expect(screen.getByText('打开一本书，开始阅读或编织')).toBeTruthy();
  });
});
