import { describe, expect, it } from 'vitest';

import { resizedPaneWidth, resizedPaneWidthFromKeyboard } from './workspaceLayout';

describe('resizedPaneWidth', () => {
  it('resizes left and right panes toward the pointer and clamps their bounds', () => {
    expect(resizedPaneWidth('left', 220, 200, 290, 180, 480)).toBe(310);
    expect(resizedPaneWidth('right', 286, 1_000, 900, 180, 480)).toBe(386);
    expect(resizedPaneWidth('left', 220, 200, -100, 180, 480)).toBe(180);
    expect(resizedPaneWidth('right', 286, 1_000, 0, 180, 480)).toBe(480);
  });

  it('uses visual arrow direction for left and right splitters', () => {
    expect(resizedPaneWidthFromKeyboard('left', 220, 'ArrowRight', 180, 480)).toBe(232);
    expect(resizedPaneWidthFromKeyboard('right', 280, 'ArrowLeft', 200, 520)).toBe(292);
    expect(resizedPaneWidthFromKeyboard('right', 280, 'Home', 200, 520)).toBe(200);
    expect(resizedPaneWidthFromKeyboard('right', 280, 'Escape', 200, 520)).toBeNull();
  });
});
