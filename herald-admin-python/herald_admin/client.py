from __future__ import annotations

from dataclasses import dataclass
from typing import Any
from urllib.parse import quote

from .errors import HeraldError
from .transport import HttpTransport
from .types import (
    Cursor, HealthResponse, Member, MemberPresenceEntry, Event,
    EventList, EventPublishResult, Stream, UserPresence,
)


@dataclass
class HeraldAdminOptions:
    url: str
    token: str


class StreamNamespace:
    def __init__(self, t: HttpTransport) -> None:
        self._t = t

    def create(self, id: str, name: str, *, meta: Any = None, public: bool = False) -> Stream:
        body: dict[str, Any] = {"id": id, "name": name}
        if meta is not None:
            body["meta"] = meta
        if public:
            body["public"] = True
        data = self._t.request("POST", "/streams", body)
        return Stream(**{k: data[k] for k in Stream.__dataclass_fields__ if k in data})

    def get(self, id: str) -> Stream:
        data = self._t.request("GET", f"/streams/{quote(id, safe='')}")
        return Stream(**{k: data[k] for k in Stream.__dataclass_fields__ if k in data})

    def update(self, id: str, *, name: str | None = None, meta: Any = None, archived: bool | None = None) -> None:
        body: dict[str, Any] = {}
        if name is not None:
            body["name"] = name
        if meta is not None:
            body["meta"] = meta
        if archived is not None:
            body["archived"] = archived
        self._t.request("PATCH", f"/streams/{quote(id, safe='')}", body)

    def list(self) -> list[Stream]:
        data = self._t.request("GET", "/streams")
        return [Stream(**{k: r[k] for k in Stream.__dataclass_fields__ if k in r}) for r in data["streams"]]

    def delete(self, id: str) -> None:
        self._t.request("DELETE", f"/streams/{quote(id, safe='')}")


class MemberNamespace:
    def __init__(self, t: HttpTransport) -> None:
        self._t = t

    def add(self, stream_id: str, user_id: str, role: str = "member") -> Member:
        data = self._t.request("POST", f"/streams/{quote(stream_id, safe='')}/members", {"user_id": user_id, "role": role})
        return Member(**{k: data[k] for k in Member.__dataclass_fields__ if k in data})

    def list(self, stream_id: str) -> list[Member]:
        data = self._t.request("GET", f"/streams/{quote(stream_id, safe='')}/members")
        return [Member(**{k: m[k] for k in Member.__dataclass_fields__ if k in m}) for m in data["members"]]

    def remove(self, stream_id: str, user_id: str) -> None:
        self._t.request("DELETE", f"/streams/{quote(stream_id, safe='')}/members/{quote(user_id, safe='')}")

    def update(self, stream_id: str, user_id: str, role: str) -> None:
        self._t.request("PATCH", f"/streams/{quote(stream_id, safe='')}/members/{quote(user_id, safe='')}", {"role": role})


class EventNamespace:
    def __init__(self, t: HttpTransport) -> None:
        self._t = t

    def publish(self, stream_id: str, sender: str, body: str, meta: Any = None, *, parent_id: str | None = None, exclude_connection: str | None = None) -> EventPublishResult:
        req: dict[str, Any] = {"sender": sender, "body": body}
        if meta is not None:
            req["meta"] = meta
        if parent_id is not None:
            req["parent_id"] = parent_id
        if exclude_connection is not None:
            req["exclude_connection"] = exclude_connection
        data = self._t.request("POST", f"/streams/{quote(stream_id, safe='')}/events", req)
        return EventPublishResult(id=data["id"], seq=data["seq"], sent_at=data["sent_at"])

    def list(self, stream_id: str, *, before: int | None = None, after: int | None = None, limit: int | None = None, thread: str | None = None) -> EventList:
        params: list[str] = []
        if before is not None:
            params.append(f"before={before}")
        if after is not None:
            params.append(f"after={after}")
        if limit is not None:
            params.append(f"limit={limit}")
        if thread is not None:
            params.append(f"thread={quote(thread, safe='')}")
        qs = "&".join(params)
        path = f"/streams/{quote(stream_id, safe='')}/events"
        if qs:
            path += f"?{qs}"
        data = self._t.request("GET", path)
        events = [Event(id=m["id"], stream=m.get("stream", stream_id), seq=m["seq"], sender=m["sender"], body=m["body"], sent_at=m["sent_at"], meta=m.get("meta"), parent_id=m.get("parent_id"), edited_at=m.get("edited_at")) for m in data["events"]]
        return EventList(events=events, has_more=data.get("has_more", False))

    def delete(self, stream_id: str, event_id: str) -> None:
        self._t.request("DELETE", f"/streams/{quote(stream_id, safe='')}/events/{quote(event_id, safe='')}")

    def edit(self, stream_id: str, event_id: str, body: str) -> None:
        self._t.request("PATCH", f"/streams/{quote(stream_id, safe='')}/events/{quote(event_id, safe='')}", {"body": body})

    def get_reactions(self, stream_id: str, event_id: str) -> list[dict]:
        data = self._t.request("GET", f"/streams/{quote(stream_id, safe='')}/events/{quote(event_id, safe='')}/reactions")
        return data["reactions"]

    def trigger(self, stream_id: str, event: str, data: Any = None, exclude_connection: int | None = None) -> None:
        body: dict[str, Any] = {"event": event}
        if data is not None:
            body["data"] = data
        if exclude_connection is not None:
            body["exclude_connection"] = exclude_connection
        self._t.request("POST", f"/streams/{quote(stream_id, safe='')}/trigger", body)

    def search(self, stream_id: str, query: str, *, limit: int | None = None) -> EventList:
        params = [f"q={quote(query, safe='')}"]
        if limit is not None:
            params.append(f"limit={limit}")
        qs = "&".join(params)
        path = f"/streams/{quote(stream_id, safe='')}/events/search?{qs}"
        data = self._t.request("GET", path)
        events = [Event(id=m["id"], stream=m.get("stream", stream_id), seq=m["seq"], sender=m["sender"], body=m["body"], sent_at=m["sent_at"], meta=m.get("meta"), parent_id=m.get("parent_id"), edited_at=m.get("edited_at")) for m in data["events"]]
        return EventList(events=events, has_more=data.get("has_more", False))


class PresenceNamespace:
    def __init__(self, t: HttpTransport) -> None:
        self._t = t

    def get_user(self, user_id: str) -> UserPresence:
        data = self._t.request("GET", f"/presence/{quote(user_id, safe='')}")
        return UserPresence(user_id=data["user_id"], status=data["status"], connections=data.get("connections", 0))

    def get_stream(self, stream_id: str) -> list[MemberPresenceEntry]:
        data = self._t.request("GET", f"/streams/{quote(stream_id, safe='')}/presence")
        return [MemberPresenceEntry(user_id=m["user_id"], status=m["status"]) for m in data["members"]]

    def get_cursors(self, stream_id: str) -> list[Cursor]:
        data = self._t.request("GET", f"/streams/{quote(stream_id, safe='')}/cursors")
        return [Cursor(user_id=c["user_id"], seq=c["seq"]) for c in data["cursors"]]


class TenantNamespace:
    def __init__(self, t: HttpTransport) -> None:
        self._t = t

    def create(self, id: str, name: str, jwt_secret: str, *, jwt_issuer: str | None = None, plan: str | None = None) -> dict[str, Any]:
        body: dict[str, Any] = {"id": id, "name": name, "jwt_secret": jwt_secret}
        if jwt_issuer is not None:
            body["jwt_issuer"] = jwt_issuer
        if plan is not None:
            body["plan"] = plan
        return self._t.request("POST", "/admin/tenants", body)

    def list(self) -> list[dict[str, Any]]:
        data = self._t.request("GET", "/admin/tenants")
        return data["tenants"]

    def get(self, id: str) -> dict[str, Any]:
        return self._t.request("GET", f"/admin/tenants/{quote(id, safe='')}")

    def update(self, id: str, *, name: str | None = None, plan: str | None = None) -> None:
        body: dict[str, Any] = {}
        if name is not None:
            body["name"] = name
        if plan is not None:
            body["plan"] = plan
        self._t.request("PATCH", f"/admin/tenants/{quote(id, safe='')}", body)

    def delete(self, id: str) -> None:
        self._t.request("DELETE", f"/admin/tenants/{quote(id, safe='')}")

    def create_token(self, tenant_id: str, scope: str | None = None) -> str:
        body = {"scope": scope} if scope else None
        data = self._t.request("POST", f"/admin/tenants/{quote(tenant_id, safe='')}/tokens", body)
        return data["token"]

    def delete_token(self, tenant_id: str, token: str) -> None:
        self._t.request("DELETE", f"/admin/tenants/{quote(tenant_id, safe='')}/tokens/{quote(token, safe='')}")

    def list_tokens(self, tenant_id: str) -> list[str]:
        data = self._t.request("GET", f"/admin/tenants/{quote(tenant_id, safe='')}/tokens")
        return data["tokens"]

    def list_streams(self, tenant_id: str) -> list[dict[str, Any]]:
        data = self._t.request("GET", f"/admin/tenants/{quote(tenant_id, safe='')}/streams")
        return data["streams"]


class BlockNamespace:
    def __init__(self, t: HttpTransport) -> None:
        self._t = t

    def block(self, user_id: str, blocked_id: str) -> None:
        self._t.request("POST", "/blocks", {"user_id": user_id, "blocked_id": blocked_id})

    def unblock(self, user_id: str, blocked_id: str) -> None:
        self._t.request("DELETE", "/blocks", {"user_id": user_id, "blocked_id": blocked_id})

    def list(self, user_id: str) -> list[str]:
        data = self._t.request("GET", f"/blocks/{quote(user_id, safe='')}")
        return data["blocked"]


class HeraldAdmin:
    """Herald HTTP admin client for Python backends."""

    def __init__(self, url: str, token: str, *, timeout: int = 30) -> None:
        t = HttpTransport(url, token, timeout=timeout)
        self.streams = StreamNamespace(t)
        self.members = MemberNamespace(t)
        self.events = EventNamespace(t)
        self.presence = PresenceNamespace(t)
        self.tenants = TenantNamespace(t)
        self.blocks = BlockNamespace(t)
        self._transport = t

    @classmethod
    def from_options(cls, opts: HeraldAdminOptions) -> "HeraldAdmin":
        return cls(opts.url, opts.token)

    def health(self) -> HealthResponse:
        data = self._transport.request("GET", "/health")
        return HealthResponse(status=data["status"], connections=data.get("connections", 0), streams=data.get("streams", 0), uptime_secs=data.get("uptime_secs", 0))

    def connections(self) -> Any:
        return self._transport.request("GET", "/admin/connections")

    def admin_events(self, *, limit: int | None = None) -> Any:
        path = "/admin/events"
        if limit is not None:
            path += f"?limit={limit}"
        return self._transport.request("GET", path)

    def errors(self, *, limit: int | None = None, category: str | None = None) -> Any:
        params: list[str] = []
        if limit is not None:
            params.append(f"limit={limit}")
        if category is not None:
            params.append(f"category={quote(category, safe='')}")
        path = "/admin/errors"
        if params:
            path += "?" + "&".join(params)
        return self._transport.request("GET", path)

    def stats(self) -> Any:
        return self._transport.request("GET", "/admin/stats")
