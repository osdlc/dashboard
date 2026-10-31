/** The session token this tab signed in with (see useDashboard). */
let current: string | null = null

export function setSessionToken(token: string | null) {
  current = token
}

/** Headers for an `/api` request outside the WebSocket, e.g. `/api/catalog`. */
export function apiHeaders(): Record<string, string> {
  return current ? { Authorization: `Bearer ${current}` } : {}
}
