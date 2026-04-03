# frozen_string_literal: true

require "erb"

module HeraldAdmin
  class Client
    attr_reader :rooms, :members, :messages, :presence, :tenants

    def initialize(url:, token:)
      transport = HttpTransport.new(url, token)
      @rooms = RoomNamespace.new(transport)
      @members = MemberNamespace.new(transport)
      @messages = MessageNamespace.new(transport)
      @presence = PresenceNamespace.new(transport)
      @tenants = TenantNamespace.new(transport)
      @transport = transport
    end

    def health
      data = @transport.request("GET", "/health")
      HealthResponse.new(
        status: data["status"],
        connections: data["connections"] || 0,
        rooms: data["rooms"] || 0,
        uptime_secs: data["uptime_secs"] || 0
      )
    end

    def connections
      @transport.request("GET", "/admin/connections")
    end

    def events(limit: nil)
      path = "/admin/events"
      path += "?limit=#{limit}" if limit
      @transport.request("GET", path)
    end

    def errors(limit: nil, category: nil)
      params = []
      params << "limit=#{limit}" if limit
      params << "category=#{ERB::Util.url_encode(category)}" if category
      path = "/admin/errors"
      path += "?#{params.join('&')}" unless params.empty?
      @transport.request("GET", path)
    end

    def stats
      @transport.request("GET", "/admin/stats")
    end
  end
end
