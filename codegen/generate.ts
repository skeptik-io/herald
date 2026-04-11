#!/usr/bin/env tsx
// Main entry point for Herald admin SDK code generation.
// Reads openapi.yaml, parses to IR, runs per-language emitters.

import { resolve, dirname } from "node:path";
import { existsSync } from "node:fs";
import { parseSpec } from "./parser.js";
import { emitTypeScript } from "./emitters/typescript.js";
import { emitGo } from "./emitters/go.js";
import { emitPython } from "./emitters/python.js";
import { emitRuby } from "./emitters/ruby.js";
import { emitPhp } from "./emitters/php.js";
import { emitCsharp } from "./emitters/csharp.js";

const ROOT = resolve(dirname(import.meta.url.replace("file://", "")), "..");
const SPEC_PATH = resolve(ROOT, "openapi.yaml");

// Emitter registry — add new languages here
const EMITTERS: Record<string, { emit: (sdk: ReturnType<typeof parseSpec>, outDir: string) => void; outDir: string }> = {
  typescript: { emit: emitTypeScript, outDir: resolve(ROOT, "herald-admin-typescript") },
  go: { emit: emitGo, outDir: resolve(ROOT, "herald-admin-go") },
  python: { emit: emitPython, outDir: resolve(ROOT, "herald-admin-python") },
  ruby: { emit: emitRuby, outDir: resolve(ROOT, "herald-admin-ruby") },
  php: { emit: emitPhp, outDir: resolve(ROOT, "herald-admin-php") },
  csharp: { emit: emitCsharp, outDir: resolve(ROOT, "herald-admin-csharp") },
};

function main() {
  const args = process.argv.slice(2);
  const langFilter = args.find((a) => a.startsWith("--lang="))?.split("=")[1];
  const dryRun = args.includes("--dry-run");

  if (!existsSync(SPEC_PATH)) {
    console.error(`OpenAPI spec not found: ${SPEC_PATH}`);
    process.exit(1);
  }

  console.log(`Parsing ${SPEC_PATH}...`);
  const sdk = parseSpec(SPEC_PATH);

  console.log(`  Namespaces: ${sdk.namespaces.map((n) => n.id).join(", ")}`);
  console.log(`  Root methods: ${sdk.rootMethods.map((m) => m.name).join(", ")}`);
  for (const ns of sdk.namespaces) {
    console.log(`  ${ns.className}: ${ns.methods.map((m) => m.name).join(", ")}`);
  }

  if (dryRun) {
    console.log("\nDry run — no files written.");
    return;
  }

  const languages = langFilter ? [langFilter] : Object.keys(EMITTERS);
  for (const lang of languages) {
    const emitter = EMITTERS[lang];
    if (!emitter) {
      console.error(`Unknown language: ${lang}`);
      process.exit(1);
    }
    console.log(`\nGenerating ${lang}...`);
    emitter.emit(sdk, emitter.outDir);
  }

  console.log("\nDone.");
}

main();
