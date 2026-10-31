// Text file: shows a text or markdown file, and shows it again when it changes.
//
// The TypeScript version of a native-vs-plugin benchmark widget. It follows the same rules as
// the native module (crates/modules-system/src/textfile.rs) and the Python plugin
// (plugins/ext/textfile-py), and must show the same view for the same file.
//
//   dashboard plugin-check --plugin-config '{"path": "README.md"}' -- node plugins/ext/textfile-ts/main.ts

import { readFileSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { basename, extname, join } from "node:path";

import { configError, serve, upstreamError, withDefaults } from "./dashboard-plugin.ts";
import type { View } from "./dashboard-plugin.ts";

/** Larger files are refused: this is for notes, not logs. */
const MAX_BYTES = 1024 * 1024;

let config: { path: string | null; markdown: boolean | null; lines: number } = {
  path: null,
  markdown: null,
  lines: 200,
};
let path = "";
let name = "";
let markdown = false;
/** Modified time and size the last view was built from, and that view. */
let seen: string | null = null;
let last: View | null = null;

/**
 * The tile for a file's text. All three versions of the widget follow these rules: drop a
 * leading byte order mark, turn CRLF into LF, drop trailing blanks (spaces, tabs, line breaks),
 * and keep the first `maxLines` lines.
 */
function view(text: string, name: string, markdown: boolean, maxLines: number): View {
  if (text.startsWith("\ufeff")) text = text.slice(1);
  text = text.replaceAll("\r\n", "\n").replace(/[ \t\r\n]+$/, "");
  if (text === "") {
    return { status: "ok", summary: `${name} · empty`, root: { type: "text", text: "(empty)", markdown: false } };
  }
  const lines = text.split("\n");
  if (lines.length > maxLines) {
    return {
      status: "ok",
      summary: `${name} · first ${maxLines} of ${lines.length} lines`,
      root: { type: "text", text: `${lines.slice(0, maxLines).join("\n")}\n…`, markdown },
    };
  }
  const count = lines.length === 1 ? "1 line" : `${lines.length} lines`;
  return { status: "ok", summary: `${name} · ${count}`, root: { type: "text", text, markdown } };
}

serve({
  manifest: () => ({
    id: "textfile-ts",
    name: `${name} (TS)`,
    refresh: { mode: "poll", interval_ms: 1000, min_interval_ms: 250 },
    layout: { size: "tall", priority: 0 },
  }),
  init(c) {
    config = withDefaults(c, config);
    if (typeof config.path !== "string") throw configError("the textfile plugin needs `config: { path: <file> }`");
    if (config.markdown !== null && typeof config.markdown !== "boolean") {
      throw configError("`markdown` must be true or false");
    }
    if (!Number.isInteger(config.lines) || config.lines < 1) throw configError("`lines` must be at least 1");
    path = config.path.startsWith("~/") ? join(homedir(), config.path.slice(2)) : config.path;
    name = basename(path) || "Text file";
    markdown = config.markdown ?? [".md", ".markdown"].includes(extname(path).toLowerCase());
  },
  // Checks the file's modified time and size, and reads it again only when one of them changed.
  render() {
    let key: string;
    let size: number;
    try {
      const st = statSync(path, { bigint: true });
      key = `${st.mtimeNs} ${st.size}`;
      size = Number(st.size);
    } catch (e) {
      throw upstreamError(`reading ${path}: ${(e as Error).message}`);
    }
    if (last !== null && seen === key) return last;
    if (size > MAX_BYTES) throw upstreamError(`${path} is over 1 MB`);
    let text: string;
    try {
      text = new TextDecoder().decode(readFileSync(path));
    } catch (e) {
      throw upstreamError(`reading ${path}: ${(e as Error).message}`);
    }
    last = view(text, name, markdown, config.lines);
    seen = key;
    return last;
  },
});
