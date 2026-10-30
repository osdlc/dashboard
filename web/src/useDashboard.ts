import { onUnmounted, ref } from 'vue'
import type { ClientMessage, ServerMessage, Theme, TileUpdate, UiSettings } from './contract'

export type ConnectionState = 'connecting' | 'open' | 'closed' | 'no-token'

/** Settings and live tile updates from the host over the WebSocket. Reconnects on close. */
export function useDashboard() {
  const tiles = ref<Record<string, TileUpdate>>({})
  const settings = ref<UiSettings | null>(null)
  const state = ref<ConnectionState>('connecting')
  const token = new URLSearchParams(location.search).get('token')

  let socket: WebSocket | null = null
  let retry: number | undefined
  let stopped = false

  function connect() {
    if (!token) {
      state.value = 'no-token'
      return
    }
    state.value = 'connecting'
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    socket = new WebSocket(`${proto}://${location.host}/api/ws?token=${encodeURIComponent(token)}`)
    socket.onopen = () => (state.value = 'open')
    socket.onmessage = (ev) => {
      const msg = JSON.parse(ev.data) as ServerMessage
      if (msg.type === 'tile') {
        tiles.value[msg.tile.tile_id] = msg.tile
      } else if (msg.type === 'settings') {
        settings.value = msg.settings
        applyTheme(msg.settings.theme)
      }
    }
    socket.onclose = () => {
      state.value = 'closed'
      if (!stopped) retry = window.setTimeout(connect, 2000)
    }
  }

  connect()
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

/** `auto` removes the attribute so the CSS follows prefers-color-scheme. */
function applyTheme(theme: Theme) {
  const root = document.documentElement
  if (theme === 'auto') delete root.dataset.theme
  else root.dataset.theme = theme
}
