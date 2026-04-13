"""
Python Admin SDK — Contract Test Runner

Reads the shared contract spec and exercises every operation
through the herald-admin Python SDK, validating responses.
"""

import json
import os
import sys

# Add the Python SDK to the path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "herald-admin-python"))

from herald_admin import HeraldAdmin  # noqa: E402


def load_spec():
    spec_path = os.environ.get(
        "HERALD_SPEC_PATH",
        os.path.join(os.path.dirname(__file__), "..", "spec", "spec.json"),
    )
    with open(spec_path) as f:
        return json.load(f)


def resolve_value(val, saved):
    if isinstance(val, str) and val.startswith("$"):
        ref = val[1:]
        if ref == "event_seq_minus_1":
            return saved["event_seq"] - 1
        return saved.get(ref)
    return val


def resolve_input(inp, saved):
    return {k: resolve_value(v, saved) for k, v in inp.items()}


def type_of(val):
    if val is None:
        return "null"
    if isinstance(val, bool):
        return "boolean"
    if isinstance(val, (int, float)):
        return "number"
    if isinstance(val, str):
        return "string"
    if isinstance(val, list):
        return "array"
    if isinstance(val, dict):
        return "object"
    return type(val).__name__


def validate_fields(obj, fields):
    errors = []
    for key, expected_type in fields.items():
        if key not in obj:
            errors.append(f"missing field: {key}")
            continue
        if expected_type == "any":
            continue
        actual = type_of(obj[key])
        if expected_type == "object" and actual in ("object", "null"):
            continue
        if actual != expected_type:
            errors.append(f"{key}: expected type {expected_type}, got {actual}")
    return errors


def validate_expect(result, expect, saved):
    errors = []

    if expect.get("void"):
        return errors

    if expect.get("type") == "array":
        if not isinstance(result, list):
            return [f"expected array, got {type_of(result)}"]
        if "length" in expect and len(result) != expect["length"]:
            errors.append(f"expected array length {expect['length']}, got {len(result)}")
        if "min_length" in expect and len(result) < expect["min_length"]:
            errors.append(f"expected array min length {expect['min_length']}, got {len(result)}")
        if "contains" in expect and expect["contains"] not in result:
            errors.append(f"expected array to contain {expect['contains']!r}")
        if "item_fields" in expect and len(result) > 0:
            errors.extend(validate_fields(result[0], expect["item_fields"]))
        return errors

    obj = result
    if not isinstance(obj, dict) and expect.get("fields"):
        return ["expected object, got " + type_of(result)]

    if "fields" in expect:
        errors.extend(validate_fields(obj, expect["fields"]))

    if "values" in expect:
        for key, expected in expect["values"].items():
            resolved = resolve_value(expected, saved)
            actual = obj.get(key)
            if actual != resolved:
                errors.append(f"{key}: expected {resolved!r}, got {actual!r}")

    if "events_length" in expect:
        events = obj.get("events", [])
        if not isinstance(events, list):
            errors.append("expected events array")
        elif len(events) != expect["events_length"]:
            errors.append(f"expected events length {expect['events_length']}, got {len(events)}")

    if "events_min_length" in expect:
        events = obj.get("events", [])
        if not isinstance(events, list):
            errors.append("expected events array")
        elif len(events) < expect["events_min_length"]:
            errors.append(f"expected events min length {expect['events_min_length']}, got {len(events)}")

    if "event_fields" in expect:
        events = obj.get("events", [])
        if isinstance(events, list) and len(events) > 0:
            errors.extend(validate_fields(events[0], expect["event_fields"]))

    if "first_event_values" in expect:
        events = obj.get("events", [])
        if isinstance(events, list) and len(events) > 0:
            for key, expected in expect["first_event_values"].items():
                resolved = resolve_value(expected, saved)
                if events[0].get(key) != resolved:
                    errors.append(f"first event {key}: expected {resolved!r}, got {events[0].get(key)!r}")

    if "first_event_fields" in expect:
        events = obj.get("events", [])
        if isinstance(events, list) and len(events) > 0:
            errors.extend(validate_fields(events[0], expect["first_event_fields"]))

    return errors


def to_dict(obj):
    """Convert SDK dataclass/struct to a plain dict for validation."""
    if isinstance(obj, dict):
        return obj
    if isinstance(obj, list):
        return [to_dict(item) for item in obj]
    if hasattr(obj, "__dataclass_fields__"):
        return {k: getattr(obj, k) for k in obj.__dataclass_fields__}
    return obj


def execute_operation(client, op, inp):
    if op == "health":
        h = client.health()
        return {"status": h.status, "connections": h.connections, "streams": h.streams, "uptime_secs": h.uptime_secs}

    # Streams
    if op == "streams.create":
        s = client.streams.create(inp["id"], inp["name"], meta=inp.get("meta"), public=inp.get("public", False))
        return to_dict(s)
    if op == "streams.get":
        return to_dict(client.streams.get(inp["id"]))
    if op == "streams.list":
        return [to_dict(s) for s in client.streams.list()]
    if op == "streams.update":
        client.streams.update(inp["id"], name=inp.get("name"), meta=inp.get("meta"), archived=inp.get("archived"))
        return None
    if op == "streams.delete":
        client.streams.delete(inp["id"])
        return None

    # Members
    if op == "members.add":
        m = client.members.add(inp["stream_id"], inp["user_id"], role=inp.get("role", "member"))
        return to_dict(m)
    if op == "members.list":
        return [to_dict(m) for m in client.members.list(inp["stream_id"])]
    if op == "members.update":
        client.members.update(inp["stream_id"], inp["user_id"], inp["role"])
        return None
    if op == "members.remove":
        client.members.remove(inp["stream_id"], inp["user_id"])
        return None

    # Events
    if op == "events.publish":
        r = client.events.publish(inp["stream_id"], inp["sender"], inp["body"], meta=inp.get("meta"), parent_id=inp.get("parent_id"))
        return {"id": r.id, "seq": r.seq, "sent_at": r.sent_at}
    if op == "events.list":
        el = client.events.list(
            inp["stream_id"],
            before=inp.get("before"), after=inp.get("after"),
            limit=inp.get("limit"), thread=inp.get("thread"),
        )
        events = []
        for e in el.events:
            d = to_dict(e)
            events.append(d)
        return {"events": events, "has_more": el.has_more}
    if op == "events.trigger":
        client.events.trigger(inp["stream_id"], inp["event"], data=inp.get("data"))
        return None
    # Presence
    if op == "presence.getUser":
        p = client.presence.get_user(inp["user_id"])
        return {"user_id": p.user_id, "status": p.status, "connections": p.connections}
    if op == "presence.getStream":
        members = client.presence.get_stream(inp["stream_id"])
        return [{"user_id": m.user_id, "status": m.status} for m in members]
    if op == "presence.getBulk":
        return client.presence.get_bulk(inp["user_ids"])
    if op == "presence.setOverride":
        return client.presence.set_override(inp["user_id"], status=inp["status"])

    # Blocks
    if op == "blocks.block":
        client.chat.blocks.block(inp["user_id"], inp["blocked_id"])
        return None
    if op == "blocks.unblock":
        client.chat.blocks.unblock(inp["user_id"], inp["blocked_id"])
        return None
    if op == "blocks.list":
        return client.chat.blocks.list(inp["user_id"])

    raise ValueError(f"Unknown operation: {op}")


def main():
    spec = load_spec()
    url = os.environ.get("HERALD_URL", "http://127.0.0.1:16300")
    api_token = os.environ.get("HERALD_API_TOKEN")
    if not api_token:
        print("HERALD_API_TOKEN is required — run via run-all.ts or set it manually", file=sys.stderr)
        sys.exit(1)

    client = HeraldAdmin(url, api_token)

    saved = {}
    passed = 0
    failed = 0
    total = sum(len(g["cases"]) for g in spec["groups"])

    print(f"\n  Python Admin SDK Contract Tests ({total} cases)")
    print("  " + "-" * 50)

    for group in spec["groups"]:
        for tc in group["cases"]:
            tc_id = tc["id"]
            inp = resolve_input(tc.get("input", {}), saved)
            try:
                result = execute_operation(client, tc["operation"], inp)
            except Exception as e:
                failed += 1
                print(f"  FAIL [py] {tc_id}: {e}")
                continue

            if tc.get("save") and isinstance(result, dict):
                for alias, field in tc["save"].items():
                    saved[alias] = result[field]

            # Convert thread/limit inputs from float to int for comparison
            if isinstance(inp.get("after"), float):
                inp["after"] = int(inp["after"])

            errs = validate_expect(result, tc["expect"], saved)
            if errs:
                failed += 1
                print(f"  FAIL [py] {tc_id}: {'; '.join(errs)}")
            else:
                passed += 1
                print(f"  ok [py] {tc_id}")

    print(f"\n  Python: {passed} passed, {failed} failed\n")
    sys.exit(1 if failed > 0 else 0)


if __name__ == "__main__":
    main()
