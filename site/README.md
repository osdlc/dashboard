# Site

The project site (Astro). It has a page for every widget, with its demo view at every tile size
(web and terminal, light and dark), what it needs, a config example and its settings. Build and
preview it:

    cd site && pnpm install && pnpm build && pnpm preview   # then open /dashboard/widgets/

The screenshots come from the web app's `?mode=demo&catalog=<file>` page (add
`&id=cpu&size=wide&theme=light` for one tile), which draws catalog entries' demo views with no
server; the terminal ones come from the TUI's `tile_shots` test. Widget pages also embed that page,
built into `site/public/demo-app/`, in a frame you can resize. `pnpm registry` here redoes it all
(needs `target/debug/dashboard`, `web/` built, Chrome and cargo).
