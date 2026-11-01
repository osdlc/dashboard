import type { Action } from './contract'

/**
 * Carries out an item's action, from Enter (mouse clicks on links use the
 * link itself). New kinds of action are added here.
 */
export function runAction(action: Action) {
  switch (action.type) {
    case 'open_url':
      // Only web links, so a module can't make this open something else.
      if (/^https?:\/\//.test(action.url)) window.open(action.url, '_blank', 'noopener')
      break
    case 'run_command':
      // Not run yet; `dashboard plugin-check` says so.
      break
  }
}
