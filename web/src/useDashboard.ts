import { onUnmounted, ref } from 'vue'
import { setSessionToken } from './session'
import type { ClientMessage, ServerMessage, TileUpdate, UiSettings } from './contract'
import { applyColorTheme, applyMode, DEFAULT_THEME } from './colors'

export type ConnectionState = 'signing in' | 'connecting' | 'open' | 'closed' | 'signed out'

/** Where this tab keeps its session token, so a reload stays signed in. */
const TOKEN_KEY = 'dashboard-session'

/**
 * Settings and live tile updates from the host over the WebSocket. Reconnects on close.
 *
 * The link `dashboard serve` opens carries a one-time boot token (`?boot=`). It is traded
 * once for the session token, which is kept in this tab's sessionStorage and never put
 * in a URL.
 */
export function useDashboard() {
  const tiles = ref<Record<string, TileUpdate>>({})
  const settings = ref<UiSettings | null>(null)
  const state = ref<ConnectionState>('signing in')
  let token: string | null = null

  let socket: WebSocket | null = null
  let retry: number | undefined
  let stopped = false

  async function start() {
    token = await signIn()
    setSessionToken(token)
    if (token) connect()
    else state.value = 'signed out'
  }

  function connect() {
    if (!token) return
    state.value = 'connecting'
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    // Browsers can't set headers on a WebSocket; the token goes as a subprotocol.
    socket = new WebSocket(`${proto}://${location.host}/api/ws`, ['dashboard', `bearer.${token}`])
    socket.onopen = () => (state.value = 'open')
    socket.onmessage = (ev) => {
      const msg = JSON.parse(ev.data) as ServerMessage
      if (msg.type === 'tile') {
        tiles.value[msg.tile.tile_id] = msg.tile
      } else if (msg.type === 'settings') {
        settings.value = msg.settings
        applyMode(msg.settings.theme)
        applyColorTheme(msg.settings.color_theme ?? DEFAULT_THEME)
      }
    }
    socket.onclose = async () => {
      state.value = 'closed'
      if (stopped) return
      // A restarted server has a new session token: this tab needs a new link.
      if (token && !(await tokenWorks(token))) {
        forget()
        setSessionToken(null)
        state.value = 'signed out'
        return
      }
      retry = window.setTimeout(connect, 2000)
    }
  }

  void start()
  onUnmounted(() => {
    stopped = true
    window.clearTimeout(retry)
    socket?.close()
  })

  function send(msg: ClientMessage) {
    if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(msg))
  }

  return { tiles, settings, state, send }
}

/** The session token: traded for the `?boot=` token in the URL, or kept from earlier in this tab. */
async function signIn(): Promise<string | null> {
  const params = new URLSearchParams(location.search)
  const boot = params.get('boot')
  if (boot) {
    // Out of the address bar and history right away; it works only once anyway.
    params.delete('boot')
    const query = params.toString()
    history.replaceState(null, '', `${location.pathname}${query ? `?${query}` : ''}${location.hash}`)
    try {
      const res = await fetch('/api/session', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ boot }),
      })
      if (res.ok) {
        const { token } = (await res.json()) as { token: string }
        remember(token)
        return token
      }
    } catch {
      // Fall through to a token kept from earlier.
    }
  }
  return recall()
}

/** Whether the server still takes `token`. A network error counts as yes: the server may be restarting. */
async function tokenWorks(token: string): Promise<boolean> {
  try {
    const res = await fetch('/api/tiles', { headers: { Authorization: `Bearer ${token}` } })
    return res.status !== 401
  } catch {
    return true
  }
}

function remember(token: string) {
  try {
    sessionStorage.setItem(TOKEN_KEY, token)
  } catch {
    // Storage blocked: signed in until the tab reloads.
  }
}

function recall(): string | null {
  try {
    return sessionStorage.getItem(TOKEN_KEY)
  } catch {
    return null
  }
}

function forget() {
  try {
    sessionStorage.removeItem(TOKEN_KEY)
  } catch {
    // Nothing to forget.
  }
}
