// The largest files under a folder: a row of stats and a table. Shows config with defaults,
// a config error, and a table with a gauge in each row.
//
//   dashboard plugin-check --plugin-config '{"dir": "."}' -- node sdk/typescript/examples/big_files.ts
//
//   modules:
//     big-files:
//       plugin: [node, sdk/typescript/examples/big_files.ts]
//       refresh: 1m
//       config: { dir: ~/Downloads, top: 8 }

import { readdirSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { join, relative } from "node:path";
import { configError, log, serve, withDefaults } from "../dashboard-plugin.ts";
import type { Row } from "../dashboard-plugin.ts";

let config = { dir: ".", top: 5, max_files: 20000 };

type File = { path: string; size: number };

/** Every file under `dir`, skipping dot folders and stopping at `max_files`. */
function walk(dir: string, out: File[]): void {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return; // unreadable: skip it
  }
  for (const e of entries) {
    if (out.length >= config.max_files) return;
    const path = join(dir, e.name);
    if (e.isDirectory() && !e.name.startsWith(".") && e.name !== "node_modules") walk(path, out);
    else if (e.isFile()) out.push({ path, size: statSync(path, { throwIfNoEntry: false })?.size ?? 0 });
  }
}

function bytes(n: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i += 1;
  }
  return i === 0 ? `${n} B` : `${n.toFixed(1)} ${units[i]}`;
}

serve({
  manifest: {
    id: "big-files",
    name: "Big files",
    refresh: { mode: "poll", interval_ms: 60000, min_interval_ms: 5000 },
    layout: { size: "medium", priority: 10, min_size: "small" },
    config_schema: {
      type: "object",
      additionalProperties: false,
      properties: {
        dir: { type: "string", description: "Folder to scan. `~` is your home folder." },
        top: { type: "integer", minimum: 1, description: "How many files to list." },
        max_files: { type: "integer", minimum: 1, description: "Stop scanning after this many files." },
      },
    },
  },
  init(c) {
    config = withDefaults(c, config);
    config.dir = config.dir.replace(/^~(?=$|\/)/, homedir());
    if (!statSync(config.dir, { throwIfNoEntry: false })?.isDirectory()) {
      throw configError(`not a folder: ${config.dir}`);
    }
  },
  render() {
    const files: File[] = [];
    walk(config.dir, files);
    if (files.length >= config.max_files) log("info", `stopped after ${config.max_files} files`);
    files.sort((a, b) => b.size - a.size);
    const total = files.reduce((sum, f) => sum + f.size, 0);
    const top = files.slice(0, config.top);
    const rows: Row[] = top.map((f) => {
      const share = total ? (f.size / total) * 100 : 0;
      return {
        cells: {
          file: relative(config.dir, f.path),
          size: bytes(f.size),
          share: { type: "gauge", label: "", percent: share },
        },
        status: share > 50 ? "warn" : "ok",
      };
    });
    return {
      status: "ok",
      summary: `${files.length} files · ${bytes(total)}`,
      root: {
        type: "column",
        children: [
          {
            type: "row",
            children: [
              { type: "stat", label: "Files", value: String(files.length) },
              { type: "stat", label: "Total", value: bytes(total) },
              { type: "stat", label: "Largest", value: top[0] ? bytes(top[0].size) : "-" },
            ],
          },
          {
            type: "table",
            columns: [
              { key: "file", label: "File" },
              { key: "size", label: "Size" },
              { key: "share", label: "Share" },
            ],
            rows,
          },
        ],
      },
    };
  },
});
