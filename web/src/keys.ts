/** Every key the web view handles. The help panel is built from this table. */
export const KEYS: ReadonlyArray<readonly [string, string]> = [
  ['?', 'show or hide this help'],
  ['1-9', 'go to screen'],
  ['← ↑ ↓ → / h j k l', 'move between tiles; inside a tile, move between items'],
  ['Enter', 'go into the focused tile; on an item, open its link in a new tab'],
  ['[ / ]', 'previous / next screen'],
  ['s', 'show or hide settings'],
  ['d', 'compact or comfortable spacing between and inside tiles'],
  ['e', 'edit screens: add, remove, resize and reorder tiles'],
  ['double-click', 'show a tile fullscreen (Esc or double-click to close)'],
  ['r', 'start or stop screen rotation (when configured)'],
  ['Esc', 'close help or settings; otherwise step back out of an item, fullscreen, then the tile'],
]
