# frozen_string_literal: true

module HeraldAdmin
  class StreamNamespace
    def initialize(transport)
      @t = transport
    end

    def create(id, name, meta: nil, public: false)
      body = { id: id, name: name }
      body[:meta] = meta if meta
      body[:public] = true if public
      data = @t.request("POST", "/streams", body)
      Stream.new(**data.transform_keys(&:to_sym))
    end

    def get(id)
      data = @t.request("GET", "/streams/#{ERB::Util.url_encode(id)}")
      Stream.new(**data.transform_keys(&:to_sym))
    end

    def update(id, name: nil, meta: nil, archived: nil)
      body = {}
      body[:name] = name if name
      body[:meta] = meta if meta
      body[:archived] = archived unless archived.nil?
      @t.request("PATCH", "/streams/#{ERB::Util.url_encode(id)}", body)
    end

    def list
      data = @t.request("GET", "/streams")
      data["streams"].map { |r| Stream.new(**r.transform_keys(&:to_sym)) }
    end

    def delete(id)
      @t.request("DELETE", "/streams/#{ERB::Util.url_encode(id)}")
    end
  end
end
