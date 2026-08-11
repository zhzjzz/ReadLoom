import { describe, expect, it } from 'vitest';

import type { TextReadingBlock } from './textParagraphs';
import {
  adjustTextWindowStartForViewport,
  MAX_RENDERED_TEXT_BLOCKS,
  estimateTextBlockLayout,
  textBlockIndexForScrollOffset,
  textBlockIndexForSourceOffset,
  textWindowStartForIndex,
} from './textVirtualization';

const blocks: TextReadingBlock[] = Array.from({ length: 2_000 }, (_, index) => ({
  kind: 'paragraph',
  text: `第 ${index + 1} 段`,
  sourceStart: index * 10,
  sourceEnd: index * 10 + 8,
}));

describe('TXT reading virtualization', () => {
  it('builds monotonic estimated offsets and resolves source and scroll positions', () => {
    const layout = estimateTextBlockLayout(blocks, 19, 1.7, 0.15, 780, 1);

    expect(layout.offsets).toHaveLength(blocks.length + 1);
    expect(layout.totalHeight).toBeGreaterThan(0);
    expect(textBlockIndexForSourceOffset(blocks, 10_005)).toBe(1_000);
    expect(textBlockIndexForScrollOffset(layout.offsets, layout.offsets[1_000])).toBe(1_000);
  });

  it('keeps the render window bounded around the active block', () => {
    const start = textWindowStartForIndex(blocks.length, 1_000);
    expect(start).toBeLessThan(1_000);
    expect(1_000).toBeLessThan(start + MAX_RENDERED_TEXT_BLOCKS);
    expect(textWindowStartForIndex(100, 50)).toBe(0);
    expect(textWindowStartForIndex(blocks.length, blocks.length - 1))
      .toBe(blocks.length - MAX_RENDERED_TEXT_BLOCKS);
  });

  it('moves a rendered window back over the viewport when estimates leave only a spacer visible', () => {
    expect(adjustTextWindowStartForViewport({
      currentStart: 400,
      maximumStart: 1_400,
      firstBlockTop: -8_260,
      lastBlockBottom: -160,
      viewportHeight: 640,
      estimatedBlockHeight: 27,
    })).toBeGreaterThan(400);

    expect(adjustTextWindowStartForViewport({
      currentStart: 800,
      maximumStart: 1_400,
      firstBlockTop: 900,
      lastBlockBottom: 9_000,
      viewportHeight: 640,
      estimatedBlockHeight: 27,
    })).toBeLessThan(800);

    expect(adjustTextWindowStartForViewport({
      currentStart: 800,
      maximumStart: 1_400,
      firstBlockTop: -200,
      lastBlockBottom: 900,
      viewportHeight: 640,
      estimatedBlockHeight: 27,
    })).toBe(800);
  });
});
