# frozen_string_literal: true

module HeraldAdmin
  class RoomNamespace
    def initialize(transport)
      @t = transport
    end

    def create(id, name, meta: nil, public: false)
      body = { id: id, name: name }
      body[:meta] = meta if meta
      body[:public] = true if public
      data = @t.request("POST", "/rooms", body)
      Room.new(**data.transform_keys(&:to_sym))
    end

    def get(id)
      data = @t.request("GET", "/rooms/#{ERB::Util.url_encode(id)}")
      Room.new(**data.transform_keys(&:to_sym))
    end

    def update(id, name: nil, meta: nil, archived: nil)
      body = {}
      body[:name] = name if name
      body[:meta] = meta if meta
      body[:archived] = archived unless archived.nil?
      @t.request("PATCH", "/rooms/#{ERB::Util.url_encode(id)}", body)
    end

    def list
      data = @t.request("GET", "/rooms")
      data["rooms"].map { |r| Room.new(**r.transform_keys(&:to_sym)) }
    end

    def delete(id)
      @t.request("DELETE", "/rooms/#{ERB::Util.url_encode(id)}")
    end
  end
end
