export interface ShortcutActions {
  open(): void;
  save(): void;
  saveAs(): void;
  close(): void;
  toggleEdit?(): void;
  previousChapter?(): void;
  nextChapter?(): void;
  bookmark?(): void | boolean;
}

export function createShortcutHandler(actions: ShortcutActions): (event: KeyboardEvent) => void {
  return (event) => {
    if (event.isComposing || event.keyCode === 229 || event.repeat) {
      return;
    }

    if (event.altKey && !event.ctrlKey && !event.metaKey && event.key === 'ArrowUp') {
      event.preventDefault();
      actions.previousChapter?.();
      return;
    }
    if (event.altKey && !event.ctrlKey && !event.metaKey && event.key === 'ArrowDown') {
      event.preventDefault();
      actions.nextChapter?.();
      return;
    }
    if (!(event.ctrlKey || event.metaKey)) return;

    const key = event.key.toLowerCase();
    if (key === 'o' && !event.shiftKey) {
      event.preventDefault();
      actions.open();
    } else if (key === 's' && event.shiftKey) {
      event.preventDefault();
      actions.saveAs();
    } else if (key === 's') {
      event.preventDefault();
      actions.save();
    } else if (key === 'w') {
      event.preventDefault();
      actions.close();
    } else if (key === 'e' && !event.shiftKey) {
      event.preventDefault();
      actions.toggleEdit?.();
    } else if (key === 'b' && !event.shiftKey && actions.bookmark) {
      if (actions.bookmark() !== false) event.preventDefault();
    }
  };
}
