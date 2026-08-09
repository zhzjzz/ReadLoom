import type {
  ChapterDraftAccepted,
  ChapterEditDto,
  ChapterDraftUpdate,
} from '../../types/epub';

export type ChapterSyncStatus =
  | 'idle'
  | 'typing'
  | 'waiting'
  | 'syncing'
  | 'synced'
  | 'warning'
  | 'failed'
  | 'conflict';

interface Snapshot {
  chapterEditId: string;
  clientRevision: number;
  editorDocument: Record<string, unknown>;
}

interface ChapterDraftSyncOptions {
  debounceMs?: number;
  submit(update: ChapterDraftUpdate): Promise<ChapterDraftAccepted>;
  onStatus(status: ChapterSyncStatus): void;
  onAccepted(accepted: ChapterDraftAccepted): void;
  onError(error: unknown): void;
}

export class ChapterDraftSync {
  private readonly debounceMs: number;
  private readonly submit: ChapterDraftSyncOptions['submit'];
  private readonly onStatus: ChapterDraftSyncOptions['onStatus'];
  private readonly onAccepted: ChapterDraftSyncOptions['onAccepted'];
  private readonly onError: ChapterDraftSyncOptions['onError'];
  private chapterEditId = '';
  private acceptedRevision = 0;
  private clientRevision = 0;
  private requestSequence = 0;
  private composing = false;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private pending: Snapshot | null = null;
  private inFlight: Promise<void> | null = null;
  private lastError: unknown = null;

  constructor(options: ChapterDraftSyncOptions) {
    this.debounceMs = options.debounceMs ?? 550;
    this.submit = options.submit;
    this.onStatus = options.onStatus;
    this.onAccepted = options.onAccepted;
    this.onError = options.onError;
  }

  open(chapter: ChapterEditDto): void {
    this.clearTimer();
    this.chapterEditId = chapter.chapterEditId;
    this.acceptedRevision = chapter.acceptedRevision;
    this.clientRevision = Math.max(chapter.revision, chapter.acceptedRevision);
    this.pending = null;
    this.lastError = null;
    this.onStatus(chapter.dirty ? 'synced' : 'idle');
  }

  update(editorDocument: Record<string, unknown>): number {
    this.clientRevision += 1;
    this.pending = {
      chapterEditId: this.chapterEditId,
      clientRevision: this.clientRevision,
      editorDocument: structuredClone(editorDocument),
    };
    this.lastError = null;
    this.onStatus(this.composing ? 'typing' : 'waiting');
    if (!this.composing) this.schedule();
    return this.clientRevision;
  }

  compositionStart(): void {
    this.composing = true;
    this.clearTimer();
    this.onStatus('typing');
  }

  compositionEnd(): void {
    this.composing = false;
    if (this.pending) {
      this.onStatus('waiting');
      this.schedule();
    }
  }

  async flush(): Promise<void> {
    this.composing = false;
    this.clearTimer();
    if (this.pending && !this.inFlight) this.startPipeline();
    await this.inFlight;
    if (this.pending && !this.lastError) {
      this.startPipeline();
      await this.inFlight;
    }
    if (this.lastError) throw this.lastError;
  }

  destroy(): void {
    this.clearTimer();
    this.pending = null;
  }

  private schedule(): void {
    this.clearTimer();
    this.timer = setTimeout(() => {
      this.timer = null;
      this.startPipeline();
    }, this.debounceMs);
  }

  private startPipeline(): void {
    if (this.inFlight || !this.pending || this.composing) return;
    this.inFlight = this.drain().finally(() => {
      this.inFlight = null;
      if (this.pending && !this.lastError && !this.composing) this.startPipeline();
    });
  }

  private async drain(): Promise<void> {
    while (this.pending && !this.composing) {
      const snapshot = this.pending;
      this.pending = null;
      const requestId = `chapter-sync-${Date.now()}-${++this.requestSequence}`;
      this.onStatus('syncing');
      try {
        const accepted = await this.submit({
          chapterEditId: snapshot.chapterEditId,
          baseRevision: this.acceptedRevision,
          clientRevision: snapshot.clientRevision,
          editorDocument: snapshot.editorDocument,
          requestId,
        });
        if (
          accepted.chapterEditId !== this.chapterEditId
          || accepted.chapterEditId !== snapshot.chapterEditId
          || accepted.requestId !== requestId
          || accepted.clientRevision !== snapshot.clientRevision
        ) {
          continue;
        }
        this.acceptedRevision = accepted.acceptedRevision;
        this.lastError = null;
        this.onAccepted(accepted);
        this.onStatus(accepted.warnings.length ? 'warning' : this.pending ? 'waiting' : 'synced');
      } catch (error) {
        this.pending ??= snapshot;
        this.lastError = error;
        const code = typeof error === 'object' && error && 'code' in error
          ? String((error as { code: unknown }).code)
          : '';
        this.onStatus(code === 'CHAPTER_REVISION_CONFLICT' ? 'conflict' : 'failed');
        this.onError(error);
        return;
      }
    }
  }

  private clearTimer(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }
}
