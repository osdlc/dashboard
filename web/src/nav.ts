/**
 * Keyboard focus. Two levels: the arrow keys move between tiles; Enter goes
 * into a tile's items (list items, table rows, uptime checks, bars), where
 * Up/Down move and Enter runs the item's action. Esc steps back out.
 *
 * Focus is kept by tile id and item key, not position, so it stays on the
 * same thing when live updates add or reorder items. The TUI does the same
 * in crates/tui/src/nav.rs. No DOM here, so `pnpm test` runs it in Node.
 */
import type { Action, Widget } from './contract'

export type Dir = 'up' | 'down' | 'left' | 'right'

export type Focus =
  | { kind: 'none' }
  | { kind: 'tile'; tile: string }
  /** `index` is where the item was when last seen: the fallback when its key is gone. */
  | { kind: 'item'; tile: string; key: string; index: number }

export const NO_FOCUS: Focus = { kind: 'none' }

/** Something inside a tile that the keyboard can select. */
export interface NavItem {
  /** Stays the same for the same item across updates. */
  key: string
  action: Action | null
}

export interface Box {
  x: number
  y: number
  width: number
  height: number
}

/** The arrow key or h/j/k/l for a direction, else null. */
export function dirForKey(key: string): Dir | null {
  switch (key) {
    case 'ArrowUp':
    case 'k':
      return 'up'
    case 'ArrowDown':
    case 'j':
      return 'down'
    case 'ArrowLeft':
    case 'h':
      return 'left'
    case 'ArrowRight':
    case 'l':
      return 'right'
    default:
      return null
  }
}

export function focusedTile(f: Focus): string | null {
  return f.kind === 'none' ? null : f.tile
}

/**
 * Where the focused item is now: the same key if it is still there,
 * otherwise the nearest position. Null at tile level or with no items.
 */
export function itemIndex(f: Focus, items: readonly NavItem[]): number | null {
  if (f.kind !== 'item' || items.length === 0) return null
  const at = items.findIndex((i) => i.key === f.key)
  return at >= 0 ? at : Math.min(f.index, items.length - 1)
}

function item(tile: string, items: readonly NavItem[], index: number): Focus {
  return { kind: 'item', tile, key: items[index]!.key, index }
}

/**
 * Arrow keys. At tile level: to the nearest tile that way (or the first tile
 * when nothing is focused). At item level: up/down move between items;
 * left/right do nothing.
 */
export function moved(
  f: Focus,
  dir: Dir,
  tiles: ReadonlyArray<readonly [string, Box]>,
  items: readonly NavItem[],
): Focus {
  if (f.kind === 'none') return tiles[0] ? { kind: 'tile', tile: tiles[0][0] } : NO_FOCUS
  if (f.kind === 'tile') {
    const from = tiles.findIndex(([id]) => id === f.tile)
    if (from < 0) return NO_FOCUS
    const to = neighbor(
      tiles.map(([, b]) => b),
      from,
      dir,
    )
    return to === null ? f : { kind: 'tile', tile: tiles[to]![0] }
  }
  const at = itemIndex(f, items)
  if (at === null) return { kind: 'tile', tile: f.tile }
  const to = dir === 'up' ? Math.max(0, at - 1) : dir === 'down' ? Math.min(items.length - 1, at + 1) : at
  return item(f.tile, items, to)
}

/** Enter. Goes into a tile that has items; on an item, returns its action. */
export function enter(f: Focus, items: readonly NavItem[]): [Focus, Action | null] {
  if (f.kind === 'tile' && items.length > 0) return [item(f.tile, items, 0), null]
  if (f.kind === 'item') {
    const at = itemIndex(f, items)
    return [f, at === null ? null : items[at]!.action]
  }
  return [f, null]
}

/** Esc: from an item back to its tile, from a tile to nothing. */
export function back(f: Focus): Focus {
  return f.kind === 'item' ? { kind: 'tile', tile: f.tile } : NO_FOCUS
}

/**
 * The selectable items in a widget tree, in the order they are drawn.
 * Repeated keys get "#2", "#3"…, like `stableKeys` in motion.ts.
 */
export function items(w: Widget): NavItem[] {
  const out: NavItem[] = []
  collect(w, out)
  const seen = new Map<string, number>()
  return out.map((i) => {
    const n = (seen.get(i.key) ?? 0) + 1
    seen.set(i.key, n)
    return n === 1 ? i : { ...i, key: `${i.key}#${n}` }
  })
}

function collect(w: Widget, out: NavItem[]) {
  // The text people see, or the link when there is none. Links make poor
  // keys on their own: many items can share one.
  const key = (text: string, a: Action | null | undefined) => (text === '' && a?.type === 'open_url' ? a.url : text)
  switch (w.type) {
    case 'list':
      for (const i of w.items) out.push({ key: key(i.title, i.action), action: i.action ?? null })
      break
    case 'table': {
      const first = w.columns[0]?.key ?? ''
      for (const r of w.rows) {
        const c = r.cells[first]
        const text = typeof c === 'string' || typeof c === 'number' ? String(c) : ''
        out.push({ key: key(text, r.action), action: r.action ?? null })
      }
      break
    }
    case 'uptime':
      for (const c of w.checks) out.push({ key: c.name, action: c.url ? { type: 'open_url', url: c.url } : null })
      break
    case 'bars':
      for (const b of w.bars) out.push({ key: b.label, action: null })
      break
    case 'row':
    case 'column':
      for (const c of w.children) collect(c, out)
      break
  }
}

/**
 * The box nearest to `boxes[from]` in direction `dir`, or null. Prefers boxes
 * that line up with it (overlap across the direction of travel), then the
 * smallest gap, then the closest centre. Same rule as the TUI.
 */
export function neighbor(boxes: readonly Box[], from: number, dir: Dir): number | null {
  const a = boxes[from]!
  const alongX = dir === 'left' || dir === 'right'
  const sign = dir === 'right' || dir === 'down' ? 1 : -1
  const span = (b: Box, x: boolean): [number, number] => (x ? [b.x, b.x + b.width] : [b.y, b.y + b.height])
  const [a0, a1] = span(a, alongX)
  const [c0, c1] = span(a, !alongX)
  let best: number | null = null
  let bestScore: number[] = []
  for (let i = 0; i < boxes.length; i++) {
    const b = boxes[i]!
    if (i === from) continue
    const [b0, b1] = span(b, alongX)
    // Must lie past `a` that way: its centre and its far edge beyond a's.
    const ahead = sign * (b0 + b1 - (a0 + a1)) > 0 && (sign > 0 ? b1 > a1 : b0 < a0)
    if (!ahead) continue
    const gap = Math.max(0, sign > 0 ? b0 - a1 : a0 - b1)
    const [d0, d1] = span(b, !alongX)
    // Negative when they don't line up: how far apart they are.
    const overlap = Math.min(c1, d1) - Math.max(c0, d0)
    // Under a pixel of overlap is rounding, not lining up.
    const score = [overlap < 1 ? 1 : 0, gap, Math.max(0, -overlap), Math.abs(d0 + d1 - (c0 + c1))]
    if (best === null || less(score, bestScore)) {
      best = i
      bestScore = score
    }
  }
  return best
}

function less(a: number[], b: number[]): boolean {
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return a[i]! < b[i]!
  return false
}
