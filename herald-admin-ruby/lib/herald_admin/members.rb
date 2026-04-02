# frozen_string_literal: true

module HeraldAdmin
  class MemberNamespace
    def initialize(transport)
      @t = transport
    end

    def add(room_id, user_id, role: "member")
      data = @t.request("POST", "/rooms/#{esc(room_id)}/members", { user_id: user_id, role: role })
      Member.new(**data.transform_keys(&:to_sym))
    end

    def list(room_id)
      data = @t.request("GET", "/rooms/#{esc(room_id)}/members")
      data["members"].map { |m| Member.new(**m.transform_keys(&:to_sym)) }
    end

    def remove(room_id, user_id)
      @t.request("DELETE", "/rooms/#{esc(room_id)}/members/#{esc(user_id)}")
    end

    def update(room_id, user_id, role:)
      @t.request("PATCH", "/rooms/#{esc(room_id)}/members/#{esc(user_id)}", { role: role })
    end

    private

    def esc(s) = ERB::Util.url_encode(s)
  end
end
