export interface ShortcutActions {
  open(): void;
  save(): void;
  saveAs(): void;
  close(): void;
}

export function createShortcutHandler(actions: ShortcutActions): (event: KeyboardEvent) => void {
  return (event) => {
    if (event.isComposing || event.keyCode === 229 || event.repeat || !(event.ctrlKey || event.metaKey)) {
      return;
    }

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
    }
  };
}
