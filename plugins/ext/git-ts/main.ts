// Local git repositories: branch, commits ahead of and behind the upstream, and uncommitted
// changes. Runs `git status --porcelain=v2 --branch` once per repository.
//
// The TypeScript copy of the native `git` module (crates/modules-dev/src/git.rs), for the
// native-vs-plugin benchmark. It shows the same view for the same repositories, so keep the
// three (Rust, TypeScript, Python) in step. Node 24 runs it as is.
//
//   dashboard plugin-check --plugin-config '{"repos": ["."]}' -- node plugins/ext/git-ts/main.ts
//
//   modules:
//     git-ts:
//       plugin: [node, plugins/ext/git-ts/main.ts]
//       config: { repos: [~/dev/api, ~/dev/web] }

import { spawnSync } from "node:child_process";
import { realpathSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { basename } from "node:path";
import { configError, serve, upstreamError, withDefaults } from "./dashboard-plugin.ts";
import type { Row, Status, View } from "./dashboard-plugin.ts";

let config = { repos: [] as string[] };

type Upstream = { kind: "none" } | { kind: "gone" } | { kind: "tracking"; ahead: number; behind: number };

/** One repository, as `git status --porcelain=v2 --branch` describes it. */
type Repo = { branch: string; upstream: Upstream; changed: number };

/** A repository's row: what git said, or why it couldn't be read. */
type Found = { name: string; repo: Repo } | { name: string; error: string };

/** `~/x` as `$HOME/x`; other paths as they are. */
function expandHome(path: string): string {
  return path.startsWith("~/") ? homedir() + path.slice(1) : path;
}

/** The folder's name, after following links. */
function repoName(path: string): string {
  let real = path;
  try {
    real = realpathSync(path);
  } catch {
    // missing: use the path as given
  }
  return basename(real) || real;
}

function status(path: string): Repo | string {
  // Any stat error (missing, or a parent we can't read) counts as "not a folder",
  // like Path::is_dir and os.path.isdir.
  let isDir = false;
  try {
    isDir = statSync(path, { throwIfNoEntry: false })?.isDirectory() ?? false;
  } catch {
    // unreadable: not a folder
  }
  if (!isDir) return "not a folder";
  const out = spawnSync("git", ["-C", path, "status", "--porcelain=v2", "--branch"], {
    encoding: "utf8",
    // Many untracked files can print more than Node's 1 MB default.
    maxBuffer: Number.POSITIVE_INFINITY,
    stdio: ["ignore", "pipe", "pipe"],
    // Don't take the index lock: the user's own git commands come first.
    env: { ...process.env, GIT_OPTIONAL_LOCKS: "0" },
  });
  if (out.error) {
    const missing = (out.error as { code?: string }).code === "ENOENT";
    throw upstreamError(missing ? "running git (is it installed?)" : `running git: ${out.error.message}`);
  }
  if (out.status !== 0) return gitError(out.stderr);
  return parse(out.stdout);
}

/** The first line of git's complaint, without "fatal: ". */
function gitError(stderr: string): string {
  let line = (stderr.split("\n")[0] ?? "").trim();
  if (line.startsWith("fatal: ")) line = line.slice("fatal: ".length);
  return line || "git status failed";
}

/** Reads `git status --porcelain=v2 --branch` output. */
export function parse(out: string): Repo {
  const repo: Repo = { branch: "", upstream: { kind: "none" }, changed: 0 };
  let hasUpstream = false;
  for (const line of out.split("\n")) {
    if (line.startsWith("# branch.head ")) {
      const head = line.slice("# branch.head ".length);
      repo.branch = head === "(detached)" ? "detached" : head;
    } else if (line.startsWith("# branch.upstream ")) {
      hasUpstream = true;
    } else if (line.startsWith("# branch.ab ")) {
      const [ahead, behind] = line
        .slice("# branch.ab ".length)
        .split(/\s+/)
        .map((n) => Number.parseInt(n.replace(/^[+-]+/, ""), 10) || 0);
      repo.upstream = { kind: "tracking", ahead: ahead ?? 0, behind: behind ?? 0 };
    } else if (!line.startsWith("#") && !line.startsWith("!") && line !== "") {
      repo.changed += 1;
    }
  }
  // An upstream with no ahead/behind line is one whose branch is gone.
  if (hasUpstream && repo.upstream.kind === "none") repo.upstream = { kind: "gone" };
  return repo;
}

function changes(n: number): string {
  return n === 0 ? "clean" : `${n} changed`;
}

function sync(u: Upstream): string {
  if (u.kind === "none") return "no upstream";
  if (u.kind === "gone") return "upstream gone";
  if (u.ahead === 0 && u.behind === 0) return "in sync";
  const parts = [];
  if (u.ahead > 0) parts.push(`↑${u.ahead}`);
  if (u.behind > 0) parts.push(`↓${u.behind}`);
  return parts.join(" ");
}

const RANK: Record<Status, number> = { error: 3, warn: 2, ok: 1, unknown: 0 };

export function view(found: Found[]): View {
  let dirty = 0;
  let unpushed = 0;
  let behind = 0;
  let worst: Status = "ok";
  const rows: Row[] = found.map((f) => {
    let cells: Record<string, string>;
    let status: Status;
    if ("repo" in f) {
      const r = f.repo;
      const [ahead, back] = r.upstream.kind === "tracking" ? [r.upstream.ahead, r.upstream.behind] : [0, 0];
      if (r.changed > 0) dirty += 1;
      if (ahead > 0) unpushed += 1;
      if (back > 0) behind += 1;
      status = r.changed > 0 || back > 0 ? "warn" : "ok";
      cells = { repo: f.name, branch: r.branch, changes: changes(r.changed), sync: sync(r.upstream) };
    } else {
      status = "error";
      cells = { repo: f.name, branch: "-", changes: f.error, sync: "-" };
    }
    if (RANK[status] > RANK[worst]) worst = status;
    return { cells, status };
  });
  return {
    status: worst,
    summary: `${found.length} ${found.length === 1 ? "repo" : "repos"} · ${dirty} dirty · ${unpushed} unpushed · ${behind} behind`,
    root: {
      type: "table",
      columns: [
        { key: "repo", label: "Repository" },
        { key: "branch", label: "Branch" },
        { key: "changes", label: "Changes" },
        { key: "sync", label: "Upstream" },
      ],
      rows,
    },
  };
}

serve({
  manifest: {
    id: "git-ts",
    name: "Git (TypeScript)",
    refresh: { mode: "poll", interval_ms: 30000, min_interval_ms: 5000 },
    layout: { size: "medium", priority: 20, min_size: "small" },
  },
  init(c) {
    config = withDefaults(c, config);
    if (!Array.isArray(config.repos) || config.repos.length === 0) {
      throw configError("the git module needs `config: { repos: [path, ...] }`");
    }
    if (!config.repos.every((r) => typeof r === "string")) {
      throw configError("`repos` must be a list of paths");
    }
  },
  render() {
    return view(
      config.repos.map((p) => {
        const path = expandHome(p);
        const repo = status(path);
        return typeof repo === "string" ? { name: repoName(path), error: repo } : { name: repoName(path), repo };
      }),
    );
  },
});
