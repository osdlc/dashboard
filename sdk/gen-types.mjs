#!/usr/bin/env node
// Generates the contract types in both SDKs from the host's JSON Schemas:
//
//   node sdk/gen-types.mjs                 # runs `cargo run -p dashboard -- schema ...`
//   node sdk/gen-types.mjs --schemas DIR   # reads DIR/{manifest,view,initialize-params}.json
//   node sdk/gen-types.mjs --check ...     # fails if the SDK files are out of date
//
// The types go between the GENERATED markers in sdk/typescript/dashboard-plugin.ts and
// sdk/python/dashboard_plugin.py; the rest of those files is written by hand.
// No packages: it handles only the schema shapes schemars produces for the contract.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SCHEMAS = ["manifest", "view", "initialize-params"];
const here = dirname(fileURLToPath(import.meta.url));
const BEGIN = "---- BEGIN GENERATED";
const END = "---- END GENERATED";

const args = process.argv.slice(2);
const check = args.includes("--check");
const dirAt = args.indexOf("--schemas");

function load(name) {
  if (dirAt >= 0) return JSON.parse(readFileSync(join(args[dirAt + 1], `${name}.json`), "utf8"));
  const out = execFileSync("cargo", ["run", "-q", "-p", "dashboard", "--", "schema", name], {
    cwd: join(here, ".."),
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
  return JSON.parse(out);
}

// ---- Collect named types ----

const pascal = (s) => s.replace(/(^|_)([a-z])/g, (_, __, c) => c.toUpperCase());

/** name -> schema, in a stable order. Tagged unions are split into one named type per variant. */
const types = new Map();

function add(name, schema) {
  const seen = types.get(name);
  if (seen && JSON.stringify(seen) !== JSON.stringify(schema)) {
    throw new Error(`two different schemas named ${name}`);
  }
  types.set(name, schema);
}

for (const which of SCHEMAS) {
  const root = load(which);
  const { $defs = {}, $schema, title, ...rest } = root;
  for (const [name, def] of Object.entries($defs)) add(name, def);
  add(title, rest);
}

/** The constant `type` / `mode` of a tagged union variant, if it is one. */
function tag(variant) {
  for (const [key, prop] of Object.entries(variant.properties ?? {})) {
    if (typeof prop.const === "string" && (variant.required ?? []).includes(key)) return prop.const;
  }
  return null;
}

/** For `Widget` with variant `meter_grid`: `MeterGridWidget`. */
const variantName = (union, variant) => pascal(tag(variant)) + union;

function isTaggedUnion(schema) {
  return Array.isArray(schema.oneOf) && schema.oneOf.every((v) => v.type === "object" && tag(v));
}

// ---- Shared walk: a type expression from a schema, in either language ----

function expr(schema, lang) {
  const ts = lang === "ts";
  const union = (parts) => {
    const unique = [...new Set(parts)];
    if (ts) return unique.join(" | ");
    if (unique.length === 1) return unique[0];
    const nullable = unique.includes("None");
    const rest = unique.filter((p) => p !== "None");
    const inner = rest.length === 1 ? rest[0] : `Union[${rest.join(", ")}]`;
    return nullable ? `Optional[${inner}]` : inner;
  };
  if (schema.$ref) {
    const name = schema.$ref.split("/").pop();
    return ts ? name : `"${name}"`;
  }
  if (schema.const !== undefined) {
    return ts ? JSON.stringify(schema.const) : `Literal[${JSON.stringify(schema.const)}]`;
  }
  if (schema.enum) {
    const values = schema.enum.map((v) => JSON.stringify(v));
    return ts ? values.join(" | ") : `Literal[${values.join(", ")}]`;
  }
  if (schema.anyOf || schema.oneOf) {
    return union((schema.anyOf ?? schema.oneOf).map((s) => expr(s, lang)));
  }
  const kinds = Array.isArray(schema.type) ? schema.type : schema.type ? [schema.type] : [];
  if (kinds.length === 0) return ts ? "unknown" : "Any";
  return union(
    kinds.map((kind) => {
      switch (kind) {
        case "null":
          return ts ? "null" : "None";
        case "string":
          return ts ? "string" : "str";
        case "boolean":
          return ts ? "boolean" : "bool";
        case "integer":
          return ts ? "number" : "int";
        case "number":
          return ts ? "number" : "float";
        case "array":
          if (schema.prefixItems) {
            const items = schema.prefixItems.map((s) => expr(s, lang));
            return ts ? `[${items.join(", ")}]` : `Tuple[${items.join(", ")}]`;
          }
          return ts ? `${wrap(expr(schema.items ?? {}, lang))}[]` : `List[${expr(schema.items ?? {}, lang)}]`;
        case "object":
          if (schema.additionalProperties && typeof schema.additionalProperties === "object") {
            const value = expr(schema.additionalProperties, lang);
            return ts ? `Record<string, ${value}>` : `Dict[str, ${value}]`;
          }
          return ts ? "Record<string, unknown>" : "Dict[str, Any]";
        default:
          throw new Error(`unknown type ${kind}`);
      }
    }),
  );
}

const wrap = (t) => (t.includes("|") ? `(${t})` : t);

const docLines = (text) => (text ?? "").split("\n").map((l) => l.trim()).filter(Boolean);

// ---- TypeScript ----

function tsDoc(text, indent) {
  const lines = docLines(text);
  if (lines.length === 0) return "";
  if (lines.length === 1) return `${indent}/** ${lines[0]} */\n`;
  return `${indent}/**\n${lines.map((l) => `${indent} * ${l}`).join("\n")}\n${indent} */\n`;
}

function tsObject(name, schema) {
  const required = new Set(schema.required ?? []);
  let out = tsDoc(schema.description, "") + `export interface ${name} {\n`;
  for (const [key, prop] of Object.entries(schema.properties ?? {})) {
    out += tsDoc(prop.description, "  ");
    out += `  ${key}${required.has(key) ? "" : "?"}: ${expr(prop, "ts")};\n`;
  }
  return out + "}\n";
}

function typescript() {
  let out = "";
  for (const [name, schema] of types) {
    if (isTaggedUnion(schema)) {
      const names = schema.oneOf.map((v) => variantName(name, v));
      out += tsDoc(schema.description, "") + `export type ${name} =\n${names.map((n) => `  | ${n}`).join("\n")};\n\n`;
      for (const v of schema.oneOf) out += tsObject(variantName(name, v), v) + "\n";
    } else if (schema.type === "object") {
      out += tsObject(name, schema) + "\n";
    } else {
      out += tsDoc(schema.description, "") + `export type ${name} = ${expr(schema, "ts")};\n\n`;
    }
  }
  return out;
}

// ---- Python ----

function pyComment(text, indent) {
  return docLines(text).map((l) => `${indent}# ${l}\n`).join("");
}

function pyObject(name, schema) {
  const required = new Set(schema.required ?? []);
  const props = Object.entries(schema.properties ?? {});
  const field = ([key, prop]) => pyComment(prop.description, "    ") + `    ${key}: ${expr(prop, "py")}\n`;
  const must = props.filter(([k]) => required.has(k));
  const may = props.filter(([k]) => !required.has(k));
  const head = pyComment(schema.description, "");
  // Python 3.9 has no NotRequired: required keys go in a base class, the rest in a
  // `total=False` subclass.
  if (may.length === 0) {
    return `${head}class ${name}(TypedDict):\n${must.map(field).join("") || "    pass\n"}`;
  }
  if (must.length === 0) {
    return `${head}class ${name}(TypedDict, total=False):\n${may.map(field).join("")}`;
  }
  return (
    `class _${name}Required(TypedDict):\n${must.map(field).join("")}\n\n` +
    `${head}class ${name}(_${name}Required, total=False):\n${may.map(field).join("")}`
  );
}

function python() {
  const aliases = [];
  const classes = [];
  const unions = [];
  for (const [name, schema] of types) {
    if (isTaggedUnion(schema)) {
      const names = schema.oneOf.map((v) => `"${variantName(name, v)}"`);
      unions.push(pyComment(schema.description, "") + `${name} = Union[${names.join(", ")}]\n`);
      for (const v of schema.oneOf) classes.push(pyObject(variantName(name, v), v));
    } else if (schema.type === "object") {
      classes.push(pyObject(name, schema));
    } else if (schema.enum) {
      aliases.push(pyComment(schema.description, "") + `${name} = ${expr(schema, "py")}\n`);
    } else {
      unions.push(pyComment(schema.description, "") + `${name} = ${expr(schema, "py")}\n`);
    }
  }
  return [aliases.join("\n"), classes.join("\n\n"), unions.join("\n")].join("\n\n");
}

// ---- Write ----

function splice(file, body) {
  const text = readFileSync(file, "utf8");
  const start = text.indexOf(BEGIN);
  const end = text.indexOf(END);
  if (start < 0 || end < 0) throw new Error(`${file}: missing the GENERATED markers`);
  const from = text.indexOf("\n", start) + 1;
  const to = text.lastIndexOf("\n", end) + 1;
  const next = text.slice(0, from) + "\n" + body.trim() + "\n\n" + text.slice(to);
  if (next === text) return true;
  if (check) {
    console.error(`${file} is out of date: run node sdk/gen-types.mjs`);
    return false;
  }
  writeFileSync(file, next);
  console.error(`wrote ${file}`);
  return true;
}

const ok = [
  splice(join(here, "typescript/dashboard-plugin.ts"), typescript()),
  splice(join(here, "python/dashboard_plugin.py"), python()),
].every(Boolean);
process.exit(ok ? 0 : 1);
