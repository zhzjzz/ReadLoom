export type WorkspacePaneSide = 'left' | 'right';

export function resizedPaneWidth(
  side: WorkspacePaneSide,
  startWidth: number,
  startPointerX: number,
  pointerX: number,
  minimum: number,
  maximum: number,
): number {
  const pointerDelta = pointerX - startPointerX;
  const requested = startWidth + (side === 'left' ? pointerDelta : -pointerDelta);
  return Math.round(Math.max(minimum, Math.min(maximum, requested)));
}

export function resizedPaneWidthFromKeyboard(
  side: WorkspacePaneSide,
  currentWidth: number,
  key: string,
  minimum: number,
  maximum: number,
  step = 12,
): number | null {
  if (key === 'Home') return minimum;
  if (key === 'End') return maximum;
  const direction = side === 'left' ? 1 : -1;
  const delta = key === 'ArrowRight' ? step * direction : key === 'ArrowLeft' ? -step * direction : null;
  if (delta === null) return null;
  return Math.round(Math.max(minimum, Math.min(maximum, currentWidth + delta)));
}
