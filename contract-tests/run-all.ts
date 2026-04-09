/**
 * Contract Test Orchestrator
 *
 * 1. Starts a Herald server
 * 2. Runs contract tests for all four admin SDKs
 * 3. Reports aggregate results
 */

import { spawn } from "node:child_process";
import { join } from "node:path";
import { startServer, stopServer, serverUrl, API_TOKEN, ADMIN_TOKEN } from "./harness.js";
import { run as runTypeScript } from "./run-typescript.js";

const specPath = join(import.meta.dirname, "spec", "spec.json");

interface SuiteResult {
  name: string;
  passed: number;
  failed: number;
}

function runSubprocess(name: string, cmd: string, args: string[], cwd: string): Promise<SuiteResult> {
  return new Promise((resolve) => {
    const env = {
      ...process.env,
      HERALD_URL: serverUrl(),
      HERALD_API_TOKEN: API_TOKEN,
      HERALD_ADMIN_TOKEN: ADMIN_TOKEN,
      HERALD_SPEC_PATH: specPath,
    };

    const proc = spawn(cmd, args, { cwd, env, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    proc.stdout.on("data", (d: Buffer) => { stdout += d.toString(); });
    proc.stderr.on("data", (d: Buffer) => { stderr += d.toString(); });

    proc.on("close", (code) => {
      // Parse pass/fail from output
      process.stdout.write(stdout);
      if (stderr) process.stderr.write(stderr);

      const passMatch = stdout.match(/(\d+) passed/);
      const failMatch = stdout.match(/(\d+) failed/);
      const passed = passMatch ? parseInt(passMatch[1], 10) : 0;
      const failed = code === 0 ? (failMatch ? parseInt(failMatch[1], 10) : 0) : 1;

      resolve({ name, passed, failed: failed || (code !== 0 ? 1 : 0) });
    });

    proc.on("error", (err) => {
      console.error(`  ${name} runner error: ${err.message}`);
      resolve({ name, passed: 0, failed: 1 });
    });
  });
}

async function main(): Promise<void> {
  console.log("Contract Tests — Starting Herald server...");
  await startServer();
  console.log("Server ready.\n");

  const results: SuiteResult[] = [];

  try {
    // TypeScript — in-process
    const tsResult = await runTypeScript();
    results.push({ name: "TypeScript", ...tsResult });

    // Go
    const goDir = join(import.meta.dirname, "go");
    const goResult = await runSubprocess(
      "Go",
      "go",
      ["test", "-v", "-count=1", "-timeout=60s", "./..."],
      goDir,
    );
    results.push(goResult);

    // Python
    const pyDir = join(import.meta.dirname, "python");
    const pyResult = await runSubprocess(
      "Python",
      "python3",
      ["test_contract.py"],
      pyDir,
    );
    results.push(pyResult);

    // Ruby
    const rbDir = join(import.meta.dirname, "ruby");
    const rbResult = await runSubprocess(
      "Ruby",
      "ruby",
      ["test_contract.rb"],
      rbDir,
    );
    results.push(rbResult);
  } finally {
    await stopServer();
  }

  // Summary
  console.log("\n" + "═".repeat(54));
  console.log("  Contract Test Summary");
  console.log("═".repeat(54));
  let totalPassed = 0;
  let totalFailed = 0;
  for (const r of results) {
    const status = r.failed > 0 ? "FAIL" : "PASS";
    console.log(`  ${status}  ${r.name.padEnd(12)} ${r.passed} passed, ${r.failed} failed`);
    totalPassed += r.passed;
    totalFailed += r.failed;
  }
  console.log("─".repeat(54));
  console.log(`  Total: ${totalPassed} passed, ${totalFailed} failed`);
  console.log("═".repeat(54));

  if (totalFailed > 0) {
    process.exit(1);
  }
}

main().catch((err) => {
  console.error("Fatal:", err);
  stopServer().finally(() => process.exit(1));
});
