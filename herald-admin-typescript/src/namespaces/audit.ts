import type { HttpTransport } from "../transport.js";
import type { AuditQueryOptions, AuditQueryResponse, AuditCountOptions, AuditCountResponse } from "../types.js";

export class AuditNamespace {
  constructor(
    private transport: HttpTransport,
    private tenantId: string,
  ) {}

  private buildParams(opts?: AuditQueryOptions | AuditCountOptions): string {
    if (!opts) return "";
    const params = new URLSearchParams();
    if (opts.operation != null) params.set("operation", opts.operation);
    if (opts.resource_type != null) params.set("resource_type", opts.resource_type);
    if (opts.resource_id != null) params.set("resource_id", opts.resource_id);
    if (opts.actor != null) params.set("actor", opts.actor);
    if (opts.result != null) params.set("result", opts.result);
    if (opts.since != null) params.set("since", opts.since);
    if (opts.until != null) params.set("until", opts.until);
    if ("limit" in opts && opts.limit != null) params.set("limit", String(opts.limit));
    const qs = params.toString();
    return qs ? `?${qs}` : "";
  }

  private basePath(): string {
    return `/admin/tenants/${encodeURIComponent(this.tenantId)}/audit`;
  }

  async query(opts?: AuditQueryOptions): Promise<AuditQueryResponse> {
    return this.transport.request<AuditQueryResponse>(
      "GET",
      `${this.basePath()}${this.buildParams(opts)}`,
    );
  }

  async count(opts?: AuditCountOptions): Promise<AuditCountResponse> {
    return this.transport.request<AuditCountResponse>(
      "GET",
      `${this.basePath()}/count${this.buildParams(opts)}`,
    );
  }
}
