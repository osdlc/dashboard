// Running Docker containers, read with the `docker` CLI: name, image, status and, from
// `docker stats`, CPU and memory. An error tile when the Docker daemon is not running.
//
// The TypeScript copy of the built-in `docker` module (crates/modules-ops/src/docker.rs), for
// the native-vs-plugin benchmark. It builds the same view from the same docker output.
//
//   dashboard plugin-check -- node plugins/ext/docker-ts/main.ts
//
//   modules:
//     docker-ts:
//       plugin: [node, plugins/ext/docker-ts/main.ts]
//       config: { stats: true, limit: 20 }

import { execFile } from "node:child_process";
import { serve, withDefaults } from "./dashboard-plugin.ts";
import type { Column, Row, View } from "./dashboard-plugin.ts";

/**
 * How long one docker command may take, in ms. A render runs up to two (ps,
 * then stats), so 2 x 10 s stays under the plugin host's 30 s limit
 * (crates/host/src/plugin.rs). Keep in step with the built-in module.
 */
const TIMEOUT = 10_000;

let config = { stats: true, limit: 20, docker: "docker" };

/** One line of `docker ps --format '{{json .}}'`. */
type Container = { ID: string; Names: string; Image: string; State: string; Status: string };
/** One line of `docker stats --no-stream --format '{{json .}}'`. */
type Stats = { ID: string; CPUPerc: string; MemUsage: string };

/** Why a docker command gave no output. */
type Failure = { kind: "not_found" } | { kind: "timed_out" } | { kind: "failed"; line: string };

/** Runs `docker <args>` and returns its stdout, or why it failed. */
function docker(args: string[]): Promise<string | Failure> {
  return new Promise((resolve) => {
    execFile(config.docker, args, { timeout: TIMEOUT, maxBuffer: 16 << 20 }, (err, stdout, stderr) => {
      if (!err) return resolve(stdout);
      const e = err as NodeJS.ErrnoException & { killed?: boolean };
      if (e.code === "ENOENT") return resolve({ kind: "not_found" });
      if (e.killed) return resolve({ kind: "timed_out" });
      // Spawn errors (EACCES, ...) have a string code; exit codes are numbers.
      if (typeof e.code === "string") return resolve({ kind: "failed", line: `running docker: ${e.message}` });
      resolve({ kind: "failed", line: lastLine(String(stderr), args[0]) });
    });
  });
}

/** The last non-empty line, or a stand-in when there is none. */
function lastLine(stderr: string, command: string): string {
  const lines = stderr.split("\n").map((l) => l.trim()).filter((l) => l !== "");
  return lines.length > 0 ? lines[lines.length - 1] : `docker ${command} failed`;
}

/** What the docker CLI says when it can't reach the daemon. */
function daemonDown(stderr: string): boolean {
  const s = stderr.toLowerCase();
  return (
    s.includes("cannot connect to the docker daemon") ||
    s.includes("failed to connect to the docker api") ||
    s.includes("is the docker daemon running")
  );
}

/** The error tile. A view, not an error, so the tile doesn't keep the last containers. */
function failureView(f: Failure): View {
  let summary: string;
  let text: string;
  if (f.kind === "not_found") {
    summary = "docker not found";
    text = "The docker command was not found. Install Docker, or set `docker:` in the config to the docker program.";
  } else if (f.kind === "timed_out") {
    summary = "docker timed out";
    text = "docker did not answer.";
  } else {
    summary = daemonDown(f.line) ? "Docker is not running" : "docker failed";
    text = f.line;
  }
  return { status: "error", summary, root: { type: "text", text, markdown: false } };
}

/** One JSON object per line; lines that aren't one are skipped. */
function parseLines<T>(text: string, keys: string[]): T[] {
  const out: T[] = [];
  for (const line of text.split("\n")) {
    try {
      const v = JSON.parse(line.trim());
      if (v && typeof v === "object" && keys.every((k) => typeof v[k] === "string")) out.push(v);
    } catch {
      // not JSON: skip it
    }
  }
  return out;
}

const unhealthy = (c: Container) => c.Status.includes("(unhealthy)");
const restarting = (c: Container) => c.State === "restarting";
const short = (id: string) => id.slice(0, 12);

/** The tile. `stats` is null when CPU and memory are off or unavailable. */
function view(containers: Container[], stats: Stats[] | null, limit: number): View {
  const sick = containers.filter(unhealthy).length;
  const looping = containers.filter(restarting).length;
  const summary = [containers.length === 1 ? "1 container" : `${containers.length} containers`];
  if (sick > 0) summary.push(`${sick} unhealthy`);
  if (looping > 0) summary.push(`${looping} restarting`);
  const status = sick + looping > 0 ? "warn" : "ok";
  if (containers.length === 0) {
    return {
      status,
      summary: "no containers running",
      root: { type: "text", text: "No containers running.", markdown: false },
    };
  }

  const columns: Column[] = [
    { key: "name", label: "Name" },
    { key: "image", label: "Image" },
    { key: "status", label: "Status" },
  ];
  if (stats) columns.push({ key: "cpu", label: "CPU" }, { key: "mem", label: "Memory" });
  const rows: Row[] = containers.slice(0, limit).map((c) => {
    const cells: Record<string, string> = { name: c.Names, image: c.Image, status: c.Status };
    if (stats) {
      // Matched on the short ID: ps and stats may print it at different lengths.
      const s = stats.find((s) => short(s.ID) === short(c.ID));
      cells.cpu = s ? s.CPUPerc : "-";
      // "10.2MiB / 7.66GiB" -> "10.2MiB"
      cells.mem = s ? s.MemUsage.split(" / ")[0] : "-";
    }
    return { cells, status: unhealthy(c) || restarting(c) ? "warn" : "ok" };
  });
  return { status, summary: summary.join(" · "), root: { type: "table", columns, rows } };
}

serve({
  manifest: {
    id: "docker-ts",
    name: "Docker (TypeScript)",
    refresh: { mode: "poll", interval_ms: 15000, min_interval_ms: 5000 },
    layout: { size: "wide", priority: 20, min_size: "small" },
    config_schema: {
      type: "object",
      additionalProperties: false,
      properties: {
        stats: { type: "boolean", description: "Show CPU and memory, from `docker stats` (about 2 s per refresh)." },
        limit: { type: "integer", minimum: 0, description: "Rows shown." },
        docker: { type: "string", description: "The docker program, e.g. a full path." },
      },
    },
  },
  init(c) {
    config = withDefaults(c, config);
  },
  async render(): Promise<View> {
    const ps = await docker(["ps", "--format", "{{json .}}"]);
    if (typeof ps !== "string") return failureView(ps);
    const containers = parseLines<Container>(ps, ["ID", "Names", "Image", "State", "Status"]);
    let stats: Stats[] | null = null;
    if (config.stats && containers.length > 0) {
      // Without stats the tile still shows the containers.
      const out = await docker(["stats", "--no-stream", "--format", "{{json .}}"]);
      if (typeof out === "string") stats = parseLines<Stats>(out, ["ID", "CPUPerc", "MemUsage"]);
    }
    return view(containers, stats, config.limit);
  },
});
