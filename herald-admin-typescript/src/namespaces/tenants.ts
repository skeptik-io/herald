import type { HttpTransport } from "../transport.js";

export interface Tenant {
  id: string;
  name: string;
  plan: string;
  created_at: number;
}

export interface CreateTenantOptions {
  id: string;
  name: string;
  jwt_secret: string;
  jwt_issuer?: string;
  plan?: string;
}

export interface ApiToken {
  token: string;
  scope?: string | null;
}

export class TenantNamespace {
  constructor(private transport: HttpTransport) {}

  async create(opts: CreateTenantOptions): Promise<Tenant> {
    return this.transport.request<Tenant>("POST", "/admin/tenants", opts);
  }

  async list(): Promise<Tenant[]> {
    const res = await this.transport.request<{ tenants: Tenant[] }>("GET", "/admin/tenants");
    return res.tenants;
  }

  async get(id: string): Promise<Tenant> {
    return this.transport.request<Tenant>("GET", `/admin/tenants/${encodeURIComponent(id)}`);
  }

  async update(id: string, opts: { name?: string; plan?: string; config?: unknown }): Promise<void> {
    await this.transport.request("PATCH", `/admin/tenants/${encodeURIComponent(id)}`, opts);
  }

  async delete(id: string): Promise<void> {
    await this.transport.request("DELETE", `/admin/tenants/${encodeURIComponent(id)}`);
  }

  async createToken(tenantId: string, scope?: string): Promise<ApiToken> {
    const body = scope ? { scope } : undefined;
    return this.transport.request<ApiToken>("POST", `/admin/tenants/${encodeURIComponent(tenantId)}/tokens`, body);
  }

  async listTokens(tenantId: string): Promise<ApiToken[]> {
    const res = await this.transport.request<{ tokens: ApiToken[] }>("GET", `/admin/tenants/${encodeURIComponent(tenantId)}/tokens`);
    return res.tokens;
  }

  async deleteToken(tenantId: string, token: string): Promise<void> {
    await this.transport.request("DELETE", `/admin/tenants/${encodeURIComponent(tenantId)}/tokens/${encodeURIComponent(token)}`);
  }
}
