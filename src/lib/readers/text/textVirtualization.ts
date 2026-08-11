import type { TextReadingBlock } from './textParagraphs';

export const MAX_RENDERED_TEXT_BLOCKS = 600;
export const TEXT_WINDOW_EDGE_BLOCKS = 100;

export interface TextVirtualLayout {
  offsets: number[];
  totalHeight: number;
}

export interface TextViewportWindowState {
  currentStart: number;
  maximumStart: number;
  firstBlockTop: number;
  lastBlockBottom: number;
  viewportHeight: number;
  estimatedBlockHeight: number;
}

export function adjustTextWindowStartForViewport({
  currentStart,
  maximumStart,
  firstBlockTop,
  lastBlockBottom,
  viewportHeight,
  estimatedBlockHeight,
}: TextViewportWindowState): number {
  const safeHeight = Math.max(1, estimatedBlockHeight);
  const inset = Math.max(1, viewportHeight / 3);
  if (lastBlockBottom <= 0) {
    const shift = Math.max(1, Math.ceil((inset - lastBlockBottom) / safeHeight));
    return Math.min(maximumStart, currentStart + shift);
  }
  if (firstBlockTop >= viewportHeight) {
    const shift = Math.max(
      1,
      Math.ceil((firstBlockTop - (viewportHeight - inset)) / safeHeight),
    );
    return Math.max(0, currentStart - shift);
  }
  return currentStart;
}

export function estimateTextBlockLayout(
  blocks: readonly TextReadingBlock[],
  fontSize: number,
  lineHeight: number,
  paragraphSpacing: number,
  contentWidth: number,
  columns: 1 | 2,
): TextVirtualLayout {
  const columnWidth = Math.max(240, contentWidth / columns - (columns === 2 ? 28 : 0));
  const charactersPerLine = Math.max(12, Math.floor(columnWidth / Math.max(1, fontSize)));
  const offsets = new Array<number>(blocks.length + 1);
  offsets[0] = 0;
  for (let index = 0; index < blocks.length; index += 1) {
    const block = blocks[index];
    const lines = Math.max(1, Math.ceil(block.text.length / charactersPerLine));
    const height = block.kind === 'blank'
      ? fontSize * lineHeight
      : block.kind === 'heading'
        ? fontSize * 1.55 * 1.35 * lines + fontSize * 2.9
        : fontSize * lineHeight * lines + fontSize * paragraphSpacing;
    offsets[index + 1] = offsets[index] + height;
  }
  return { offsets, totalHeight: offsets.at(-1) ?? 0 };
}

export function textBlockIndexForSourceOffset(
  blocks: readonly TextReadingBlock[],
  sourceOffset: number,
): number {
  if (!blocks.length) return 0;
  let low = 0;
  let high = blocks.length - 1;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (blocks[middle].sourceEnd >= sourceOffset) high = middle;
    else low = middle + 1;
  }
  return low;
}

export function textBlockIndexForScrollOffset(offsets: readonly number[], scrollOffset: number): number {
  if (offsets.length <= 1) return 0;
  let low = 0;
  let high = offsets.length - 2;
  while (low < high) {
    const middle = Math.floor((low + high + 1) / 2);
    if (offsets[middle] <= scrollOffset) low = middle;
    else high = middle - 1;
  }
  return low;
}

export function textWindowStartForIndex(blockCount: number, blockIndex: number): number {
  if (blockCount <= MAX_RENDERED_TEXT_BLOCKS) return 0;
  const preferred = blockIndex - Math.floor(MAX_RENDERED_TEXT_BLOCKS / 3);
  return Math.max(0, Math.min(preferred, blockCount - MAX_RENDERED_TEXT_BLOCKS));
}
