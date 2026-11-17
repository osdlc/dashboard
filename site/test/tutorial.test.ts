// Run with `pnpm test` (node --test). Checks the tutorial page's code windows,
// and that docs/tutorial.md only uses configs and commands that exist.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { codeWindows, plainText } from '../src/lib/tutorial.ts'

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repo = resolve(site, '..')
const markdown = readFileSync(join(repo, 'docs/tutorial.md'), 'utf8')

/** The fenced code blocks of the tutorial with the given language. */
function blocks(lang: string): string[] {
  return [...markdown.matchAll(/^```(\w*)\n([\s\S]*?)^```$/gm)].filter((m) => m[1] === lang).map((m) => m[2])
}

test('code blocks become windows titled by kind or file name', () => {
  const html =
    '<pre class="astro-code vitesse-dark" style="background-color:#000" tabindex="0" data-language="sh"><code><span>dashboard tui</span>\n</code></pre>' +
    '<pre class="astro-code" data-language="yaml"><code># ~/.config/dashboard/config.yaml\ntheme: auto</code></pre>' +
    '<pre class="astro-code" data-language="text"><code>ok</code></pre>'
  const out = codeWindows(html)
  assert.equal(out.match(/class="window code-window"/g)?.length, 3)
  assert.match(out, /<span class="title">terminal<\/span><button type="button" class="copy" data-copy="dashboard tui">/)
  assert.match(out, /<span class="title">~\/.config\/dashboard\/config.yaml<\/span>/)
  assert.match(out, /<span class="title">output<\/span><span class="spacer"><\/span>/)
  assert.doesNotMatch(out, /background-color/)
  assert.match(out, /<pre class="code astro-code vitesse-dark"/)
})

test('plainText drops tags and decodes entities', () => {
  assert.equal(plainText('<b>a &amp; b</b> &lt;x&gt; &quot;q&quot; &#39;s&#x27;\n'), 'a & b <x> "q" \'s\'')
})

const bin = [process.env.DASHBOARD_BIN, join(repo, 'target/debug/dashboard'), join(repo, 'target/release/dashboard')]
  .filter((p): p is string => !!p)
  .find((p) => existsSync(p))

test('every yaml block in the tutorial loads in `dashboard config`', { skip: !bin && 'no dashboard binary' }, () => {
  const dir = join(repo, '.tmp', 'tutorial-check')
  rmSync(dir, { recursive: true, force: true })
  // The tutorial installs clock-py; here it is found in plugins/ next to the config.
  mkdirSync(join(dir, 'plugins'), { recursive: true })
  symlinkSync(join(repo, 'plugins/ext/clock-py'), join(dir, 'plugins', 'clock-py'))
  const env = { ...process.env }
  for (const d of ['CONFIG', 'DATA', 'STATE', 'CACHE']) env[`XDG_${d}_HOME`] = join(dir, 'home', d.toLowerCase())
  const failed: string[] = []
  try {
    blocks('yaml').forEach((text, i) => {
      const file = join(dir, `block-${i}.yaml`)
      writeFileSync(file, text)
      try {
        execFileSync(bin!, ['--no-project', '--config', file, 'config'], { env, cwd: dir, stdio: 'pipe' })
      } catch (err: any) {
        failed.push(`block ${i} (${text.split('\n')[0]}): ${String(err.stderr || err.message).trim()}`)
      }
    })
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
  assert.deepEqual(failed, [])
})

test('every `dashboard` command in the tutorial exists', { skip: !bin && 'no dashboard binary' }, () => {
  const commands = (args: string[]) =>
    new Set(
      [...execFileSync(bin!, [...args, '--help'], { encoding: 'utf8' }).matchAll(/^ {2}([a-z][\w-]*) /gm)].map((m) => m[1]),
    )
  const top = commands([])
  const unknown: string[] = []
  for (const line of blocks('sh').flatMap((b) => b.split('\n'))) {
    // `dashboard [--global-flag [value]] command [subcommand]`
    const m = /^dashboard((?:\s+--[\w-]+(?:\s+[a-z][\w-]*)?)*?)\s+([a-z][\w-]*)(?:\s+([a-z][\w-]*))?/.exec(line.trim())
    if (!m) continue
    const [, , cmd, sub] = m
    if (!top.has(cmd)) unknown.push(line)
    else if ((cmd === 'plugin' || cmd === 'dashboards') && sub && !commands([cmd]).has(sub)) unknown.push(line)
  }
  assert.deepEqual(unknown, [])
})
