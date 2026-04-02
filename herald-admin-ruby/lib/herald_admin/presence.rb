# frozen_string_literal: true

module HeraldAdmin
  class PresenceNamespace
    def initialize(transport)
      @t = transport
    end

    def get_user(user_id)
      data = @t.request("GET", "/presence/#{ERB::Util.url_encode(user_id)}")
      UserPresence.new(user_id: data["user_id"], status: data["status"], connections: data["connections"] || 0)
    end

    def get_room(room_id)
      data = @t.request("GET", "/rooms/#{ERB::Util.url_encode(room_id)}/presence")
      data["members"].map { |m| MemberPresenceEntry.new(user_id: m["user_id"], status: m["status"]) }
    end

    def get_cursors(room_id)
      data = @t.request("GET", "/rooms/#{ERB::Util.url_encode(room_id)}/cursors")
      data["cursors"].map { |c| Cursor.new(user_id: c["user_id"], seq: c["seq"]) }
    end
  end
end
