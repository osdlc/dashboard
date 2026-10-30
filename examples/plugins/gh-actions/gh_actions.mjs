#!/usr/bin/env node
// Recent GitHub Actions runs for a few repositories, newest first, with
// pass/fail/running counts. Uses the `gh` CLI and its login; no npm packages.
//
//   ci:
//     plugin: [node, examples/plugins/gh-actions/gh_actions.mjs]
//     config:
//       repos: [osdlc/dashboard, osdlc/osdlc]
//       limit: 5          # runs per repository
//       branch: main      # optional: only this branch
//
// Shows: stats in a row, a list with a status per item, tags and an
// open_url action, the async request pattern (each request is answered when
// its work finishes), and protocol errors with the dashboard's own codes.
//
//   dashboard plugin-check --plugin-config '{"repos":["osdlc/dashboard"]}' -- node examples/plugins/gh-actions/gh_actions.mjs

import { execFile } from 'node:child_process'
import { createInterface } from 'node:readline'
import { promisify } from 'node:util'

const run = promisify(execFile)
const UPSTREAM_UNAVAILABLE = -32001
const CONFIG_INVALID = -32002

let config = { repos: [], limit: 5, branch: null }

class RpcError extends Error {
  constructor(code, message) {
    super(message)
    this.code = code
  }
}

const send = (msg) => process.stdout.write(JSON.stringify(msg) + '\n')

async function initialize(params) {
  const user = params.config ?? {}
  const unknown = Object.keys(user).filter((k) => !(k in config))
  if (unknown.length) throw new RpcError(CONFIG_INVALID, `unknown settings: ${unknown.join(', ')}`)
  config = { ...config, ...user }
  if (!Array.isArray(config.repos) || config.repos.length === 0) {
    throw new RpcError(CONFIG_INVALID, 'set `repos: [owner/name, ...]`')
  }
  return {
    manifest: {
      id: 'gh-actions',
      name: 'GitHub Actions',
      contract_version: '0.1',
      // gh uses the GitHub API (5000 requests an hour): a minute is plenty.
      refresh: { mode: 'poll', interval_ms: 60_000, min_interval_ms: 15_000 },
      layout: { size: 'medium', priority: 30 },
      commands: [],
    },
  }
}

const FIELDS = 'displayTitle,workflowName,status,conclusion,headBranch,event,createdAt,updatedAt,url'

async function runs(repo) {
  const args = ['run', 'list', '-R', repo, '--limit', String(config.limit), '--json', FIELDS]
  if (config.branch) args.push('--branch', config.branch)
  try {
    const { stdout } = await run('gh', args, { timeout: 20_000 })
    return JSON.parse(stdout).map((r) => ({ ...r, repo }))
  } catch (e) {
    if (e.code === 'ENOENT') throw new RpcError(UPSTREAM_UNAVAILABLE, 'gh is not installed')
    throw new RpcError(UPSTREAM_UNAVAILABLE, `${repo}: ${(e.stderr || e.message).trim()}`)
  }
}

/** queued / in_progress -> running; otherwise the conclusion decides. */
function state(r) {
  if (r.status !== 'completed') return 'running'
  if (r.conclusion === 'success') return 'passed'
  if (['cancelled', 'skipped', 'neutral'].includes(r.conclusion)) return 'other'
  return 'failed'
}

const STATUS = { passed: 'ok', failed: 'error', running: 'warn', other: 'unknown' }

function ago(iso) {
  const s = Math.max(0, (Date.now() - Date.parse(iso)) / 1000)
  if (s < 60) return 'just now'
  if (s < 3600) return `${Math.floor(s / 60)}m ago`
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`
  return `${Math.floor(s / 86400)}d ago`
}

function took(r) {
  const s = Math.round((Date.parse(r.updatedAt) - Date.parse(r.createdAt)) / 1000)
  return s >= 60 ? `${Math.floor(s / 60)}m ${s % 60}s` : `${s}s`
}

async function render() {
  const all = (await Promise.all(config.repos.map(runs))).flat()
  all.sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt))
  const count = (s) => all.filter((r) => state(r) === s).length
  const [passed, failed, running] = [count('passed'), count('failed'), count('running')]
  // The tile's status follows each repository's newest finished run.
  const latest = new Map()
  for (const r of all) if (state(r) !== 'running' && !latest.has(r.repo)) latest.set(r.repo, r)
  const red = [...latest.values()].filter((r) => state(r) === 'failed')

  return {
    status: red.length ? 'error' : running ? 'warn' : 'ok',
    summary: red.length
      ? `failing: ${red.map((r) => r.repo).join(', ')}`
      : `${passed} passed · ${running} running`,
    root: {
      type: 'column',
      children: [
        {
          type: 'row',
          children: [
            { type: 'stat', label: 'Passed', value: String(passed), status: 'ok' },
            { type: 'stat', label: 'Failed', value: String(failed), status: failed ? 'error' : 'ok' },
            { type: 'stat', label: 'Running', value: String(running), status: running ? 'warn' : 'ok' },
          ],
        },
        {
          type: 'list',
          empty_text: 'No runs yet',
          items: all.map((r) => ({
            title: r.displayTitle,
            subtitle: `${r.repo} · ${r.workflowName} · ${ago(r.createdAt)}${state(r) === 'running' ? '' : ` · took ${took(r)}`}`,
            status: STATUS[state(r)],
            tags: [r.headBranch, ...(state(r) === 'running' ? ['running'] : [])],
            action: { type: 'open_url', url: r.url },
          })),
        },
      ],
    },
  }
}

const handlers = { initialize, 'view/render': render, shutdown: async () => null }

createInterface({ input: process.stdin }).on('line', async (line) => {
  const msg = JSON.parse(line)
  if (msg.method === 'exit') process.exit(0)
  if (msg.id === undefined) return
  try {
    const handler = handlers[msg.method]
    if (!handler) throw new RpcError(-32601, `unknown method ${msg.method}`)
    send({ jsonrpc: '2.0', id: msg.id, result: await handler(msg.params ?? {}) })
  } catch (e) {
    send({ jsonrpc: '2.0', id: msg.id, error: { code: e.code ?? -32603, message: e.message } })
  }
})
