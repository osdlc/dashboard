// System load average as a live chart. Shows `liveSeries`: each render sends only the newest
// values and the dashboard keeps the history, so the plugin stays stateless.
//
//   dashboard plugin-check --renders 5 -- node sdk/typescript/examples/load.ts

import { availableParallelism, loadavg, platform } from "node:os";
import { liveSeries, serve, upstreamError } from "../dashboard-plugin.ts";

serve({
  manifest: {
    id: "load",
    name: "Load",
    refresh: { mode: "poll", interval_ms: 5000, min_interval_ms: 1000 },
    layout: { size: "medium", priority: 20 },
  },
  init() {
    // Windows has no load average; Node reports zeros there.
    if (platform() === "win32") throw upstreamError("no load average on Windows");
  },
  render() {
    const [one, five, fifteen] = loadavg();
    const cores = availableParallelism();
    const busy = one / cores;
    return {
      status: busy > 1.5 ? "error" : busy > 1 ? "warn" : "ok",
      summary: `load ${one.toFixed(2)} on ${cores} cores`,
      root: {
        type: "column",
        children: [
          {
            type: "row",
            children: [
              { type: "stat", label: "1 min", value: one.toFixed(2) },
              { type: "stat", label: "5 min", value: five.toFixed(2) },
              { type: "stat", label: "15 min", value: fifteen.toFixed(2) },
            ],
          },
          liveSeries({ id: "load", label: "Load (1 min)", history: 120, style: "area" }, { load: one }),
        ],
      },
    };
  },
});
