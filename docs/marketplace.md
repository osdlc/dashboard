# Installing plugin packages

`dashboard plugin install` adds a [plugin package](plugins.md#package-it) to
`<data home>/dashboard/plugins/<id>/` (`~/.local/share/dashboard/plugins/` by default, moved by
`XDG_DATA_HOME`), where every config finds it. Packages come from a marketplace index, a
`.tar.gz` file, a URL or a folder.

```sh
dashboard plugin install clock-py --index marketplace/index.json   # by ID, from an index
dashboard plugin install ./clock-py-0.1.0.tar.gz                    # a file
dashboard plugin install https://example.com/x.tar.gz --sha256 <hash>  # a URL: the hash is required
dashboard plugin install plugins/ext/clock-py                       # a folder
dashboard plugin list   [--index ...]      # installed, and what the index offers
dashboard plugin update [ids] [--index ...]  # newer versions from the index
dashboard plugin remove clock-py
```

Instead of `--index`, set `DASHBOARD_PLUGIN_INDEX` to a path or URL. An index must be
`https://`, a `file://` URL or a path; plain `http://` is refused, because the index supplies
the hashes, and an index changed on the way could change them too. A single package URL may
be `http://`, since you give its hash with `--sha256`.

## What install does

1. Reads the archive and checks its sha256 against the index, or against `--sha256`. A
   mismatch stops here; nothing is written. A file or folder with no hash given is installed
   with its hash shown, not checked.
2. Unpacks it into a staging folder next to the plugins folder. Only plain files and folders
   are taken. A path that would land outside the folder (`../x`, `/etc/x`), a link or a device
   file stops the install (checked with `path-guard`). There are size limits: 64 MiB packed,
   256 MiB unpacked, 10,000 entries.
3. Reads the package's `dashboard-plugin.yaml` and checks it as the dashboard does. From an
   index, its ID and version must match the index entry.
4. Shows what it runs, the programs it needs (and whether they are on `PATH`), the
   environment variables it reads and the hosts it calls, then asks. `--yes` skips the
   question.
5. Moves the package into place, replacing an older version, and records where it came from
   in `plugins/installed.json` (an atomic, compare-and-swap write with `etag-write`).

Then add it to a config with `package: <id>`.

`update` installs newer versions of the packages named, or of every package installed with
`install`. Each uses the index it was installed from, unless `--index` names another.
`DASHBOARD_PLUGIN_INDEX` does not replace that index. A package installed from a file, URL or
folder is skipped, unless it is named and an index is given (by `--index` or the variable).
One package failing does not stop the others; `update` reports the failures at the end.

Versions are ordered as in semver: dotted numbers, then a release after its pre-releases
(`1.0.0-beta.1` < `1.0.0-beta.2` < `1.0.0`). `+build` is ignored. A version in another shape
is never taken as an update; install it again to change it.

If install fails after moving the new version in (for example, another install changed
`installed.json` at the same moment), it puts the old version back, so the folder and the
record still agree.
`remove` deletes the folder. It only touches the plugins folder in the data home, never
packages kept next to a config.

**Safety, plainly:** a plugin runs as you, with your permissions. The hash proves you got the
file the index pinned; it says nothing about what the plugin does. Read what install shows
before saying yes.

## The index

`index.json` lists packages, each pinned by URL and sha256. `dashboard schema index` prints its
JSON Schema.

```json
{
  "format": 1,
  "plugins": [
    {
      "id": "clock-py",
      "version": "0.1.0",
      "description": "The local time in one or more time zones...",
      "tags": ["time", "local"],
      "url": "packages/clock-py-0.1.0.tar.gz",
      "sha256": "lowercase hex sha256 of the .tar.gz",
      "runtime": "python3>=3.9",
      "requirements": { "binaries": [], "env": [], "hosts": [] }
    }
  ]
}
```

- `url` is an `https://` or `file://` URL, or a path relative to the index file. An index on
  the web may only point at `https://` URLs, never at local files.
- `runtime` and `requirements` repeat the package's `runtime`, `binaries`, `env` and `hosts`,
  so they can be shown before downloading.
- `name`, `size`, `config_schema` and `demo` are optional and copied from the package's
  `dashboard-plugin.yaml` by `plugin pack`. The web editor's catalog uses them to show a package
  before it is installed; an entry without `demo` is not shown there.
- Unknown keys are an error.

Each time `plugin install`, `list` or `update` reads an index, it keeps a copy at
`<data home>/dashboard/marketplace/index.json`. The web editor's "Add component" list and
`dashboard widgets` show the packages in that copy that are not installed yet.

The archive is a `.tar.gz` holding the package as `<id>/...` (or its files at the top).

## Making an index

`dashboard plugin pack <folders...> --out <dir>` packs each package folder into
`<dir>/packages/<id>-<version>.tar.gz` and adds or replaces its entry in `<dir>/index.json`.
Packing leaves out dotfiles, `__pycache__`, `*.pyc` and `node_modules`, refuses links, and
gives the same bytes for the same files (sorted, no times or owners), so the hash is stable.

`scripts/build-marketplace.sh` builds the sample marketplace from every package in
`plugins/ext` into `marketplace/` (build output, not checked in):

```sh
scripts/build-marketplace.sh
dashboard plugin list --index marketplace/index.json
```

## Not yet

- The shared index (`osdlc/dashboard-registry`), where "approved" means a reviewed pull
  request to it.
- An install button in the web catalog.
- Checking a runtime's version, not only that the program is on `PATH`.
