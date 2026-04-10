import { HeraldError } from "./errors.js";

const HTTP_ERROR_CODES: Record<number, string> = {
  400: "BAD_REQUEST",
  401: "UNAUTHORIZED",
  403: "FORBIDDEN",
  404: "NOT_FOUND",
  409: "CONFLICT",
  429: "RATE_LIMITED",
  500: "INTERNAL",
  503: "UNAVAILABLE",
};

export class HttpTransport {
  private baseUrl: string;
  private authHeader: string;
  private timeoutMs: number;

  constructor(baseUrl: string, auth: { token: string } | { key: string; secret: string }, timeoutMs?: number) {
    this.baseUrl = baseUrl.replace(/\/$/, "");
    this.authHeader = "token" in auth
      ? `Bearer ${auth.token}`
      : `Basic ${btoa(`${auth.key}:${auth.secret}`)}`;
    this.timeoutMs = timeoutMs ?? 30_000;
  }

  async request<T>(method: string, path: string, body?: unknown, extraHeaders?: Record<string, string>): Promise<T> {
    const headers: Record<string, string> = {
      Authorization: this.authHeader,
    };
    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
    }
    if (extraHeaders) {
      Object.assign(headers, extraHeaders);
    }

    const signal = AbortSignal.timeout(this.timeoutMs);

    const resp = await fetch(`${this.baseUrl}${path}`, {
      method,
      headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
      signal,
    });

    if (!resp.ok) {
      const code = HTTP_ERROR_CODES[resp.status] ?? "INTERNAL";
      let message = `HTTP ${resp.status}`;
      try {
        const json = (await resp.json()) as Record<string, string>;
        if (json.error) {
          message = json.error;
        }
      } catch {
        const text = await resp.text().catch(() => "");
        if (text) message = text;
      }
      throw new HeraldError(code, message, resp.status);
    }

    const contentLength = resp.headers.get("content-length");
    if (resp.status === 204 || contentLength === "0") {
      return undefined as T;
    }

    const text = await resp.text();
    if (!text) return undefined as T;
    return JSON.parse(text) as T;
  }
}
