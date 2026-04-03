# frozen_string_literal: true

module HeraldAdmin
  class BlockNamespace
    def initialize(transport)
      @t = transport
    end

    def block(user_id, blocked_id)
      @t.request("POST", "/blocks", { user_id: user_id, blocked_id: blocked_id })
    end

    def unblock(user_id, blocked_id)
      @t.request("DELETE", "/blocks", { user_id: user_id, blocked_id: blocked_id })
    end

    def list(user_id)
      data = @t.request("GET", "/blocks/#{ERB::Util.url_encode(user_id)}")
      data["blocked"]
    end
  end
end
