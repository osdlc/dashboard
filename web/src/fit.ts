/**
 * Fitting a tile's view to its box. A tile's height comes from its size on
 * the grid, never from its content, so a long view can't push other tiles
 * around (docs/contract.md, "When a view doesn't fit"). After each draw,
 * `fitTile` walks the view from the top and cuts what falls below the
 * tile's bottom edge, the same way the TUI does (crates/tui/src/widgets.rs):
 *
 * - a list, table, bar list, uptime list, pie legend or meter grid shows the
 *   items that fit whole, and "+N more" after them;
 * - in a row or column, a child that still doesn't fit (or would show none
 *   of its items) is left out, and so is every child after it; a one-line
 *   text just before the cut (a heading) is left out first, to make room;
 * - a table too wide for the tile drops columns, lowest `priority` first
 *   (of equal ones the rightmost), always keeping the first, and narrows
 *   the rest, cutting cells with "…".
 *
 * Left-out elements get `data-fit-cut`, which the CSS hides; a shortened
 * container gets `data-fit-more="+N more"`, which the CSS shows after its
 * items. The fullscreen tile scrolls instead and is not fitted.
 */

/** Containers whose items are shortened to fit, and where their items are. */
const ITEMS: Array<[string, string]> = [
  ['.w-list', ':scope > li'],
  ['.w-table', ':scope > tbody > tr'],
  ['.w-bars', ':scope > .bar-row'],
  ['.w-uptime', ':scope > .up-row'],
  ['.pie-legend', ':scope > li'],
  ['.meters', ':scope > .meter'],
]
const TRIMMED = ITEMS.map(([sel]) => sel).join(', ')

/**
 * Column indexes in the order a too-wide table drops them: lowest priority
 * first, of equal ones the rightmost. The first column is never dropped.
 */
export function dropOrder(priorities: ReadonlyArray<number | null | undefined>): number[] {
  return priorities
    .map((p, i) => ({ p: p ?? 0, i }))
    .slice(1)
    .sort((a, b) => a.p - b.p || b.i - a.i)
    .map((c) => c.i)
}

/** How many items, from the first, end at or above `limit`. */
export function fitCount(bottoms: readonly number[], limit: number): number {
  const over = bottoms.findIndex((b) => b > limit + 0.5)
  return over === -1 ? bottoms.length : over
}

/**
 * The columns a table keeps in `room` pixels, with their widths, like the
 * TUI's `table_columns`: while the columns' least widths don't fit, drop
 * one (see `dropOrder`); then the widest columns give up width until the
 * row fits. `natural` is each column's width uncut, `least` the narrowest
 * it is cut to.
 */
export function tableColumns(
  natural: readonly number[],
  least: readonly number[],
  priorities: ReadonlyArray<number | null | undefined>,
  room: number,
): Array<[number, number]> {
  const order = dropOrder(natural.map((_, i) => priorities[i]))
  let keep = natural.map((_, i) => i)
  const sum = (cols: number[], w: readonly number[]) => cols.reduce((t, i) => t + w[i]!, 0)
  while (keep.length > 1 && sum(keep, least) > room) {
    const drop = order.shift()
    keep = keep.filter((i) => i !== drop)
  }
  const widths = keep.map((i) => natural[i]!)
  let over = widths.reduce((t, w) => t + w, 0) - room
  // A pixel at a time from the widest column that can still give.
  while (over > 0.5) {
    let k = -1
    widths.forEach((w, j) => {
      if (w > least[keep[j]!]! + 0.5 && (k === -1 || w > widths[k]!)) k = j
    })
    if (k === -1) break
    const give = Math.min(over, 1, widths[k]! - least[keep[k]!]!)
    widths[k]! -= give
    over -= give
  }
  if (over > 0.5) widths[0] = Math.max(0, widths[0]! - over)
  return keep.map((i, k) => [i, widths[k]!])
}

/** Fits the view inside `tile` (a `.tile` element) to the tile's box. */
export function fitTile(tile: HTMLElement): void {
  for (const el of tile.querySelectorAll<HTMLElement>('[data-fit-cut]')) delete el.dataset.fitCut
  for (const el of tile.querySelectorAll<HTMLElement>('[data-fit-more]')) delete el.dataset.fitMore
  if (tile.dataset.size === 'full') return
  const root = [...tile.children].find((c) => [...c.classList].some((name) => name.startsWith('w-')))
  if (!(root instanceof HTMLElement)) return
  const box = tile.getBoundingClientRect()
  const style = getComputedStyle(tile)
  const limit = box.bottom - parseFloat(style.borderBottomWidth) - parseFloat(style.paddingBottom)
  for (const table of tile.querySelectorAll<HTMLTableElement>('table.w-table')) {
    if (!table.parentElement?.closest('.w-table')) fitColumns(table)
  }
  fit(root, limit)
}

/** Where an element's content ends, counting what overflows it. */
function bottom(el: HTMLElement): number {
  const r = el.getBoundingClientRect()
  return r.top + Math.max(r.height, el.scrollHeight)
}

function fit(el: HTMLElement, limit: number): void {
  if (bottom(el) <= limit + 0.5) return
  if (el.matches('.w-row, .w-column')) {
    fitChildren(el, limit)
    return
  }
  const targets = el.matches(TRIMMED) ? [el] : [...el.querySelectorAll<HTMLElement>(TRIMMED)]
  for (const target of targets) {
    if (!target.parentElement?.closest('.w-table')) trim(target, limit)
  }
}

/** A row's or column's children: the first that doesn't fit and all after it are left out. */
function fitChildren(container: HTMLElement, limit: number): void {
  const kids = [...container.children].filter((k): k is HTMLElement => k instanceof HTMLElement)
  let cut = false
  kids.forEach((kid, i) => {
    if (cut) {
      kid.dataset.fitCut = ''
      return
    }
    if (fits(kid, limit)) return
    cut = true
    // A one-line text just before the cut is a heading: it goes, and the
    // child it heads gets its room.
    const before = kids[i - 1]
    if (before?.matches('.w-text') && oneLine(before)) {
      before.dataset.fitCut = ''
      clear(kid)
      if (fits(kid, limit)) return
    }
    kid.dataset.fitCut = ''
  })
}

/** Fits `kid` above `limit`; false if it still doesn't fit or shows none of its items. */
function fits(kid: HTMLElement, limit: number): boolean {
  if (kid.getBoundingClientRect().top < limit) fit(kid, limit)
  if (bottom(kid) > limit + 0.5) return false
  const targets = kid.matches(TRIMMED) ? [kid] : [...kid.querySelectorAll<HTMLElement>(TRIMMED)]
  return !targets.some((t) => t.dataset.fitMore && !itemsOf(t).some((item) => item.dataset.fitCut === undefined))
}

function itemsOf(target: HTMLElement): HTMLElement[] {
  const selector = ITEMS.find(([sel]) => target.matches(sel))?.[1]
  if (!selector) return []
  return [...target.querySelectorAll<HTMLElement>(selector)].filter((el) => !el.classList.contains('row-leave-active'))
}

function clear(el: HTMLElement): void {
  for (const c of el.querySelectorAll<HTMLElement>('[data-fit-cut]')) delete c.dataset.fitCut
  for (const c of [el, ...el.querySelectorAll<HTMLElement>('[data-fit-more]')]) delete c.dataset.fitMore
}

function oneLine(el: HTMLElement): boolean {
  const lineHeight = parseFloat(getComputedStyle(el).lineHeight)
  const height = el.getBoundingClientRect().height
  return Number.isNaN(lineHeight) ? height < 30 : height < lineHeight * 1.5
}

/** Shows the items that fit whole, and "+N more" after them. */
function trim(target: HTMLElement, limit: number): void {
  const items = itemsOf(target)
  if (!items.length || bottom(target) <= limit + 0.5) return
  const n = items.length
  let k = fitCount(
    items.map((el) => el.getBoundingClientRect().bottom),
    limit,
  )
  const cutFrom = (from: number) => {
    for (let i = from; i < n; i++) items[i]!.dataset.fitCut = ''
    target.dataset.fitMore = `+${n - from} more`
  }
  cutFrom(k)
  // Make room for the "+N more" line.
  while (k > 0 && bottom(target) > limit + 0.5) cutFrom(--k)
}

/**
 * Fits a too-wide table to its tile: drops columns, lowest priority first,
 * and narrows the rest, whose cells end with "…" (see `tableColumns`).
 */
function fitColumns(table: HTMLTableElement): void {
  for (const cell of table.querySelectorAll<HTMLElement>('[data-fit-width]')) {
    delete cell.dataset.fitWidth
    cell.style.width = cell.style.maxWidth = ''
  }
  const head = table.tHead?.rows[0]
  const room = table.parentElement?.clientWidth
  if (!head || !room || table.offsetWidth <= room + 0.5) return
  const cells = [...head.cells]
  const em = parseFloat(getComputedStyle(table).fontSize) || 14
  const pad = 12
  const natural = cells.map((th) => th.offsetWidth)
  // Like the TUI: the first column keeps about 12 characters, the others 6.
  const least = natural.map((w, i) => Math.min(w, (i === 0 ? 7 : 3.5) * em + pad))
  const priorities = cells.map((th) => (th.dataset.priority === undefined ? 0 : Number(th.dataset.priority)))
  const kept = new Map(tableColumns(natural, least, priorities, room))
  for (const row of table.rows) {
    ;[...row.cells].forEach((cell, i) => {
      const width = kept.get(i)
      if (width === undefined) {
        cell.dataset.fitCut = ''
      } else if (width < natural[i]! - 0.5) {
        cell.dataset.fitWidth = ''
        cell.style.width = cell.style.maxWidth = `${width}px`
      }
    })
  }
}
