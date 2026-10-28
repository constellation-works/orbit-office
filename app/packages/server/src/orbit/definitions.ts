/**
 * Which routines and auto-tasks the app shows, and the paged routine listing.
 * Shared by the Routines and Calendar features, so both list the same set.
 *
 * Display membership comes from the shipped component trees, never workspace
 * state or guessed prefixes. Read once when a feature starts.
 * App bundles carry the same manifest/definition trees under app/plugins/.
 */
import { existsSync, readFileSync, realpathSync } from "node:fs";
import { resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import type { OrbitToolClient } from "./client.js";

export interface DefinitionNames {
  routines: ReadonlySet<string>;
  auto_tasks: ReadonlySet<string>;
}

export function defaultPluginRoots(): readonly string[] {
  // From `packages/server/{src,dist}/orbit/` up to the repository root.
  const source = new URL("../../../../../", import.meta.url);
  if (existsSync(new URL("plugin/.orbit-plugin/plugin.yaml", source))) {
    return ["plugin/.orbit-plugin/", "gateway/plugins/gateway/.orbit-plugin/"].map(path => fileURLToPath(new URL(path, source)));
  }
  const bundle = new URL("../../../../plugins/", import.meta.url);
  return ["office/.orbit-plugin/", "gateway/.orbit-plugin/"].map(path => fileURLToPath(new URL(path, bundle)));
}

// These repository-owned YAML fields use literal paths and identifier scalars,
// with flow or block path lists. Reject unsupported syntax rather than guessing
// membership. This is deliberately not a parser for arbitrary workspace YAML.
function scalar(value: string): string {
  const match = /^(?:"([A-Za-z0-9_./-]+)"|'([A-Za-z0-9_./-]+)'|([A-Za-z0-9_./-]+))(?:\s+#.*)?$/.exec(value.trim());
  const result = match?.[1] ?? match?.[2] ?? match?.[3];
  if (!result) throw new Error("invalid shipped definition scalar");
  return result;
}

function lines(text: string): string[] {
  return text.split(/\r?\n/).filter(line => line.trim() && !line.trimStart().startsWith("#"));
}

function block(source: readonly string[], key: string, indent: number): string[] {
  const prefix = `${" ".repeat(indent)}${key}:`;
  const starts = source.flatMap((line, index) => line.startsWith(prefix) ? [index] : []);
  if (starts.length > 1) throw new Error("duplicate shipped manifest field");
  const start = starts[0] ?? -1;
  if (start < 0) return [];
  if (source[start]?.slice(prefix.length).trim().replace(/^#.*$/, "") !== "") throw new Error("unsupported shipped manifest block");
  const result: string[] = [];
  for (const line of source.slice(start + 1)) {
    if ((/^ */.exec(line)?.[0].length ?? 0) <= indent) break;
    result.push(line);
  }
  return result;
}

function paths(source: readonly string[], kind: keyof DefinitionNames): string[] {
  const start = source.findIndex(line => new RegExp(`^    ${kind}:`).test(line));
  if (start < 0) return [];
  let value = source[start]?.slice(`    ${kind}:`.length).trim() ?? "";
  for (const line of source.slice(start + 1)) {
    if (!line.startsWith("      ")) break;
    value += `\n${line.trim()}`;
  }
  value = value.trim();
  if (value.startsWith("[") && value.endsWith("]")) {
    const content = value.slice(1, -1).trim();
    return content ? content.split(",").map(scalar) : [];
  }
  return value.split("\n").filter(Boolean).map(line => {
    if (!line.startsWith("- ")) throw new Error("invalid shipped definition list");
    return scalar(line.slice(2));
  });
}

export function readDefinitionNames(pluginRoots: readonly string[]): DefinitionNames {
  const names = { routines: new Set<string>(), auto_tasks: new Set<string>() };
  for (const path of pluginRoots) {
    const root = realpathSync(path);
    const manifest = lines(readFileSync(resolve(root, "plugin.yaml"), "utf8"));
    if (!manifest.some(line => /^spec:\s*(?:#.*)?$/.test(line))) throw new Error("missing shipped plugin spec");
    const definitions = block(block(manifest, "spec", 0), "definitions", 2);
    for (const kind of ["routines", "auto_tasks"] as const) {
      for (const relative of paths(definitions, kind)) {
        const file = realpathSync(resolve(root, relative));
        if (!file.startsWith(root + sep)) throw new Error("shipped definition escapes plugin tree");
        const declarations = lines(readFileSync(file, "utf8")).filter(line => line.startsWith("name:"));
        if (declarations.length !== 1) throw new Error("missing or duplicate shipped definition name");
        const name = scalar(declarations[0]?.slice(5) ?? "");
        if (!/^[A-Za-z0-9][A-Za-z0-9_-]*$/.test(name)) throw new Error("invalid shipped definition name");
        names[kind].add(name);
      }
    }
  }
  return names;
}

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** Every scheduled routine, page by page; a malformed page fails the whole listing. */
export async function routineListing(client: Pick<OrbitToolClient, "callTool">): Promise<Record<string, unknown>[]> {
  const rows: Record<string, unknown>[] = [];
  let offset = 0;
  for (;;) {
    const page = await client.callTool("orbit.routine.control", { action: "list", offset, limit: 50 });
    if (!object(page) || !Array.isArray(page.items) || !object(page.pagination) ||
        !Array.isArray(page.notes) || page.notes.length > 0) throw new Error("invalid routine listing");
    for (const row of page.items) {
      if (!object(row) || typeof row.name !== "string" || typeof row.description !== "string" ||
          typeof row.enabled !== "boolean" || typeof row.effective !== "boolean" ||
          typeof row.target !== "string" || !object(row.schedule)) throw new Error("invalid routine listing");
      rows.push(row);
    }
    const next = page.pagination.next_offset;
    if (next === null) return rows;
    if (typeof next !== "number" || !Number.isSafeInteger(next) || next !== offset + page.items.length || next <= offset) {
      throw new Error("invalid routine pagination");
    }
    offset = next;
    if (offset >= 10000) throw new Error("routine listing exceeds snapshot limit");
  }
}
