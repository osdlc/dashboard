// The registry's browsing sections. A built-in is listed under its bundle; a
// plugin under the area its tags point to. Kept apart from registry.ts so tests
// can run it without the site (`pnpm test`).
import type { Widget } from './config-example.ts'

export interface Area {
  id: string
  name: string
  blurb: string
}

/** In page order. Built-ins use the first six; plugins can use any. */
export const AREAS: Area[] = [
  { id: 'system', name: 'System', blurb: 'This machine: CPU, memory, GPU, disks, network, power and local files.' },
  { id: 'dev', name: 'Developer', blurb: 'Code, reviews, issues and builds.' },
  { id: 'ops', name: 'Ops and services', blurb: 'Containers, monitoring, alerts and the services you run.' },
  { id: 'fun', name: 'News and fun', blurb: 'Clocks, news, feeds, prices and scores. Most need no account.' },
  { id: 'agents', name: 'AI agents', blurb: 'Usage and limits for coding agents, read from their own local logins.' },
  { id: 'personal', name: 'Personal', blurb: 'Calendar, tasks, notes and weather.' },
  { id: 'examples', name: 'Examples', blurb: 'Small plugins to copy when you write your own.' },
  {
    id: 'benchmark',
    name: 'Benchmark copies',
    blurb: 'Built-in widgets rewritten in TypeScript and Python to compare against the native ones.',
  },
]

const BUILTIN_AREAS = new Set(['system', 'dev', 'ops', 'fun', 'agents', 'personal'])

/** The area a widget is listed under. */
export function areaOf(w: Widget): string {
  if (w.kind === 'builtin' && w.bundle && BUILTIN_AREAS.has(w.bundle)) return w.bundle
  const has = (...tags: string[]) => tags.some((t) => w.tags.includes(t))
  if (has('benchmark')) return 'benchmark'
  if (has('example')) return 'examples'
  if (has('agents')) return 'agents'
  if (has('ops')) return 'ops'
  if (has('dev')) return 'dev'
  if (has('personal', 'tasks', 'productivity')) return 'personal'
  if (has('news', 'fun', 'public-api')) return 'fun'
  if (has('system')) return 'system'
  return 'ops'
}

/** The areas that have widgets of this kind, each with its widgets, in page order. */
export function sections(widgets: Widget[], kind: Widget['kind']): { area: Area; widgets: Widget[] }[] {
  const mine = widgets.filter((w) => w.kind === kind)
  return AREAS.map((area) => ({ area, widgets: mine.filter((w) => areaOf(w) === area.id) })).filter(
    (s) => s.widgets.length > 0,
  )
}
