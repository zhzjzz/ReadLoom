import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

describe('native window lifecycle capability', () => {
  it('allows the main window close listener to complete its automatic destroy call', () => {
    const capability = JSON.parse(
      readFileSync(resolve('src-tauri/capabilities/default.json'), 'utf8'),
    ) as { windows: string[]; permissions: string[] };

    expect(capability.windows).toContain('main');
    expect(capability.permissions).toContain('core:window:allow-destroy');
  });
});
