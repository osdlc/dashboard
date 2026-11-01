# feedreader test data

The `*.xml` feeds and their `*.view.json` views are shared by the native `feedreader`
module and its TypeScript and Python versions in `plugins/ext/`.

`xml-cases.json` is a table of odd XML and what each version must say about it: `ok`,
`not valid XML` or `not an RSS or Atom feed`. The native module checks it in its own tests,
and `crates/host/tests/bench_feedreader.rs` runs both plugins over it.

## Known differences

The three versions show the same view for the same input, with these exceptions. All are
unusual input that real feeds do not use.

| Input | Native | TypeScript | Python |
| --- | --- | --- | --- |
| `<?xml version="1.0" standalone="maybe"?>` | ok | not valid XML | not valid XML |
| `<?xml version="1.0" encoding="a b"?>` (a bad encoding name) | ok | not valid XML | not valid XML |
| `xmlns:xmlns="u"` | ok | ok | not valid XML |
| `xmlns:p=""` (a prefix bound to no URI) | ok | ok | not valid XML |

The table was checked with Python 3.9 (expat 2.2.8, macOS's `python3`) and Python 3.14.
