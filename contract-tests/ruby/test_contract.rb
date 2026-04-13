# frozen_string_literal: true

# Ruby Admin SDK — Contract Test Runner
#
# Reads the shared contract spec and exercises every operation
# through the herald-admin Ruby SDK, validating responses.

$LOAD_PATH.unshift(File.join(__dir__, "..", "..", "herald-admin-ruby", "lib"))
require "herald_admin"
require "json"

def load_spec
  spec_path = ENV["HERALD_SPEC_PATH"] || File.join(__dir__, "..", "spec", "spec.json")
  JSON.parse(File.read(spec_path))
end

def resolve_value(val, saved)
  return val unless val.is_a?(String) && val.start_with?("$")

  ref = val[1..]
  if ref == "event_seq_minus_1"
    saved["event_seq"] - 1
  else
    saved[ref]
  end
end

def resolve_input(inp, saved)
  inp.transform_values { |v| resolve_value(v, saved) }
end

def type_of(val)
  case val
  when nil then "null"
  when true, false then "boolean"
  when Integer, Float then "number"
  when String then "string"
  when Array then "array"
  when Hash then "object"
  else val.class.name
  end
end

def validate_fields(obj, fields)
  errors = []
  fields.each do |key, expected_type|
    unless obj.key?(key)
      errors << "missing field: #{key}"
      next
    end
    next if expected_type == "any"

    actual = type_of(obj[key])
    next if expected_type == "object" && %w[object null].include?(actual)

    errors << "#{key}: expected type #{expected_type}, got #{actual}" if actual != expected_type
  end
  errors
end

def validate_expect(result, expect, saved)
  errors = []
  return errors if expect["void"]

  if expect["type"] == "array"
    return ["expected array, got #{type_of(result)}"] unless result.is_a?(Array)

    if expect.key?("length") && result.length != expect["length"]
      errors << "expected array length #{expect['length']}, got #{result.length}"
    end
    if expect.key?("min_length") && result.length < expect["min_length"]
      errors << "expected array min length #{expect['min_length']}, got #{result.length}"
    end
    if expect.key?("contains") && !result.include?(expect["contains"])
      errors << "expected array to contain #{expect['contains'].inspect}"
    end
    if expect.key?("item_fields") && !result.empty?
      errors.concat(validate_fields(result[0], expect["item_fields"]))
    end
    return errors
  end

  obj = result
  if !obj.is_a?(Hash) && expect.key?("fields")
    return ["expected object, got #{type_of(result)}"]
  end

  errors.concat(validate_fields(obj, expect["fields"])) if expect.key?("fields")

  if expect.key?("values")
    expect["values"].each do |key, expected|
      resolved = resolve_value(expected, saved)
      actual = obj[key]
      errors << "#{key}: expected #{resolved.inspect}, got #{actual.inspect}" if actual != resolved
    end
  end

  if expect.key?("events_length")
    events = obj["events"]
    if !events.is_a?(Array)
      errors << "expected events array"
    elsif events.length != expect["events_length"]
      errors << "expected events length #{expect['events_length']}, got #{events.length}"
    end
  end

  if expect.key?("events_min_length")
    events = obj["events"]
    if !events.is_a?(Array)
      errors << "expected events array"
    elsif events.length < expect["events_min_length"]
      errors << "expected events min length #{expect['events_min_length']}, got #{events.length}"
    end
  end

  if expect.key?("event_fields")
    events = obj["events"]
    errors.concat(validate_fields(events[0], expect["event_fields"])) if events.is_a?(Array) && !events.empty?
  end

  if expect.key?("first_event_values")
    events = obj["events"]
    if events.is_a?(Array) && !events.empty?
      expect["first_event_values"].each do |key, expected|
        resolved = resolve_value(expected, saved)
        actual = events[0][key]
        errors << "first event #{key}: expected #{resolved.inspect}, got #{actual.inspect}" if actual != resolved
      end
    end
  end

  if expect.key?("first_event_fields")
    events = obj["events"]
    errors.concat(validate_fields(events[0], expect["first_event_fields"])) if events.is_a?(Array) && !events.empty?
  end

  errors
end

def struct_to_hash(obj)
  case obj
  when Hash then obj
  when Array then obj.map { |item| struct_to_hash(item) }
  when Struct
    obj.members.each_with_object({}) do |key, h|
      h[key.to_s] = obj[key]
    end
  else
    obj
  end
end

def execute_operation(client, op, inp) # rubocop:disable Metrics/CyclomaticComplexity,Metrics/MethodLength
  case op
  when "health"
    h = client.health
    { "status" => h.status, "connections" => h.connections, "streams" => h.streams, "uptime_secs" => h.uptime_secs }

  # Streams
  when "streams.create"
    s = client.streams.create(inp["id"], inp["name"], meta: inp["meta"], public: inp["public"] || false)
    struct_to_hash(s)
  when "streams.get"
    struct_to_hash(client.streams.get(inp["id"]))
  when "streams.list"
    client.streams.list.map { |s| struct_to_hash(s) }
  when "streams.update"
    client.streams.update(inp["id"], name: inp["name"], meta: inp["meta"], archived: inp["archived"])
    nil
  when "streams.delete"
    client.streams.delete(inp["id"])
    nil

  # Members
  when "members.add"
    m = client.members.add(inp["stream_id"], inp["user_id"], role: inp["role"] || "member")
    struct_to_hash(m)
  when "members.list"
    client.members.list(inp["stream_id"]).map { |m| struct_to_hash(m) }
  when "members.update"
    client.members.update(inp["stream_id"], inp["user_id"], role: inp["role"])
    nil
  when "members.remove"
    client.members.remove(inp["stream_id"], inp["user_id"])
    nil

  # Events
  when "events.publish"
    r = client.events.publish(inp["stream_id"], sender: inp["sender"], body: inp["body"],
                                                 meta: inp["meta"], parent_id: inp["parent_id"])
    { "id" => r.id, "seq" => r.seq, "sent_at" => r.sent_at }
  when "events.list"
    el = client.events.list(inp["stream_id"], before: inp["before"], after: inp["after"],
                                               limit: inp["limit"], thread: inp["thread"])
    events = el.events.map { |e| struct_to_hash(e) }
    { "events" => events, "has_more" => el.has_more }
  when "events.trigger"
    client.events.trigger(inp["stream_id"], inp["event"], data: inp["data"])
    nil
  # Presence
  when "presence.getUser"
    p = client.presence.get_user(inp["user_id"])
    { "user_id" => p.user_id, "status" => p.status, "connections" => p.connections }
  when "presence.getStream"
    client.presence.get_stream(inp["stream_id"]).map { |m| { "user_id" => m.user_id, "status" => m.status } }
  when "presence.getBulk"
    client.presence.get_bulk(inp["user_ids"])
  when "presence.setOverride"
    client.presence.set_override(inp["user_id"], status: inp["status"])

  # Blocks
  when "blocks.block"
    client.chat.blocks.block(inp["user_id"], inp["blocked_id"])
    nil
  when "blocks.unblock"
    client.chat.blocks.unblock(inp["user_id"], inp["blocked_id"])
    nil
  when "blocks.list"
    client.chat.blocks.list(inp["user_id"])

  else
    raise "Unknown operation: #{op}"
  end
end

def main
  spec = load_spec
  url = ENV["HERALD_URL"] || "http://127.0.0.1:16300"
  api_token = ENV["HERALD_API_TOKEN"]
  unless api_token
    warn "HERALD_API_TOKEN is required — run via run-all.ts or set it manually"
    exit 1
  end

  client = HeraldAdmin::Client.new(url: url, token: api_token, tenant_id: "default")
  saved = {}
  passed = 0
  failed = 0
  total = spec["groups"].sum { |g| g["cases"].length }

  puts "\n  Ruby Admin SDK Contract Tests (#{total} cases)"
  puts "  " + ("-" * 50)

  spec["groups"].each do |group|
    group["cases"].each do |tc|
      tc_id = tc["id"]
      inp = resolve_input(tc.fetch("input", {}), saved)
      begin
        result = execute_operation(client, tc["operation"], inp)
      rescue StandardError => e
        failed += 1
        puts "  FAIL [rb] #{tc_id}: #{e.message}"
        next
      end

      if tc["save"] && result.is_a?(Hash)
        tc["save"].each do |aliaz, field|
          saved[aliaz] = result[field]
        end
      end

      errs = validate_expect(result, tc["expect"], saved)
      if errs.any?
        failed += 1
        puts "  FAIL [rb] #{tc_id}: #{errs.join('; ')}"
      else
        passed += 1
        puts "  ok [rb] #{tc_id}"
      end
    end
  end

  puts "\n  Ruby: #{passed} passed, #{failed} failed\n"
  exit(failed.positive? ? 1 : 0)
end

main
