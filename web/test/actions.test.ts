// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { webUrl } from '../src/actions.ts'

test('only web links are used as links', () => {
  assert.equal(webUrl('https://example.com/a'), 'https://example.com/a')
  assert.equal(webUrl('HTTP://example.com'), 'HTTP://example.com')
  for (const url of ['file:///etc/passwd', 'javascript:alert(1)', 'vscode://file/x', 'ftp://x', '', null, undefined]) {
    assert.equal(webUrl(url), undefined, String(url))
  }
})
