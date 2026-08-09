import { describe, expect, it } from 'vitest';

import { resizedPaneWidth } from './workspaceLayout';

describe('resizedPaneWidth', () => {
  it('resizes left and right panes toward the pointer and clamps their bounds', () => {
    expect(resizedPaneWidth('left', 220, 200, 290, 180, 480)).toBe(310);
    expect(resizedPaneWidth('right', 286, 1_000, 900, 180, 480)).toBe(386);
    expect(resizedPaneWidth('left', 220, 200, -100, 180, 480)).toBe(180);
    expect(resizedPaneWidth('right', 286, 1_000, 0, 180, 480)).toBe(480);
  });
});
