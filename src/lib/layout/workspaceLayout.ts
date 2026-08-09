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
