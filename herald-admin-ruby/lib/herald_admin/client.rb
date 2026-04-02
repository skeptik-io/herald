# frozen_string_literal: true

require "erb"

module HeraldAdmin
  class Client
    attr_reader :rooms, :members, :messages, :presence

    def initialize(url:, token:)
      transport = HttpTransport.new(url, token)
      @rooms = RoomNamespace.new(transport)
      @members = MemberNamespace.new(transport)
      @messages = MessageNamespace.new(transport)
      @presence = PresenceNamespace.new(transport)
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
  end
end
