# frozen_string_literal: true

module HeraldAdmin
  class MessageNamespace
    def initialize(transport)
      @t = transport
    end

    def send(room_id, sender:, body:, meta: nil, parent_id: nil, exclude_connection: nil)
      req = { sender: sender, body: body }
      req[:meta] = meta if meta
      req[:parent_id] = parent_id if parent_id
      req[:exclude_connection] = exclude_connection if exclude_connection
      data = @t.request("POST", "/rooms/#{esc(room_id)}/messages", req)
      MessageSendResult.new(id: data["id"], seq: data["seq"], sent_at: data["sent_at"])
    end

    def list(room_id, before: nil, after: nil, limit: nil, thread: nil)
      params = []
      params << "before=#{before}" if before
      params << "after=#{after}" if after
      params << "limit=#{limit}" if limit
      params << "thread=#{ERB::Util.url_encode(thread)}" if thread
      qs = params.empty? ? "" : "?#{params.join("&")}"
      data = @t.request("GET", "/rooms/#{esc(room_id)}/messages#{qs}")
      messages = data["messages"].map do |m|
        Message.new(id: m["id"], room: m["room"] || room_id, seq: m["seq"],
                    sender: m["sender"], body: m["body"], meta: m["meta"],
                    parent_id: m["parent_id"], edited_at: m["edited_at"], sent_at: m["sent_at"])
      end
      MessageList.new(messages: messages, has_more: data["has_more"] || false)
    end

    def delete(room_id, message_id)
      @t.request("DELETE", "/rooms/#{esc(room_id)}/messages/#{esc(message_id)}")
    end

    def edit(room_id, message_id, body)
      @t.request("PATCH", "/rooms/#{esc(room_id)}/messages/#{esc(message_id)}", { body: body })
    end

    def get_reactions(room_id, message_id)
      data = @t.request("GET", "/rooms/#{esc(room_id)}/messages/#{esc(message_id)}/reactions")
      data["reactions"]
    end

    def trigger(room_id, event, data: nil, exclude_connection: nil)
      body = { event: event }
      body[:data] = data if data
      body[:exclude_connection] = exclude_connection if exclude_connection
      @t.request("POST", "/rooms/#{esc(room_id)}/trigger", body)
    end

    def search(room_id, query:, limit: nil)
      params = ["q=#{ERB::Util.url_encode(query)}"]
      params << "limit=#{limit}" if limit
      qs = params.join("&")
      data = @t.request("GET", "/rooms/#{esc(room_id)}/messages/search?#{qs}")
      messages = data["messages"].map do |m|
        Message.new(id: m["id"], room: m["room"] || room_id, seq: m["seq"],
                    sender: m["sender"], body: m["body"], meta: m["meta"],
                    parent_id: m["parent_id"], edited_at: m["edited_at"], sent_at: m["sent_at"])
      end
      MessageList.new(messages: messages, has_more: data["has_more"] || false)
    end

    private

    def esc(s) = ERB::Util.url_encode(s)
  end
end
