# frozen_string_literal: true

module HeraldAdmin
  Room = Struct.new(:id, :name, :encryption_mode, :meta, :created_at, keyword_init: true)
  Member = Struct.new(:room_id, :user_id, :role, :joined_at, keyword_init: true)
  Message = Struct.new(:id, :room, :seq, :sender, :body, :meta, :sent_at, keyword_init: true)
  MessageList = Struct.new(:messages, :has_more, keyword_init: true)
  MessageSendResult = Struct.new(:id, :seq, :sent_at, keyword_init: true)
  UserPresence = Struct.new(:user_id, :status, :connections, keyword_init: true)
  MemberPresenceEntry = Struct.new(:user_id, :status, keyword_init: true)
  Cursor = Struct.new(:user_id, :seq, keyword_init: true)
  HealthResponse = Struct.new(:status, :connections, :rooms, :uptime_secs, keyword_init: true)
end
