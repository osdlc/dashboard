// Feed reader: the newest items of some RSS and Atom feeds, as one list. Reads RSS 2.0 (and
// 0.91), RSS 1.0 (RDF) and Atom 1.0. Items from every feed are sorted together, newest first;
// items without a date go last. Times are shown in UTC.
//
// The TypeScript version of a native-vs-plugin benchmark widget. It follows the same rules as
// the native module (crates/modules-fun/src/feedreader.rs) and the Python plugin
// (plugins/ext/feedreader-py), and must show the same view for the same input. Node has no XML
// parser, so this file has a small one.
//
//   dashboard plugin-check -- node plugins/ext/feedreader-ts/main.ts

import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

import { configError, serve, upstreamError, withDefaults } from "./dashboard-plugin.ts";
import type { ListItem, View } from "./dashboard-plugin.ts";

const XML_NS = "http://www.w3.org/XML/1998/namespace";
const RDF_NS = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const RSS1_NS = "http://purl.org/rss/1.0/";
const ATOM_NS = "http://www.w3.org/2005/Atom";
const DC_NS = "http://purl.org/dc/elements/1.1/";

let config: { feeds: string[]; max_items: number } = { feeds: [], max_items: 10 };

type Feed = {
  /** The feed's own title; empty when it has none. */
  title: string;
  items: Item[];
};

type Item = {
  title: string;
  /** Only http(s) links; others would not open. */
  link: string | null;
  /** Seconds since the epoch. */
  time: number | null;
};

/**
 * Why a feed could not be read. The tile shows the reason, which is the same in all three
 * versions, so it leaves out the library's error text.
 */
class Failure extends Error {}

const isHttp = (s: string) => s.startsWith("http://") || s.startsWith("https://");

/** Fetches a URL or reads a file. The body is read as UTF-8 whatever the feed declares. */
async function read(entry: string): Promise<Feed> {
  let bytes: Uint8Array;
  if (isHttp(entry)) {
    let resp: Response;
    try {
      resp = await fetch(entry, {
        headers: { "user-agent": "dashboard/0.1.0" },
        signal: AbortSignal.timeout(20_000),
      });
    } catch {
      throw new Failure("network error");
    }
    if (!resp.ok) {
      await resp.body?.cancel();
      throw new Failure(`HTTP ${resp.status}`);
    }
    try {
      bytes = new Uint8Array(await resp.arrayBuffer());
    } catch {
      throw new Failure("network error");
    }
  } else {
    const path = entry.startsWith("~/") ? join(homedir(), entry.slice(2)) : entry;
    try {
      bytes = await readFile(path);
    } catch {
      throw new Failure("could not read the file");
    }
  }
  return parseFeed(new TextDecoder("utf-8", { ignoreBOM: true }).decode(bytes));
}

// ---- A small XML parser ----
//
// Enough of XML 1.0 with namespaces for feeds, and as strict as roxmltree and Python's
// ElementTree where feeds go wrong: tags must match, entities must be known, there is one root,
// every character is an XML character, names are names and prefixes are bound. A DOCTYPE is
// skipped; one that declares entities is refused, as the other two versions do.

type Element = { ns: string; name: string; attrs: Map<string, string>; children: (Element | string)[] };

class XmlError extends Error {}

/** XML's own entities and character references; anything else is an error. */
function decodeXml(s: string): string {
  return s.replace(/&([^&;]*);|&/g, (m, name: string | undefined) => {
    if (name === undefined) throw new XmlError("bare &");
    switch (name) {
      case "lt":
        return "<";
      case "gt":
        return ">";
      case "amp":
        return "&";
      case "quot":
        return '"';
      case "apos":
        return "'";
    }
    const ref = /^#(?:([0-9]+)|x([0-9a-fA-F]+))$/.exec(name);
    if (ref === null) throw new XmlError(`unknown entity ${m}`);
    const n = ref[1] !== undefined ? Number(ref[1]) : parseInt(ref[2], 16);
    const ok =
      n === 0x9 || n === 0xa || n === 0xd || (n >= 0x20 && n <= 0xd7ff) || (n >= 0xe000 && n <= 0xfffd) ||
      (n >= 0x10000 && n <= 0x10ffff);
    if (!ok) throw new XmlError(`bad character reference ${m}`);
    return String.fromCodePoint(n);
  });
}

/** A character that XML does not allow anywhere in a document. */
const BAD_CHAR = /[^\t\n\r\x20-\uD7FF\uE000-\uFFFD\u{10000}-\u{10FFFF}]/u;
/** A name with at most one colon, from XML's NameStartChar and NameChar. */
const START = "A-Z_a-z\\u00C0-\\u00D6\\u00D8-\\u00F6\\u00F8-\\u02FF\\u0370-\\u037D\\u037F-\\u1FFF\\u200C\\u200D\\u2070-\\u218F\\u2C00-\\u2FEF\\u3001-\\uD7FF\\uF900-\\uFDCF\\uFDF0-\\uFFFD\\u{10000}-\\u{EFFFF}";
const NCNAME = `[${START}][${START}\\-.0-9\\u00B7\\u0300-\\u036F\\u203F\\u2040]*`;
const QNAME = `(?:${NCNAME}:)?${NCNAME}`;
// XML's white space is only these four (\r is gone by now); \s would take more.
const NAME = new RegExp(QNAME, "uy");
const ATTR = new RegExp(`[ \\t\\n]+(${QNAME})[ \\t\\n]*=[ \\t\\n]*(?:"([^"<]*)"|'([^'<]*)')`, "uy");
const TAG_END = /[ \t\n]*(\/?)>/y;
const CLOSE = new RegExp(`</(${QNAME})[ \\t\\n]*>`, "uy");
const PI_END = /(?:[ \t\n][^]*?)?\?>/y;
// The rest of `<?xml`: version, then encoding and standalone if given, in that order.
// Like expat and roxmltree, any version number is taken.
const DECL_END = new RegExp(
  [
    `[ \\t\\n]+version[ \\t\\n]*=[ \\t\\n]*(?:"[\\w.:-]+"|'[\\w.:-]+')`,
    `(?:[ \\t\\n]+encoding[ \\t\\n]*=[ \\t\\n]*(?:"[A-Za-z][\\w.-]*"|'[A-Za-z][\\w.-]*'))?`,
    `(?:[ \\t\\n]+standalone[ \\t\\n]*=[ \\t\\n]*(?:"(?:yes|no)"|'(?:yes|no)'))?`,
    `[ \\t\\n]*\\?>`,
  ].join(""),
  "y",
);

function parseXml(input: string): Element {
  if (BAD_CHAR.test(input)) throw new XmlError("not an XML character");
  const text = input.replace(/\r\n?/g, "\n");
  type Open = { el: Element; qname: string; scope: Map<string, string> };
  const stack: Open[] = [];
  let root: Element | null = null;
  let doctype = false;
  let i = 0;
  const until = (end: string, from: number): number => {
    const at = text.indexOf(end, from);
    if (at < 0) throw new XmlError(`no ${end}`);
    return at + end.length;
  };
  const sticky = (re: RegExp): RegExpExecArray | null => {
    re.lastIndex = i;
    const m = re.exec(text);
    if (m !== null) i = re.lastIndex;
    return m;
  };
  while (i < text.length) {
    const top = stack.at(-1);
    const lt = text.indexOf("<", i);
    const end = lt < 0 ? text.length : lt;
    if (end > i) {
      const raw = text.slice(i, end);
      if (raw.includes("]]>")) throw new XmlError("]]> in text");
      if (top) top.el.children.push(decodeXml(raw));
      else if (!/^[ \t\n]*$/.test(raw)) throw new XmlError("text outside the root");
      i = end;
    } else if (text.startsWith("<!--", i)) {
      const close = until("-->", i + 4);
      if (text.slice(i + 4, close - 3).includes("--") || text[close - 4] === "-") throw new XmlError("-- in a comment");
      i = close;
    } else if (text.startsWith("<![CDATA[", i)) {
      if (!top) throw new XmlError("CDATA outside the root");
      const close = until("]]>", i + 9);
      top.el.children.push(text.slice(i + 9, close - 3));
      i = close;
    } else if (text.startsWith("<?", i)) {
      // The XML declaration may only come first; other PIs are skipped.
      const at = i;
      i += 2;
      const target = sticky(NAME);
      if (target === null || target[0].includes(":")) throw new XmlError("bad PI");
      if (target[0].toLowerCase() === "xml" && (at !== 0 || target[0] !== "xml")) throw new XmlError("misplaced XML declaration");
      if (sticky(target[0] === "xml" ? DECL_END : PI_END) === null) throw new XmlError("bad PI");
    } else if (text.startsWith("<!DOCTYPE", i)) {
      if (top || root || doctype) throw new XmlError("misplaced DOCTYPE");
      doctype = true;
      // Skip it, with its internal subset in [...] if any.
      const at = i;
      let depth = 0;
      for (i += 9; i < text.length; i++) {
        const c = text[i];
        if (c === "[") depth++;
        else if (c === "]") depth--;
        else if (c === ">" && depth === 0) break;
      }
      if (i >= text.length) throw new XmlError("unclosed DOCTYPE");
      i++;
      const decls = text.slice(at, i).replace(/<!--[^]*?-->|<\?[^]*?\?>/g, " ");
      if (decls.includes("<!ENTITY")) throw new XmlError("declares entities");
    } else if (text.startsWith("</", i)) {
      const m = sticky(CLOSE);
      if (m === null || !top || m[1] !== top.qname) throw new XmlError("mismatched end tag");
      stack.pop();
    } else {
      i++;
      const name = sticky(NAME);
      if (name === null) throw new XmlError("bad tag");
      if (root && !top) throw new XmlError("a second root");
      const raw = new Map<string, string>();
      let attr: RegExpExecArray | null;
      while ((attr = sticky(ATTR)) !== null) {
        if (raw.has(attr[1])) throw new XmlError(`duplicate attribute ${attr[1]}`);
        raw.set(attr[1], decodeXml((attr[2] ?? attr[3]).replace(/[\t\n]/g, " ")));
      }
      const close = sticky(TAG_END);
      if (close === null) throw new XmlError("bad tag");
      // Namespaces: xmlns and xmlns:p declare them for this element and its children.
      let scope = top?.scope ?? new Map([["xml", XML_NS]]);
      const attrs = new Map<string, string>();
      for (const [k, v] of raw) {
        if (k === "xmlns" || k.startsWith("xmlns:")) {
          if (k === "xmlns:xml" && v !== XML_NS) throw new XmlError("xml prefix bound to another URI");
          if (scope === top?.scope) scope = new Map(scope);
          scope.set(k === "xmlns" ? "" : k.slice(6), v);
        } else {
          attrs.set(k, v);
        }
      }
      // Each attribute prefix must be bound, and no two attributes may have the same
      // namespace and name (p:x and q:x with p and q bound to one URI).
      const expanded = new Set<string>();
      for (const k of attrs.keys()) {
        const colon = k.indexOf(":");
        if (colon >= 0 && !scope.has(k.slice(0, colon))) throw new XmlError(`unbound prefix in ${k}`);
        const key = colon < 0 ? ` ${k}` : `${scope.get(k.slice(0, colon))} ${k.slice(colon + 1)}`;
        if (expanded.has(key)) throw new XmlError(`duplicate attribute ${k}`);
        expanded.add(key);
      }
      const qname = name[0];
      const colon = qname.indexOf(":");
      const prefix = colon < 0 ? "" : qname.slice(0, colon);
      const ns = scope.get(prefix) ?? "";
      if (prefix !== "" && !scope.has(prefix)) throw new XmlError(`unbound prefix ${prefix}`);
      const el: Element = { ns, name: qname.slice(colon + 1), attrs, children: [] };
      if (top) top.el.children.push(el);
      else root = el;
      if (close[1] !== "/") stack.push({ el, qname, scope });
    }
  }
  if (stack.length > 0 || root === null) throw new XmlError("unclosed element");
  return root;
}

// ---- Feeds ----

function children(el: Element, ns: string, name: string): Element[] {
  return el.children.filter((c): c is Element => typeof c !== "string" && c.ns === ns && c.name === name);
}

function child(el: Element, ns: string, name: string): Element | null {
  return children(el, ns, name)[0] ?? null;
}

/** All the text inside an element, without tags. */
function rawText(el: Element): string {
  return el.children.map((c) => (typeof c === "string" ? c : rawText(c))).join("");
}

/** XML whitespace. Only these count, in all three versions. */
const collapse = (s: string) => s.replace(/[ \t\r\n]+/g, " ").replace(/^ | $/g, "");

/** The text inside an element, with each run of whitespace made one space. */
const text = (el: Element) => collapse(rawText(el));

function childText(el: Element, ns: string, name: string): string | null {
  const c = child(el, ns, name);
  return c === null ? null : text(c);
}

/** Drops every `<...>`. A `<` with no `>` after it stays. */
const stripTags = (s: string) => s.replace(/<[^>]*>/g, "");

/**
 * Decodes &lt; &gt; &amp; &quot; &apos; &nbsp; and numeric references. Anything else, or a
 * number that is not a character, stays as it is.
 */
function decodeHtml(s: string): string {
  const named: Record<string, string> = { lt: "<", gt: ">", amp: "&", quot: '"', apos: "'", nbsp: " " };
  return s.replace(/&(lt|gt|amp|quot|apos|nbsp|#[0-9]+|#[xX][0-9a-fA-F]+);/g, (m, name: string) => {
    if (!name.startsWith("#")) return named[name];
    const n = name[1] === "x" || name[1] === "X" ? parseInt(name.slice(2), 16) : Number(name.slice(1));
    const ok = n > 0 && n <= 0x10ffff && !(n >= 0xd800 && n <= 0xdfff);
    return ok ? String.fromCodePoint(n) : m;
  });
}

/**
 * An Atom text: type="html" holds escaped HTML, so its tags are dropped and its entities
 * decoded. Plain text and xhtml are read as they are.
 */
function atomText(el: Element): string {
  return el.attrs.get("type") === "html" ? collapse(decodeHtml(stripTags(rawText(el)))) : text(el);
}

/**
 * Parses RSS 2.0 / 0.91 (<rss>), RSS 1.0 (<rdf:RDF>) or Atom (<feed>). Elements are matched by
 * namespace and name, so media:title or atom:link inside an RSS item are not taken for its
 * title or link.
 */
function parseFeed(input: string): Feed {
  let root: Element;
  try {
    root = parseXml(input.startsWith("﻿") ? input.slice(1) : input);
  } catch (e) {
    if (e instanceof XmlError) throw new Failure("not valid XML");
    throw e;
  }
  if (root.ns === "" && root.name === "rss") {
    const channel = child(root, "", "channel");
    if (channel === null) throw new Failure("not an RSS or Atom feed");
    return rss(channel, children(channel, "", "item"), "");
  }
  if (root.ns === RDF_NS && root.name === "RDF") {
    const channel = child(root, RSS1_NS, "channel");
    if (channel === null) throw new Failure("not an RSS or Atom feed");
    return rss(channel, children(root, RSS1_NS, "item"), RSS1_NS);
  }
  if (root.ns === ATOM_NS && root.name === "feed") return atom(root);
  throw new Failure("not an RSS or Atom feed");
}

function firstDate(candidates: (string | null)[]): number | null {
  for (const c of candidates) {
    const t = c === null ? null : parseDate(c);
    if (t !== null) return t;
  }
  return null;
}

function rss(channel: Element, items: Element[], ns: string): Feed {
  return {
    title: childText(channel, ns, "title") ?? "",
    items: items.map((item) => {
      let link = childText(item, ns, "link");
      if (link === null || !isHttp(link)) {
        // An RSS 2.0 guid is a permalink unless it says otherwise.
        const guid = child(item, ns, "guid");
        link = guid !== null && guid.attrs.get("isPermaLink") !== "false" ? text(guid) : null;
        if (link !== null && !isHttp(link)) link = null;
      }
      return {
        title: childText(item, ns, "title") ?? "",
        link,
        time: firstDate([childText(item, ns, "pubDate"), childText(item, DC_NS, "date")]),
      };
    }),
  };
}

function atom(feed: Element): Feed {
  const title = child(feed, ATOM_NS, "title");
  return {
    title: title === null ? "" : atomText(title),
    items: children(feed, ATOM_NS, "entry").map((entry) => {
      // The first link to a page (rel missing or "alternate") that opens.
      let link: string | null = null;
      for (const l of children(entry, ATOM_NS, "link")) {
        const rel = l.attrs.get("rel");
        const href = l.attrs.get("href");
        if ((rel === undefined || rel === "alternate") && href !== undefined) {
          const h = href.replace(/^[ \t\r\n]+|[ \t\r\n]+$/g, "");
          if (isHttp(h)) {
            link = h;
            break;
          }
        }
      }
      const t = child(entry, ATOM_NS, "title");
      return {
        title: t === null ? "" : atomText(t),
        link,
        time: firstDate([childText(entry, ATOM_NS, "published"), childText(entry, ATOM_NS, "updated")]),
      };
    }),
  };
}

// ---- Dates ----

const RFC3339 =
  /^(\d{4})-(\d{2})-(\d{2})(?:[Tt ](\d{2}):(\d{2})(?::(\d{2})(?:\.\d+)?)?)? ?([Zz]|[+-]\d{2}:?\d{2})?$/;
const RFC822 =
  /^(?:[A-Za-z]+, ?)?(\d{1,2}) ([A-Za-z]+) (\d{4}|\d{2}) (\d{1,2}):(\d{2})(?::(\d{2}))?(?: ?([A-Za-z]+|[+-]\d{4}))?$/;
const MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
/** The US zone names RFC 822 allows. GMT, UT, UTC, Z and any other name are UTC. */
const ZONES: Record<string, number> = { EST: -5, EDT: -4, CST: -6, CDT: -5, MST: -7, MDT: -6, PST: -8, PDT: -7 };

/**
 * Seconds since the epoch from an RFC 3339 or RFC 822 date, the two kinds feeds use (often the
 * wrong one for the format). Years before 1970 count as no date: some feeds send 0001-01-01
 * for "unset".
 */
function parseDate(s: string): number | null {
  const t = rfc3339(s) ?? rfc822(s);
  return t !== null && t >= 0 ? t : null;
}

function rfc3339(s: string): number | null {
  const m = RFC3339.exec(s);
  if (m === null) return null;
  let offset = 0;
  const z = m[7];
  if (z !== undefined && z !== "Z" && z !== "z") {
    const digits = z.slice(1).replace(":", "");
    offset = (z[0] === "-" ? -1 : 1) * (Number(digits.slice(0, 2)) * 3600 + Number(digits.slice(2)) * 60);
  }
  const n = (x: string | undefined) => (x === undefined ? 0 : Number(x));
  return epoch(n(m[1]), n(m[2]), n(m[3]), n(m[4]), n(m[5]), n(m[6]), offset);
}

function rfc822(s: string): number | null {
  const m = RFC822.exec(s);
  if (m === null) return null;
  const month = m[2].toLowerCase();
  const mo = month.length >= 3 ? MONTHS.indexOf(month.slice(0, 3)) + 1 : 0;
  if (mo === 0) return null;
  let y = Number(m[3]);
  if (m[3].length === 2) y += y < 50 ? 2000 : 1900;
  let offset = 0;
  const z = m[7];
  if (z !== undefined && (z[0] === "+" || z[0] === "-")) {
    const hhmm = Number(z.slice(1));
    offset = (z[0] === "-" ? -1 : 1) * (Math.floor(hhmm / 100) * 3600 + (hhmm % 100) * 60);
  } else if (z !== undefined) {
    offset = (ZONES[z.toUpperCase()] ?? 0) * 3600;
  }
  return epoch(y, mo, Number(m[1]), Number(m[4]), Number(m[5]), m[6] === undefined ? 0 : Number(m[6]), offset);
}

/** Seconds since the epoch from a local time and its offset from UTC, or null when a field is out of range. */
function epoch(y: number, mo: number, d: number, h: number, mi: number, s: number, offset: number): number | null {
  if (y < 1970 || mo < 1 || mo > 12 || d < 1 || d > 31 || h > 23 || mi > 59 || s > 60) return null;
  // Date.UTC rolls a day past the month's end into the next month, as the native days-from-civil does.
  return Date.UTC(y, mo - 1, d, h, mi, s) / 1000 - offset;
}

/** "2026-10-27 02:09" (UTC). */
const formatTime = (t: number) => new Date(t * 1000).toISOString().slice(0, 16).replace("T", " ");

// ---- The tile ----

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/**
 * The tile, from each configured feed (as written in the config) and what reading it gave.
 * Feeds that failed come first, then the newest `maxItems` items of the rest. Built exactly
 * like the native module's.
 */
function view(feeds: [string, Feed | Failure][], maxItems: number): View {
  const failed = feeds.filter((f): f is [string, Failure] => f[1] instanceof Failure);
  if (failed.length === feeds.length) {
    throw upstreamError(`could not read any feed: ${failed[0][0]}: ${failed[0][1].message}`);
  }
  let items: [string, Item][] = [];
  for (const [entry, feed] of feeds) {
    if (feed instanceof Failure) continue;
    const name = feed.title === "" ? entry : feed.title;
    for (const item of feed.items) items.push([name, item]);
  }
  // Newest first, undated last; a stable sort keeps feed order for ties.
  items.sort(([, a], [, b]) => Number(a.time === null) - Number(b.time === null) || (b.time ?? 0) - (a.time ?? 0));
  items = items.slice(0, maxItems);

  const list: ListItem[] = failed.map(([entry, why]) => ({
    title: `Could not read ${entry}`,
    subtitle: why.message,
    status: "error",
  }));
  for (const [feed, item] of items) {
    list.push({
      title: item.title === "" ? "(no title)" : item.title,
      subtitle: item.time === null ? feed : `${feed} · ${formatTime(item.time)}`,
      action: item.link === null ? null : { type: "open_url", url: item.link },
    });
  }
  let summary = `${plural(items.length, "item")} from ${plural(feeds.length - failed.length, "feed")}`;
  if (failed.length > 0) summary += ` · ${failed.length} failed`;
  return {
    status: failed.length > 0 ? "warn" : "ok",
    summary,
    root: { type: "list", items: list, empty_text: "No items in these feeds." },
  };
}

serve({
  manifest: {
    id: "feedreader-ts",
    name: "Feed reader (TS)",
    refresh: { mode: "poll", interval_ms: 900_000, min_interval_ms: 60_000 },
    layout: { size: "medium", priority: 40, min_size: "small" },
  },
  init(c) {
    config = withDefaults(c, config);
    if (!Array.isArray(config.feeds) || !config.feeds.every((f) => typeof f === "string")) {
      throw configError("feeds must be a list of URLs");
    }
    if (config.feeds.length === 0) throw configError("list at least one feed URL under feeds");
    if (!Number.isInteger(config.max_items) || config.max_items < 0) {
      throw configError("max_items must be a whole number");
    }
    if (config.max_items === 0) throw configError("max_items must be at least 1");
  },
  async render() {
    const feeds = await Promise.all(
      config.feeds.map(async (entry): Promise<[string, Feed | Failure]> => {
        try {
          return [entry, await read(entry)];
        } catch (e) {
          if (e instanceof Failure) return [entry, e];
          throw e;
        }
      }),
    );
    return view(feeds, config.max_items);
  },
});
