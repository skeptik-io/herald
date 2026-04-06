import { HeraldError } from "./errors.js";

export class HttpTransport {
  private baseUrl: string;
  private token: string;

  constructor(baseUrl: string, token: string) {
    this.baseUrl = baseUrl.replace(/\/$/, "");
    this.token = token;
  }

  async request<T>(method: string, path: string, body?: unknown, extraHeaders?: Record<string, string>): Promise<T> {
    const headers: Record<string, string> = {
      Authorization: `Bearer ${this.token}`,
    };
    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
    }
    if (extraHeaders) {
      Object.assign(headers, extraHeaders);
    }

    const resp = await fetch(`${this.baseUrl}${path}`, {
      method,
      headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });

    if (!resp.ok) {
      let code = "INTERNAL";
      let message = `HTTP ${resp.status}`;
      try {
        const json = (await resp.json()) as Record<string, string>;
        if (json.error) {
          message = json.error;
          code = resp.status === 404 ? "NOT_FOUND" : resp.status === 401 ? "UNAUTHORIZED" : "INTERNAL";
        }
      } catch {
        // raw text
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
