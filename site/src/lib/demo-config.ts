// Pulls an excerpt out of the repo's examples/demo.yaml at build time, so the
// config shown on the site is the real one. The build fails if a marker moves.
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

// `pnpm build` runs in site/; the demo config is one level up.
const file = resolve(process.cwd(), '../examples/demo.yaml')

function between(lines: string[], start: RegExp, end: RegExp): string[] {
  const from = lines.findIndex((l) => start.test(l))
  if (from < 0) throw new Error(`demo.yaml: no line matching ${start}`)
  const to = lines.findIndex((l, i) => i > from && end.test(l))
  if (to < 0) throw new Error(`demo.yaml: no line matching ${end} after ${start}`)
  return lines.slice(from, to)
}

export function demoExcerpt(): string {
  const lines = readFileSync(file, 'utf8').split('\n')
  const trim = (ls: string[]) => {
    while (ls.length && ls[ls.length - 1].trim() === '') ls.pop()
    return ls
  }
  const parts = [
    trim(between(lines, /^# Each demo tile/, /^theme:/)),
    trim(between(lines, /^theme:/, /^# rotate:|^$/)),
    ['', 'modules:'],
    trim(between(lines, /^ {2}# ---- showcase ----/, /^ {2}# ---- charts ----/)),
    ['', '  # …'],
    trim(between(lines, /^ {2}# ---- uptime/, /^ {2}# ---- lists ----/)),
    [''],
    trim(between(lines, /^# Sizes on the 12-column grid/, /^ {2}- name: charts/)),
    ['  # …'],
  ]
  return parts.flat().join('\n')
}
