# Sample marketplace

`scripts/build-marketplace.sh` fills this folder: every package in `plugins/ext` as
`packages/<id>-<version>.tar.gz`, and `index.json` listing them with their sha256. Both are
build output and not checked in. Try it:

```sh
scripts/build-marketplace.sh
dashboard plugin list --index marketplace/index.json
dashboard plugin install clock-py --index marketplace/index.json
```

See [docs/marketplace.md](../docs/marketplace.md).
