# frozen_string_literal: true

module HeraldAdmin
  class TenantNamespace
    def initialize(transport)
      @t = transport
    end

    def create(name, plan: nil)
      body = { name: name }
      body[:plan] = plan if plan
      @t.request("POST", "/admin/tenants", body)
    end

    def list
      data = @t.request("GET", "/admin/tenants")
      data["tenants"]
    end

    def get(id)
      @t.request("GET", "/admin/tenants/#{ERB::Util.url_encode(id)}")
    end

    def update(id, name: nil, plan: nil, event_ttl_days: nil)
      body = {}
      body[:name] = name if name
      body[:plan] = plan if plan
      body[:event_ttl_days] = event_ttl_days if event_ttl_days
      @t.request("PATCH", "/admin/tenants/#{ERB::Util.url_encode(id)}", body)
    end

    def delete(id)
      @t.request("DELETE", "/admin/tenants/#{ERB::Util.url_encode(id)}")
    end

    def create_token(tenant_id, scope: nil)
      body = scope ? { scope: scope } : nil
      data = @t.request("POST", "/admin/tenants/#{ERB::Util.url_encode(tenant_id)}/tokens", body)
      data["token"]
    end

    def delete_token(tenant_id, token)
      @t.request("DELETE", "/admin/tenants/#{ERB::Util.url_encode(tenant_id)}/tokens/#{ERB::Util.url_encode(token)}")
    end

    def list_tokens(tenant_id)
      data = @t.request("GET", "/admin/tenants/#{ERB::Util.url_encode(tenant_id)}/tokens")
      data["tokens"]
    end

    def list_streams(tenant_id)
      data = @t.request("GET", "/admin/tenants/#{ERB::Util.url_encode(tenant_id)}/streams")
      data["streams"]
    end
  end
end
