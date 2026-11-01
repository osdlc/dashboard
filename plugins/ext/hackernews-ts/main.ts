// Hacker News: the top stories (or new, best, Ask HN, Show HN, jobs) from the public HN
// Firebase API, as a list. Clicking a story opens its link, or its HN page when it has none.
//
// The TypeScript version of a native-vs-plugin benchmark widget. It follows the same rules as
// the native module (crates/modules-fun/src/hackernews.rs) and the Python plugin
// (plugins/ext/hackernews-py), and must show the same view for the same input.
//
//   dashboard plugin-check -- node plugins/ext/hackernews-ts/main.ts

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

import { configError, serve, upstreamError, withDefaults } from "./dashboard-plugin.ts";
import type { ListItem, View } from "./dashboard-plugin.ts";

const API = "https://hacker-news.firebaseio.com/v0";
/** Most stories shown. Each one is a request per refresh, so keep it small. */
const MAX_LIMIT = 30;

/** Per feed: the API's list of story ids, and the label in the summary ("10 top stories"). */
const FEEDS: Record<string, [string, string]> = {
  top: ["topstories", "top stories"],
  new: ["newstories", "new stories"],
  best: ["beststories", "best stories"],
  ask: ["askstories", "Ask HN posts"],
  show: ["showstories", "Show HN posts"],
  job: ["jobstories", "jobs"],
};

let config: { feed: string; limit: number; sample: string | null } = {
  feed: "top",
  limit: 10,
  sample: null,
};

/** A saved sample: a JSON array of HN items, in feed order. */
function readSample(path: string): unknown[] {
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    throw upstreamError(`reading ${path}: ${(e as Error).message}`);
  }
  let items: unknown;
  try {
    items = JSON.parse(text);
  } catch {
    items = null;
  }
  if (!Array.isArray(items)) throw upstreamError(`${path}: expected a JSON array of items`);
  return items;
}

async function getJson(url: string): Promise<unknown> {
  let resp: Response;
  try {
    resp = await fetch(url, { signal: AbortSignal.timeout(20_000) });
  } catch (e) {
    throw upstreamError(`${url}: ${(e as Error).message}`);
  }
  if (!resp.ok) {
    const body = (await resp.text().catch(() => "")).slice(0, 300);
    throw upstreamError(`HTTP ${resp.status}: ${body}`);
  }
  try {
    return await resp.json();
  } catch (e) {
    throw upstreamError(`${url}: ${(e as Error).message}`);
  }
}

/** The feed's first `limit` items, fetched at the same time, in feed order. */
async function fetchItems(feed: string, limit: number): Promise<unknown[]> {
  const ids = await getJson(`${API}/${FEEDS[feed][0]}.json`);
  if (!Array.isArray(ids)) throw upstreamError("the feed is not a list of ids");
  return Promise.all(ids.slice(0, limit).map((id) => getJson(`${API}/item/${id}.json`)));
}

/** One story, from an HN item. Deleted and dead items, and items without a title, give null. */
function story(item: unknown, now: number): ListItem | null {
  if (item === null || typeof item !== "object" || Array.isArray(item)) return null;
  const o = item as Record<string, unknown>;
  const text = (key: string) => (typeof o[key] === "string" ? (o[key] as string) : null);
  // Number.isInteger also takes 312.0, which Rust and Python reject. HN never
  // sends those, so the views still match.
  const int = (key: string) => (Number.isInteger(o[key]) ? (o[key] as number) : null);
  if (o.deleted === true || o.dead === true) return null;
  const title = text("title");
  if (title === null) return null;
  const url = text("url") || null;

  const sub: string[] = [];
  const score = int("score");
  if (score !== null) sub.push(plural(score, "point"));
  const comments = int("descendants");
  if (comments !== null) sub.push(plural(comments, "comment"));
  const by = text("by");
  if (by !== null) sub.push(`by ${by}`);
  const time = int("time");
  if (time !== null) sub.push(ago(now - time));
  const id = int("id");
  const link =
    url ?? (id !== null ? `https://news.ycombinator.com/item?id=${id}` : "https://news.ycombinator.com/");
  const host = url === null ? null : domain(url);
  const out: ListItem = { title, tags: host === null ? [] : [host], action: { type: "open_url", url: link } };
  if (sub.length > 0) out.subtitle = sub.join(" · ");
  return out;
}

/** The tile, built exactly like the native module's. `now` is in unix seconds. */
function view(items: unknown[], feed: string, limit: number, now: number): View {
  const stories = items
    .slice(0, limit)
    .map((item) => story(item, now))
    .filter((s): s is ListItem => s !== null);
  return {
    status: "ok",
    summary: `${stories.length} ${FEEDS[feed][1]}`,
    root: { type: "list", items: stories, empty_text: "no stories" },
  };
}

function plural(n: number, word: string): string {
  return n === 1 ? `1 ${word}` : `${n} ${word}s`;
}

/** "5m ago" style age from seconds. */
function ago(secs: number): string {
  const s = Math.max(0, secs);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86_400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86_400)}d ago`;
}

/** "https://www.Example.com:8080/a?b" -> "example.com". The same steps as the native module. */
function domain(url: string): string | null {
  const at = url.indexOf("://");
  const rest = at >= 0 ? url.slice(at + 3) : url;
  let host = rest.split(/[/?#]/)[0];
  host = host.slice(host.lastIndexOf("@") + 1);
  host = host.split(":")[0].toLowerCase();
  if (host.startsWith("www.")) host = host.slice(4);
  return host === "" ? null : host;
}

serve({
  manifest: {
    id: "hackernews-ts",
    name: "Hacker News (TS)",
    refresh: { mode: "poll", interval_ms: 300_000, min_interval_ms: 60_000 },
    layout: { size: "medium", priority: 30, min_size: "small" },
  },
  init(c) {
    config = withDefaults(c, config);
    if (typeof config.feed !== "string" || !(config.feed in FEEDS)) {
      throw configError(`feed: expected one of ${Object.keys(FEEDS).join(", ")}`);
    }
    if (!Number.isInteger(config.limit) || config.limit < 0) {
      throw configError("limit: expected a whole number, 0 or more");
    }
    if (config.limit > MAX_LIMIT) throw configError(`limit: at most ${MAX_LIMIT}`);
    if (config.sample !== null && typeof config.sample !== "string") {
      throw configError("sample: expected a file path");
    }
    if (typeof config.sample === "string" && config.sample.startsWith("~/")) {
      config.sample = join(homedir(), config.sample.slice(2));
    }
  },
  async render() {
    const items =
      config.sample !== null ? readSample(config.sample) : await fetchItems(config.feed, config.limit);
    return view(items, config.feed, config.limit, Math.floor(Date.now() / 1000));
  },
});
