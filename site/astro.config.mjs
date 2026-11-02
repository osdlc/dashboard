import { defineConfig } from 'astro/config'

// Served from https://osdlc.github.io/dashboard/. Change BASE here (and only
// here) if the site moves, e.g. to '/' on its own domain.
const BASE = '/dashboard'

export default defineConfig({
  site: 'https://osdlc.github.io',
  base: BASE,
  trailingSlash: 'ignore',
  // Markdown (the tutorial page, from docs/tutorial.md): the same code colors as the rest of the site.
  markdown: { shikiConfig: { theme: 'vitesse-dark' } },
})
