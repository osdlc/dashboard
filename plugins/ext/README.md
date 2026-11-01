# Plugins in other languages

Widgets written as TypeScript or Python plugins, each a [plugin package](../../docs/plugins.md#package-it):
a folder with a `dashboard-plugin.yaml`, the plugin, and a copy of its SDK file
(`dashboard-plugin.ts` or `dashboard_plugin.py`, from `sdk/`). A test checks that the copies
match the SDK; after an SDK change, copy it again.

| Package | What |
|---|---|
| `clock-ts`, `clock-py` | The built-in `clock` module in TypeScript and Python |
| `textfile-ts`, `textfile-py` | The built-in `textfile` module in TypeScript and Python |
| `git-ts`, `git-py` | The built-in `git` module in TypeScript and Python |
| `docker-ts`, `docker-py` | The built-in `docker` module in TypeScript and Python |
| `hackernews-ts`, `hackernews-py` | The built-in `hackernews` module in TypeScript and Python |
| `weather-ts`, `weather-py` | The built-in `weather` module in TypeScript and Python |
| `feedreader-ts`, `feedreader-py` | The built-in `feedreader` module in TypeScript and Python |
| `battery-ts`, `battery-py` | The built-in `battery` module in TypeScript and Python |
| `datadog` | Datadog monitors: alert, warn and no-data counts, and the monitors that need a look |
| `newrelic` | New Relic alerts: open violations, critical first |
| `pagerduty` | PagerDuty: open incidents, and who is first on call |
| `opsgenie` | Opsgenie alerts: open alerts, unacknowledged and highest priority first |
| `sentry` | Sentry: unresolved issues for an organization or one project |

## Benchmark copies

These are the native-vs-plugin benchmark copies. Each shows the same view as the built-in
module for the same input; the tests that check this are:

| Widget | Test |
|---|---|
| clock | `crates/host/tests/bench_clock.rs`, on the samples in `crates/modules-fun/testdata/clock/` |
| textfile | `crates/host/tests/bench_textfile.rs`, on the cases in `crates/modules-system/testdata/textfile/cases.json` |
| git | `crates/host/tests/bench_git.rs` and `crates/cli/tests/bench_git.rs` |
| docker | `crates/modules-ops/tests/docker_same_view.rs` and `crates/host/tests/bench_docker.rs` |
| hackernews | `crates/modules-fun/tests/hackernews_same_view.rs` |
| weather | `crates/modules-personal/tests/weather_same_view.rs` and `crates/host/tests/bench_weather.rs` |
| feedreader | `crates/host/tests/bench_feedreader.rs`, on the samples in `crates/modules-fun/testdata/feedreader/` |
| battery | `crates/host/tests/bench_battery.rs`, on the samples in `crates/modules-system/testdata/battery/` |

`examples/bench.yaml` shows them side by side.

Check one with:

```sh
dashboard plugin-check -- node plugins/ext/clock-ts/main.ts
dashboard plugin-check -- python3 plugins/ext/clock-py/main.py
dashboard plugin-check --plugin-config '{"path": "README.md"}' -- node plugins/ext/textfile-ts/main.ts
dashboard plugin-check --plugin-config '{"path": "README.md"}' -- python3 plugins/ext/textfile-py/main.py
dashboard plugin-check --plugin-config '{"repos": ["."]}' -- node plugins/ext/git-ts/main.ts
dashboard plugin-check --plugin-config '{"repos": ["."]}' -- python3 plugins/ext/git-py/git_py.py
dashboard plugin-check -- node plugins/ext/docker-ts/main.ts
dashboard plugin-check -- python3 plugins/ext/docker-py/main.py
dashboard plugin-check -- node plugins/ext/hackernews-ts/main.ts
dashboard plugin-check -- python3 plugins/ext/hackernews-py/main.py
dashboard plugin-check --plugin-config '{"place":"Berlin"}' -- node plugins/ext/weather-ts/main.ts
dashboard plugin-check --plugin-config '{"place":"Berlin"}' -- python3 plugins/ext/weather-py/main.py
dashboard plugin-check --plugin-config '{"feeds": ["crates/modules-fun/testdata/feedreader/rss2.xml"]}' -- node plugins/ext/feedreader-ts/main.ts
dashboard plugin-check --plugin-config '{"feeds": ["crates/modules-fun/testdata/feedreader/rss2.xml"]}' -- python3 plugins/ext/feedreader-py/main.py
dashboard plugin-check -- node plugins/ext/battery-ts/main.ts
dashboard plugin-check -- python3 plugins/ext/battery-py/main.py
```

## Use one

To use one by its ID (`package: textfile-ts`), copy its folder into the plugins folder of your data
home (`~/.local/share/dashboard/plugins/`).

## Healthchecks.io (token API)

Widgets for services that need an account. They are plugins until someone runs them live
(see the rule in [docs/modules.md](../../docs/modules.md#built-in-or-plugin)). Each was built
from the service's API docs and a hand-written sample reply, and is marked "untested live". Run
with `--sample`, each shows its sample instead of calling the API; that is what the tests and the
catalog demo use.

| Package | What | Test |
|---|---|---|
| `healthchecks` (Python) | Healthchecks.io checks: up, late and down counts, then the checks, down ones first | `crates/cli/tests/ext_healthchecks.rs` |

```sh
dashboard plugin-check -- python3 plugins/ext/healthchecks/main.py --sample
HEALTHCHECKS_API_KEY=... dashboard plugin-check -- python3 plugins/ext/healthchecks/main.py
```

## Ops widgets (token APIs)

`datadog`, `newrelic`, `pagerduty`, `opsgenie` and `sentry` call a service that needs a token.
Per the rule in [docs/modules.md](../../docs/modules.md#built-in-or-plugin), they are plugins,
in Python (3.9 or newer, standard library only). They were built from each service's public API
docs and **have not been run against the real service** ("untested live"). Each has:

- a hand-written sample response in `sample/`;
- a `demo:` view in its `dashboard-plugin.yaml` with the same made-up data;
- tests in `crates/cli/tests/ext_ops.rs` that run it through the real plugin host against a local
  server answering with the sample, and check the request (path, query, auth header) and the view.

Tokens come from an environment variable (`token_env`, e.g. `PAGERDUTY_TOKEN`) or a command that
prints them (`token_command`), never from the config file. Datadog needs two keys, so it also has
`app_key_env` / `app_key_command`. Each package also carries `ops.py`, the helpers they share
(tokens, HTTP with a timeout, ages); a test checks the copies match `datadog/ops.py`.

`examples/widgets/w-ops-a.yaml` shows all five with `demo: true`. Check one with:

```sh
dashboard plugin-check --plugin-config '{"org": "acme"}' -- python3 plugins/ext/sentry/main.py
```

## Code review and CI (token APIs)

These call APIs that need an account, so they are plugins, in Python (see the rule in
[docs/modules.md](../../docs/modules.md#built-in-or-plugin)). None has been run against the
real service with a token: each is built from the API docs and a hand-written `sample.json`,
and says "Untested live" in its description.

| Package | What | Token variable |
|---|---|---|
| `gitlab` | Merge requests and issues, one list per query | `GITLAB_TOKEN` |
| `gitlab-todos` | Your pending GitLab to-do items | `GITLAB_TOKEN` |
| `gerrit` | Changes per search query, with Code-Review and Verified votes | `GERRIT_HTTP_PASSWORD` |
| `azure-devops` | Work items from a WIQL query and active pull requests | `AZURE_DEVOPS_EXT_PAT` |
| `circleci` | The latest pipelines per project, with their workflows | `CIRCLECI_TOKEN` |

Each reads its token from that variable, another one (`token_env`) or a command
(`token_command`), never from the config file. Each folder has:
- `main.py`, the plugin; `main.py --sample` reads `sample.json` instead of the network, with
  ages as of a fixed time;
- `sample.json`, the API responses, keyed by request path;
- `test_main.py`, parse tests on the sample (`python3 -m unittest` in the folder);
- `dashboard-plugin.yaml`, whose `demo:` is exactly the `--sample` view.

Each package is self-contained, so `token()`, `ago()` and `parse_time()` are copied into every
`main.py`. A fix to one of them has to be made in all five. The token is read once per render, so
a `token_command` runs once per refresh, not once per request.

`crates/cli/tests/ext_dev_a.rs` runs the parse tests and checks the demo views, config errors
and the missing-token message. `examples/widgets/w-dev-a.yaml` shows all five, as demo tiles
and running on their samples. Gerrit's anonymous mode (`auth: none`) was checked once against
a public server.

```sh
dashboard plugin-check -- python3 plugins/ext/gitlab/main.py --sample
dashboard plugin-check --plugin-config '{"url": "https://review.opendev.org", "auth": "none", "sections": [{"label": "Nova", "query": "status:open project:openstack/nova"}]}' -- python3 plugins/ext/gerrit/main.py
```

## Builds and tasks (token APIs)

Widgets for services that need an account. They have no built-in version: per the rule in
[docs/modules.md](../../docs/modules.md#built-in-or-plugin) they ship as Python
plugins (standard library only), not built in. None has been run against the real service yet:
each is built from the service's API docs and a hand-written sample response, and says
"untested live" in its description.

| Package | What | Token |
|---|---|---|
| `buildkite` | Recent builds for an organization or one pipeline | `BUILDKITE_TOKEN` |
| `jenkins` | Jobs on a server or one view, with each job's last build | `JENKINS_TOKEN` (with `user:`) |
| `asana` | Your open tasks in a workspace, or every open task in a project | `ASANA_TOKEN` |
| `todoist` | Tasks matching a filter, by default `today \| overdue` | `TODOIST_TOKEN` |
| `trello` | Open cards on a board, soonest due first, with a bar per list | `TRELLO_KEY` and `TRELLO_TOKEN` |

Each folder has:
- `main.py`: the plugin.
- `apitoken.py`: reads the token from `token_command` (a list, such as
  `[op, read, "op://dev/buildkite/token"]`) or the `token_env` variable, never from the config
  file. A command runs once per plugin process. Trello reads its key the same way, from
  `key_command` or `key_env`. The file is the same in every folder; a test checks the copies.
- `view.py`: turns the API response into the view, with no I/O.
- `sample.json`: a hand-written API response. `python3 main.py --sample` renders it instead of
  calling the service, and that render is the package's `demo:` view.
- `test_view.py`: parse tests on the sample (`python3 -m unittest` in the folder).

`crates/cli/tests/ext_dev_b.rs` checks all five: the package loads, the `--sample` render is
the `demo:` view, the parse tests pass, a missing token names its variable, a failing
`token_command` is reported without its output, and a bad config is a config error.
`examples/widgets/w-dev-b.yaml` shows them, as demo views and running on their
samples.

```sh
dashboard plugin-check -- python3 plugins/ext/buildkite/main.py --sample
```

## Errors, uptime, hosting and tickets (token APIs)

Widgets for services that need an account, written in Python as the
[built-in-or-plugin rule](../../docs/modules.md#built-in-or-plugin) says. None has been run
against the real service yet, so each says "untested live". Each package has:

- `main.py`: config, the API call and the stdio loop;
- `view.py`: turns the API reply into the tile, with no I/O;
- `sample.json`: a hand-written API reply, built from the service's API docs;
- `test_view.py`: parse tests on `sample.json` (`python3 -m unittest`);
- `apitoken.py`: the token from `token_env` or `token_command`. A command runs once and its
  output is kept until the API answers 401. This is the same file the other token-API
  packages use; every copy must match.
- `token_api.py`: HTTP and age helpers. Every package in this table has the same copy.

| Package | What | Token variable |
|---|---|---|
| `rollbar` | A Rollbar project's active items | `ROLLBAR_TOKEN` |
| `airbrake` | An Airbrake project's unresolved error groups | `AIRBRAKE_USER_KEY` |
| `uptimerobot` | UptimeRobot monitors: up or down, uptime, response time | `UPTIMEROBOT_API_KEY` |
| `digitalocean` | DigitalOcean droplets and their monthly cost | `DIGITALOCEAN_TOKEN` |
| `zendesk` | Zendesk tickets from a search | `ZENDESK_API_TOKEN` |

With `--sample`, a plugin shows its `sample.json` instead of calling the API, and its ages are
measured from a fixed time, so the view is always the same. That view is the package's `demo:`.
`crates/cli/tests/ext_python_plugins.rs` checks that the two match, runs the parse tests, and
checks the config and missing-token errors. `examples/widgets/w-ops-b.yaml` shows them all.

```sh
dashboard plugin-check -- python3 plugins/ext/rollbar/main.py --sample
```
