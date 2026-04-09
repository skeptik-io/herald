/**
 * Server lifecycle management for contract tests.
 * Starts a real Herald server, waits for readiness, and provides teardown.
 */

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomBytes } from "node:crypto";

export const WS_PORT = 16300;
export const HTTP_PORT = 16301;
export const MASTER_KEY = randomBytes(32).toString("hex");
export const JWT_SECRET = "contract-test-secret";
export const ADMIN_TOKEN = "contract-admin-token";
export const API_TOKEN = "contract-api-token-1234";

let serverProcess: ChildProcess | null = null;
let dataDir: string;

export async function startServer(): Promise<void> {
  dataDir = await mkdtemp(join(tmpdir(), "herald-contract-"));

  const configPath = join(dataDir, "herald.toml");
  const config = `
[server]
ws_bind = "127.0.0.1:${WS_PORT}"
http_bind = "127.0.0.1:${HTTP_PORT}"
log_level = "error"
shutdown_timeout_secs = 1
api_rate_limit = 10000
max_messages_per_sec = 1000
ws_max_message_size = 65536

[store]
path = "${join(dataDir, "data")}"

[auth]
jwt_secret = "${JWT_SECRET}"
super_admin_token = "${ADMIN_TOKEN}"

[auth.api]
tokens = ["${API_TOKEN}"]

[presence]
linger_secs = 0
manual_override_ttl_secs = 14400
`;
  await writeFile(configPath, config);

  const binaryPath = join(process.cwd(), "..", "target", "release", "herald");

  return new Promise((resolve, reject) => {
    serverProcess = spawn(binaryPath, ["--single-tenant", configPath], {
      env: { ...process.env, SHROUDB_MASTER_KEY: MASTER_KEY },
      stdio: ["ignore", "pipe", "pipe"],
    });

    let stderr = "";
    serverProcess.stderr!.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });

    serverProcess.on("error", (err) => {
      reject(new Error(`Failed to start server: ${err.message}`));
    });

    const maxAttempts = 40;
    let attempts = 0;
    const check = setInterval(async () => {
      attempts++;
      try {
        const resp = await fetch(`http://127.0.0.1:${HTTP_PORT}/health`);
        if (resp.ok) {
          clearInterval(check);
          resolve();
        }
      } catch {
        if (attempts > maxAttempts) {
          clearInterval(check);
          reject(new Error(`Server failed to start after ${maxAttempts} attempts.\nstderr: ${stderr}`));
        }
      }
    }, 250);
  });
}

export async function stopServer(): Promise<void> {
  if (serverProcess) {
    serverProcess.kill("SIGTERM");
    await new Promise<void>((resolve) => {
      serverProcess!.on("exit", () => resolve());
      setTimeout(resolve, 3000);
    });
    serverProcess = null;
  }
  try {
    await rm(dataDir, { recursive: true, force: true });
  } catch {}
}

export function serverUrl(): string {
  return `http://127.0.0.1:${HTTP_PORT}`;
}
