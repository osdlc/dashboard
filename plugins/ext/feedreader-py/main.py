#!/usr/bin/env python3
"""Feed reader: the newest items of some RSS and Atom feeds, as one list. Reads RSS 2.0 (and
0.91), RSS 1.0 (RDF) and Atom 1.0. Items from every feed are sorted together, newest first;
items without a date go last. Times are shown in UTC.

The Python version of a native-vs-plugin benchmark widget. It follows the same rules as the
native module (crates/modules-fun/src/feedreader.rs) and the TypeScript plugin
(plugins/ext/feedreader-ts), and must show the same view for the same input. It parses with
the standard library's ElementTree.

    dashboard plugin-check -- python3 plugins/ext/feedreader-py/main.py
"""

import http.client
import os
import re
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
from xml.parsers import expat
from concurrent.futures import ThreadPoolExecutor
from typing import List, Optional, Tuple, TypedDict, Union

from dashboard_plugin import ListItem, View, config_error, serve, upstream_error, with_defaults

RDF_NS = "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
RSS1_NS = "http://purl.org/rss/1.0/"
ATOM_NS = "http://www.w3.org/2005/Atom"
DC_NS = "http://purl.org/dc/elements/1.1/"

config = {"feeds": [], "max_items": 10}


class Item(TypedDict):
    title: str
    link: Optional[str]  # only http(s) links; others would not open
    time: Optional[int]  # seconds since the epoch


class Feed(TypedDict):
    title: str  # the feed's own title; empty when it has none
    items: List[Item]


class Failure(Exception):
    """Why a feed could not be read. The tile shows the reason, which is the same in all three
    versions, so it leaves out the library's error text."""


def is_http(s: str) -> bool:
    return s.startswith("http://") or s.startswith("https://")


def read(entry: str) -> Feed:
    """Fetches a URL or reads a file. The body is read as UTF-8 whatever the feed declares."""
    if is_http(entry):
        req = urllib.request.Request(entry, headers={"User-Agent": "dashboard/0.1.0"})
        try:
            with urllib.request.urlopen(req, timeout=20) as resp:
                data = resp.read()
        except urllib.error.HTTPError as e:
            raise Failure(f"HTTP {e.code}")
        except (OSError, ValueError, http.client.HTTPException):
            raise Failure("network error")
    else:
        try:
            with open(os.path.expanduser(entry) if entry.startswith("~/") else entry, "rb") as f:
                data = f.read()
        except OSError:
            raise Failure("could not read the file")
    return parse_feed(data.decode("utf-8", "replace"))


# ---- Feeds ----
#
# ElementTree names elements "{namespace}name", so matching on that keeps media:title or
# atom:link inside an RSS item from being taken for its title or link.


def tag(ns: str, name: str) -> str:
    return f"{{{ns}}}{name}" if ns else name


def children(el: ET.Element, ns: str, name: str) -> List[ET.Element]:
    return el.findall(tag(ns, name))


def child(el: ET.Element, ns: str, name: str) -> Optional[ET.Element]:
    return el.find(tag(ns, name))


def raw_text(el: ET.Element) -> str:
    """All the text inside an element, without tags."""
    return "".join(el.itertext())


def collapse(s: str) -> str:
    """Each run of XML whitespace made one space. Only these four count, in all three versions."""
    return re.sub(r"[ \t\r\n]+", " ", s).strip(" ")


def text(el: ET.Element) -> str:
    return collapse(raw_text(el))


def child_text(el: ET.Element, ns: str, name: str) -> Optional[str]:
    c = child(el, ns, name)
    return None if c is None else text(c)


def strip_tags(s: str) -> str:
    """Drops every <...>. A < with no > after it stays."""
    return re.sub(r"<[^>]*>", "", s)


NAMED = {"lt": "<", "gt": ">", "amp": "&", "quot": '"', "apos": "'", "nbsp": " "}
HTML_ENTITY = re.compile(r"&(lt|gt|amp|quot|apos|nbsp|#[0-9]+|#[xX][0-9a-fA-F]+);", re.ASCII)


def decode_html(s: str) -> str:
    """Decodes &lt; &gt; &amp; &quot; &apos; &nbsp; and numeric references. Anything else, or a
    number that is not a character, stays as it is."""

    def one(m: "re.Match[str]") -> str:
        name = m.group(1)
        if not name.startswith("#"):
            return NAMED[name]
        n = int(name[2:], 16) if name[1] in "xX" else int(name[1:])
        ok = 0 < n <= 0x10FFFF and not 0xD800 <= n <= 0xDFFF
        return chr(n) if ok else m.group(0)

    return HTML_ENTITY.sub(one, s)


def atom_text(el: ET.Element) -> str:
    """An Atom text: type="html" holds escaped HTML, so its tags are dropped and its entities
    decoded. Plain text and xhtml are read as they are."""
    if el.get("type") == "html":
        return collapse(decode_html(strip_tags(raw_text(el))))
    return text(el)


class _Stop(Exception):
    pass


def declares_entities(data: str) -> bool:
    """Whether the DOCTYPE declares an entity. All three versions refuse such a feed as not
    valid XML: expat before 2.4 (macOS's python3 has 2.2.8) expands nested entities with no
    limit, the "billion laughs", and the TypeScript parser skips the DTD. Only the part before
    the root is parsed here, and nothing is expanded."""
    found = False

    def entity(*_args) -> None:
        nonlocal found
        found = True
        raise _Stop

    def start(*_args) -> None:
        raise _Stop

    parser = expat.ParserCreate()
    parser.EntityDeclHandler = entity
    parser.StartElementHandler = start
    try:
        parser.Parse(data, True)
    except _Stop:
        pass
    return found


def parse_feed(data: str) -> Feed:
    """Parses RSS 2.0 / 0.91 (<rss>), RSS 1.0 (<rdf:RDF>) or Atom (<feed>)."""
    if data.startswith("﻿"):
        data = data[1:]
    try:
        if declares_entities(data):
            raise Failure("not valid XML")
        root = ET.fromstring(data)
    except (ET.ParseError, expat.ExpatError):
        raise Failure("not valid XML")
    if root.tag == "rss":
        channel = child(root, "", "channel")
        if channel is None:
            raise Failure("not an RSS or Atom feed")
        return rss(channel, children(channel, "", "item"), "")
    if root.tag == tag(RDF_NS, "RDF"):
        channel = child(root, RSS1_NS, "channel")
        if channel is None:
            raise Failure("not an RSS or Atom feed")
        return rss(channel, children(root, RSS1_NS, "item"), RSS1_NS)
    if root.tag == tag(ATOM_NS, "feed"):
        return atom(root)
    raise Failure("not an RSS or Atom feed")


def first_date(candidates: List[Optional[str]]) -> Optional[int]:
    for c in candidates:
        t = None if c is None else parse_date(c)
        if t is not None:
            return t
    return None


def rss(channel: ET.Element, items: List[ET.Element], ns: str) -> Feed:
    out: List[Item] = []
    for item in items:
        link = child_text(item, ns, "link")
        if link is None or not is_http(link):
            # An RSS 2.0 guid is a permalink unless it says otherwise.
            guid = child(item, ns, "guid")
            link = text(guid) if guid is not None and guid.get("isPermaLink") != "false" else None
            if link is not None and not is_http(link):
                link = None
        out.append(
            {
                "title": child_text(item, ns, "title") or "",
                "link": link,
                "time": first_date([child_text(item, ns, "pubDate"), child_text(item, DC_NS, "date")]),
            }
        )
    return {"title": child_text(channel, ns, "title") or "", "items": out}


def atom(feed: ET.Element) -> Feed:
    out: List[Item] = []
    for entry in children(feed, ATOM_NS, "entry"):
        # The first link to a page (rel missing or "alternate") that opens.
        link = None
        for l in children(entry, ATOM_NS, "link"):
            href = l.get("href")
            if l.get("rel") in (None, "alternate") and href is not None and is_http(href.strip(" \t\r\n")):
                link = href.strip(" \t\r\n")
                break
        title = child(entry, ATOM_NS, "title")
        out.append(
            {
                "title": "" if title is None else atom_text(title),
                "link": link,
                "time": first_date([child_text(entry, ATOM_NS, "published"), child_text(entry, ATOM_NS, "updated")]),
            }
        )
    title = child(feed, ATOM_NS, "title")
    return {"title": "" if title is None else atom_text(title), "items": out}


# ---- Dates ----

RFC3339 = re.compile(
    r"(\d{4})-(\d{2})-(\d{2})(?:[Tt ](\d{2}):(\d{2})(?::(\d{2})(?:\.\d+)?)?)? ?([Zz]|[+-]\d{2}:?\d{2})?", re.ASCII
)
RFC822 = re.compile(
    r"(?:[A-Za-z]+, ?)?(\d{1,2}) ([A-Za-z]+) (\d{4}|\d{2}) (\d{1,2}):(\d{2})(?::(\d{2}))?(?: ?([A-Za-z]+|[+-]\d{4}))?",
    re.ASCII,
)
MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]
# The US zone names RFC 822 allows. GMT, UT, UTC, Z and any other name are UTC.
ZONES = {"EST": -5, "EDT": -4, "CST": -6, "CDT": -5, "MST": -7, "MDT": -6, "PST": -8, "PDT": -7}


def parse_date(s: str) -> Optional[int]:
    """Seconds since the epoch from an RFC 3339 or RFC 822 date, the two kinds feeds use (often
    the wrong one for the format). Years before 1970 count as no date: some feeds send
    0001-01-01 for "unset"."""
    t = rfc3339(s)
    if t is None:
        t = rfc822(s)
    return t if t is not None and t >= 0 else None


def rfc3339(s: str) -> Optional[int]:
    m = RFC3339.fullmatch(s)
    if m is None:
        return None
    offset = 0
    z = m.group(7)
    if z is not None and z not in ("Z", "z"):
        digits = z[1:].replace(":", "")
        offset = (-1 if z[0] == "-" else 1) * (int(digits[:2]) * 3600 + int(digits[2:]) * 60)
    y, mo, d, h, mi, sec = (int(g) if g is not None else 0 for g in m.groups()[:6])
    return epoch(y, mo, d, h, mi, sec, offset)


def rfc822(s: str) -> Optional[int]:
    m = RFC822.fullmatch(s)
    if m is None:
        return None
    month = m.group(2).lower()
    if len(month) < 3 or month[:3] not in MONTHS:
        return None
    mo = MONTHS.index(month[:3]) + 1
    y = int(m.group(3))
    if len(m.group(3)) == 2:
        y += 2000 if y < 50 else 1900
    offset = 0
    z = m.group(7)
    if z is not None and z[0] in "+-":
        hhmm = int(z[1:])
        offset = (-1 if z[0] == "-" else 1) * (hhmm // 100 * 3600 + hhmm % 100 * 60)
    elif z is not None:
        offset = ZONES.get(z.upper(), 0) * 3600
    sec = int(m.group(6)) if m.group(6) is not None else 0
    return epoch(y, mo, int(m.group(1)), int(m.group(4)), int(m.group(5)), sec, offset)


def epoch(y: int, mo: int, d: int, h: int, mi: int, s: int, offset: int) -> Optional[int]:
    """Seconds since the epoch from a local time and its offset from UTC, or None when a field
    is out of range."""
    if y < 1970 or not 1 <= mo <= 12 or not 1 <= d <= 31 or h > 23 or mi > 59 or s > 60:
        return None
    # Days from civil date (Howard Hinnant's algorithm).
    y = y - 1 if mo <= 2 else y
    era = y // 400
    yoe = y - era * 400
    doy = (153 * (mo - 3 if mo > 2 else mo + 9) + 2) // 5 + d - 1
    doe = yoe * 365 + yoe // 4 - yoe // 100 + doy
    days = era * 146097 + doe - 719468
    return days * 86400 + h * 3600 + mi * 60 + s - offset


def format_time(t: int) -> str:
    """'2026-10-27 02:09' (UTC)."""
    days, secs = divmod(t, 86400)
    # Civil date from days (Howard Hinnant's algorithm).
    z = days + 719468
    era = z // 146097
    doe = z - era * 146097
    yoe = (doe - doe // 1460 + doe // 36524 - doe // 146096) // 365
    doy = doe - (365 * yoe + yoe // 4 - yoe // 100)
    mp = (5 * doy + 2) // 153
    d = doy - (153 * mp + 2) // 5 + 1
    m = mp + 3 if mp < 10 else mp - 9
    y = yoe + era * 400 + (1 if m <= 2 else 0)
    return f"{y:04d}-{m:02d}-{d:02d} {secs // 3600:02d}:{secs // 60 % 60:02d}"


# ---- The tile ----


def plural(n: int, word: str) -> str:
    return f"{n} {word}" if n == 1 else f"{n} {word}s"


def view(feeds: List[Tuple[str, Union[Feed, Failure]]], max_items: int) -> View:
    """The tile, from each configured feed (as written in the config) and what reading it gave.
    Feeds that failed come first, then the newest `max_items` items of the rest. Built exactly
    like the native module's."""
    failed = [(entry, f) for entry, f in feeds if isinstance(f, Failure)]
    if len(failed) == len(feeds):
        raise upstream_error(f"could not read any feed: {failed[0][0]}: {failed[0][1]}")
    items: List[Tuple[str, Item]] = []
    for entry, feed in feeds:
        if isinstance(feed, Failure):
            continue
        name = feed["title"] or entry
        items.extend((name, item) for item in feed["items"])
    # Newest first, undated last; a stable sort keeps feed order for ties.
    items.sort(key=lambda fi: (fi[1]["time"] is None, -(fi[1]["time"] or 0)))
    items = items[:max_items]

    rows: List[ListItem] = [
        {"title": f"Could not read {entry}", "subtitle": str(why), "status": "error"} for entry, why in failed
    ]
    for feed_name, item in items:
        t = item["time"]
        rows.append(
            {
                "title": item["title"] or "(no title)",
                "subtitle": feed_name if t is None else f"{feed_name} · {format_time(t)}",
                "action": None if item["link"] is None else {"type": "open_url", "url": item["link"]},
            }
        )
    summary = f"{plural(len(items), 'item')} from {plural(len(feeds) - len(failed), 'feed')}"
    if failed:
        summary += f" · {len(failed)} failed"
    return {
        "status": "warn" if failed else "ok",
        "summary": summary,
        "root": {"type": "list", "items": rows, "empty_text": "No items in these feeds."},
    }


def init(c):
    config.update(with_defaults(c, config))
    feeds, max_items = config["feeds"], config["max_items"]
    if not isinstance(feeds, list) or not all(isinstance(f, str) for f in feeds):
        raise config_error("feeds must be a list of URLs")
    if not feeds:
        raise config_error("list at least one feed URL under feeds")
    # 3.0 is refused too, as the native version's serde does.
    if isinstance(max_items, bool) or not isinstance(max_items, int) or max_items < 0:
        raise config_error("max_items must be a whole number")
    if max_items == 0:
        raise config_error("max_items must be at least 1")


def read_one(entry: str) -> Union[Feed, Failure]:
    try:
        return read(entry)
    except Failure as e:
        return e


def render() -> View:
    with ThreadPoolExecutor(max_workers=min(8, len(config["feeds"]))) as pool:
        results = list(pool.map(read_one, config["feeds"]))
    return view(list(zip(config["feeds"], results)), config["max_items"])


serve(
    manifest={
        "id": "feedreader-py",
        "name": "Feed reader (Python)",
        "refresh": {"mode": "poll", "interval_ms": 900000, "min_interval_ms": 60000},
        "layout": {"size": "medium", "priority": 40, "min_size": "small"},
    },
    init=init,
    render=render,
)
