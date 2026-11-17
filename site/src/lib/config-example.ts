// A widget's settings and a config that adds it, worked out from its catalog
// entry. Kept apart from registry.ts, which loads the data, so tests can run it
// without the site (`pnpm test`).

type Schema = Record<string, any>

export interface Widget {
  id: string
  name: string
  version: string
  description: string
  tags: string[]
  kind: 'builtin' | 'plugin'
  /** `builtin`, `bundled`, or a package folder relative to the repository. */
  source: string
  /** For a built-in: the bundle it is compiled in with (system, dev, ops, fun, agents, personal). */
  bundle?: string
  default_size: 'small' | 'medium' | 'large' | 'wide' | 'tall'
  config_schema: Schema | null
  requirements: { binaries: string[]; env: string[]; hosts: string[] }
  demo: { status: string; summary?: string | null }
}

// ---- settings, from the config schema ----

export interface Setting {
  key: string
  type: string
  required: boolean
  /** The default as YAML, or '' when there is none. */
  default: string
  description: string
}

/** One row per top-level property of the config schema. */
export function settings(w: Widget): Setting[] {
  const schema = w.config_schema
  if (!schema?.properties) return []
  const required = new Set<string>(schema.required ?? [])
  return Object.entries(schema.properties as Record<string, Schema>).map(([key, raw]) => {
    const prop = resolve(schema, raw)
    const fallback = prop.default ?? raw.default
    return {
      key,
      type: typeOf(schema, prop),
      required: required.has(key),
      default: fallback === undefined || fallback === null ? '' : yaml(fallback),
      description: prop.description ?? raw.description ?? '',
    }
  })
}

/** Follows a local `$ref` (`#/$defs/...` or `#/definitions/...`). */
function resolve(root: Schema, s: Schema): Schema {
  const ref: string | undefined = s.$ref ?? s.allOf?.[0]?.$ref
  if (!ref?.startsWith('#/')) return s
  const target = ref
    .slice(2)
    .split('/')
    .reduce<Schema | undefined>((at, part) => at?.[part], root)
  return target ? { ...target, ...s, $ref: undefined } : s
}

function typeOf(root: Schema, s: Schema): string {
  const choices = (s.oneOf ?? s.anyOf ?? [])
    .map((c: Schema) => resolve(root, c))
    .filter((c: Schema) => c.const !== undefined)
    .map((c: Schema) => String(c.const))
  if (s.enum) return s.enum.map(String).join(' | ')
  if (choices.length) return choices.join(' | ')
  const types = ([] as string[]).concat(s.type ?? []).filter((t) => t !== 'null')
  if (types.includes('array')) {
    const item = s.items ? typeOf(root, resolve(root, s.items)) : ''
    return item && !item.includes(' ') ? `list of ${item}` : 'list'
  }
  return types.join(' | ') || 'value'
}

// ---- config example ----

/** A config that adds the widget: its module block with every default spelled out, and a tile. */
export function configExample(w: Widget): string {
  const lines = ['modules:', `  ${w.id}:`]
  if (w.kind === 'plugin' && w.source !== 'bundled') lines.push(`    package: ${w.id}`)
  // A real value, so the module block is never empty (YAML would read it as null).
  lines.push('    demo: false          # true: made-up data, no account or network needed')
  const withDefaults = settings(w).filter((s) => s.default !== '' && s.default !== '[]' && s.default !== '{}')
  const unset = settings(w).filter((s) => s.required && s.default === '')
  if (withDefaults.length || unset.length) {
    lines.push('    config:')
    for (const s of unset) lines.push(`      ${s.key}: ${placeholder(s)}`)
    for (const s of withDefaults) lines.push(`      ${s.key}: ${s.default}`)
  }
  lines.push('screens:', '  - name: main', '    tiles:', `      - { module: ${w.id}, size: ${w.default_size} }`)
  return lines.join('\n')
}

function placeholder(s: Setting): string {
  if (s.type.startsWith('list')) return '[]   # required'
  return '...  # required'
}

/** A JSON value as one line of YAML (flow style; JSON is valid YAML). */
export function yaml(value: unknown): string {
  if (typeof value === 'string') {
    // Plain only when YAML would read it back as the same string.
    const plain =
      /^[A-Za-z_./@~][\w./:@~-]*$/.test(value) &&
      !value.endsWith(':') &&
      !/^(true|false|null|yes|no|on|off|y|n|~|\.inf|\.nan)$/i.test(value)
    return plain ? value : JSON.stringify(value)
  }
  if (Array.isArray(value)) return `[${value.map(yaml).join(', ')}]`
  if (value && typeof value === 'object') {
    return `{ ${Object.entries(value)
      .map(([k, v]) => `${k}: ${yaml(v)}`)
      .join(', ')} }`
  }
  return JSON.stringify(value)
}
