// __NAME__: a sitroom plugin. Made by `sitroom plugin new`.
//
// Try it:   sitroom plugin-check -- node main.ts      (or: bun main.ts)
// SDK:      sitroom-plugin.ts, next to this file. Protocol and widgets: docs/plugins.md.

import { serve, withDefaults } from "./sitroom-plugin.ts";
import type { View } from "./sitroom-plugin.ts";

// Settings from the tile's `config:` block, with their defaults.
let config = { greeting: "hello" };

serve({
  manifest: {
    id: "__ID__",
    name: "__NAME__",
    refresh: { mode: "poll", interval_ms: 30000, min_interval_ms: 5000 },
    layout: { size: "medium" },
    config_schema: {
      type: "object",
      additionalProperties: false,
      properties: { greeting: { type: "string" } },
    },
  },

  // Check the config here. Throw configError("...") if it is wrong.
  init(c) {
    config = withDefaults(c, config);
  },

  // Return what the tile shows. Throw upstreamError("...") if a service you need is down.
  render(): View {
    const now = new Date();
    return {
      status: "ok",
      summary: `${config.greeting} · ${now.toTimeString().slice(0, 5)}`,
      root: {
        type: "column",
        children: [
          {
            type: "row",
            children: [
              { type: "stat", label: "Greeting", value: config.greeting },
              { type: "stat", label: "Time", value: now.toTimeString().slice(0, 8) },
            ],
          },
          {
            type: "list",
            items: [
              { title: "Edit main.ts to show your own data", tags: ["todo"] },
              {
                title: "Read about widgets",
                subtitle: "docs/plugins.md",
                action: { type: "open_url", url: "https://github.com/osdlc/sitroom/blob/main/docs/plugins.md" },
              },
            ],
          },
        ],
      },
    };
  },
});
