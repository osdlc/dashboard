// The tutorial page is docs/tutorial.md, so the repository and the site show the
// same text. This turns the rendered markdown's code blocks into the site's code
// windows: a title bar (the file name, or what kind of block it is) and a Copy
// button. Kept apart from the page so `pnpm test` can run it.

const kinds: Record<string, string> = {
  sh: 'terminal',
  text: 'output',
  yaml: 'yaml',
  ts: 'typescript',
  json: 'json',
}

/** Wraps every `<pre class="astro-code" data-language="…">` in a window with a title and a Copy button. */
export function codeWindows(html: string): string {
  return html.replace(/<pre\b([^>]*)>([\s\S]*?)<\/pre>/g, (_, attrs: string, inner: string) => {
    const lang = /data-language="([^"]*)"/.exec(attrs)?.[1] ?? ''
    const text = plainText(inner)
    // A yaml block that starts with a file name comment is titled with it.
    const file = lang === 'yaml' ? /^# ((?:~\/|[\w.-]+\/)*[\w.-]+\.ya?ml)$/m.exec(text.split('\n')[0])?.[1] : undefined
    const title = file ?? kinds[lang] ?? lang
    const pre = `<pre${attrs.replace(/\sstyle="[^"]*"/, '').replace(/class="/, 'class="code ')}>${inner}</pre>`
    const copy = lang === 'text' ? '' : `<button type="button" class="copy" data-copy="${attr(text)}">Copy</button>`
    return (
      `<div class="window code-window">` +
      `<div class="window-bar"><span class="lights" aria-hidden="true"><i></i><i></i><i></i></span>` +
      `<span class="title">${escape(title)}</span>${copy || '<span class="spacer"></span>'}</div>` +
      `${pre}</div>`
    )
  })
}

/** The text of an HTML fragment: tags dropped, entities decoded. */
export function plainText(html: string): string {
  return html
    .replace(/<[^>]*>/g, '')
    .replace(/&#x([0-9a-f]+);/gi, (_, h) => String.fromCodePoint(parseInt(h, 16)))
    .replace(/&#(\d+);/g, (_, d) => String.fromCodePoint(Number(d)))
    .replace(/&quot;/g, '"')
    .replace(/&#39;|&apos;/g, "'")
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&')
    .replace(/\n$/, '')
}

function escape(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

function attr(s: string): string {
  return escape(s).replace(/"/g, '&quot;')
}
