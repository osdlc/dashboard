// The smallest plugin with the SDK: a greeting, a clock and a render counter.
//
//   dashboard plugin-check -- node sdk/typescript/examples/hello.ts
//   dashboard plugin-check -- bun sdk/typescript/examples/hello.ts
//
//   modules:
//     hello:
//       plugin: [node, sdk/typescript/examples/hello.ts]
//       config: { name: you }

import { serve, withDefaults } from "../dashboard-plugin.ts";

let config = { name: "world" };
let renders = 0;

serve({
  manifest: {
    id: "hello",
    name: "Hello",
    refresh: { mode: "poll", interval_ms: 5000, min_interval_ms: 1000 },
    layout: { size: "small", priority: 0 },
  },
  init(c) {
    config = withDefaults(c, config);
  },
  render() {
    renders += 1;
    return {
      status: "ok",
      summary: `hello, ${config.name}`,
      root: {
        type: "row",
        children: [
          { type: "stat", label: `Hello, ${config.name}`, value: new Date().toTimeString().slice(0, 8) },
          { type: "stat", label: "Renders", value: String(renders) },
        ],
      },
    };
  },
});
