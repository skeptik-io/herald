from __future__ import annotations

from dataclasses import dataclass
from typing import Any
from urllib.parse import quote

from .errors import HeraldError
from .transport import HttpTransport
from .types import (
    Cursor, HealthResponse, Member, MemberPresenceEntry, Message,
    MessageList, MessageSendResult, Room, UserPresence,
)


@dataclass
class HeraldAdminOptions:
    url: str
    token: str


class RoomNamespace:
    def __init__(self, t: HttpTransport) -> None:
        self._t = t

    def create(self, id: str, name: str, *, meta: Any = None) -> Room:
        body: dict[str, Any] = {"id": id, "name": name}
        if meta is not None:
            body["meta"] = meta
        data = self._t.request("POST", "/rooms", body)
        return Room(**{k: data[k] for k in Room.__dataclass_fields__ if k in data})

    def get(self, id: str) -> Room:
        data = self._t.request("GET", f"/rooms/{quote(id, safe='')}")
        return Room(**{k: data[k] for k in Room.__dataclass_fields__ if k in data})

    def update(self, id: str, *, name: str | None = None, meta: Any = None) -> None:
        body: dict[str, Any] = {}
        if name is not None:
            body["name"] = name
        if meta is not None:
            body["meta"] = meta
        self._t.request("PATCH", f"/rooms/{quote(id, safe='')}", body)

    def list(self) -> list[Room]:
        data = self._t.request("GET", "/rooms")
        return [Room(**{k: r[k] for k in Room.__dataclass_fields__ if k in r}) for r in data["rooms"]]

    def delete(self, id: str) -> None:
        self._t.request("DELETE", f"/rooms/{quote(id, safe='')}")


class MemberNamespace:
    def __init__(self, t: HttpTransport) -> None:
        self._t = t

    def add(self, room_id: str, user_id: str, role: str = "member") -> Member:
        data = self._t.request("POST", f"/rooms/{quote(room_id, safe='')}/members", {"user_id": user_id, "role": role})
        return Member(**{k: data[k] for k in Member.__dataclass_fields__ if k in data})

    def list(self, room_id: str) -> list[Member]:
        data = self._t.request("GET", f"/rooms/{quote(room_id, safe='')}/members")
        return [Member(**{k: m[k] for k in Member.__dataclass_fields__ if k in m}) for m in data["members"]]

    def remove(self, room_id: str, user_id: str) -> None:
        self._t.request("DELETE", f"/rooms/{quote(room_id, safe='')}/members/{quote(user_id, safe='')}")

    def update(self, room_id: str, user_id: str, role: str) -> None:
        self._t.request("PATCH", f"/rooms/{quote(room_id, safe='')}/members/{quote(user_id, safe='')}", {"role": role})


class MessageNamespace:
    def __init__(self, t: HttpTransport) -> None:
        self._t = t

    def send(self, room_id: str, sender: str, body: str, meta: Any = None) -> MessageSendResult:
        req: dict[str, Any] = {"sender": sender, "body": body}
        if meta is not None:
            req["meta"] = meta
        data = self._t.request("POST", f"/rooms/{quote(room_id, safe='')}/messages", req)
        return MessageSendResult(id=data["id"], seq=data["seq"], sent_at=data["sent_at"])

    def list(self, room_id: str, *, before: int | None = None, after: int | None = None, limit: int | None = None) -> MessageList:
        params: list[str] = []
        if before is not None:
            params.append(f"before={before}")
        if after is not None:
            params.append(f"after={after}")
        if limit is not None:
            params.append(f"limit={limit}")
        qs = "&".join(params)
        path = f"/rooms/{quote(room_id, safe='')}/messages"
        if qs:
            path += f"?{qs}"
        data = self._t.request("GET", path)
        messages = [Message(id=m["id"], room=m.get("room", room_id), seq=m["seq"], sender=m["sender"], body=m["body"], sent_at=m["sent_at"], meta=m.get("meta")) for m in data["messages"]]
        return MessageList(messages=messages, has_more=data.get("has_more", False))

    def search(self, room_id: str, query: str, *, limit: int | None = None) -> MessageList:
        params = [f"q={quote(query, safe='')}"]
        if limit is not None:
            params.append(f"limit={limit}")
        qs = "&".join(params)
        path = f"/rooms/{quote(room_id, safe='')}/messages/search?{qs}"
        data = self._t.request("GET", path)
        messages = [Message(id=m["id"], room=m.get("room", room_id), seq=m["seq"], sender=m["sender"], body=m["body"], sent_at=m["sent_at"], meta=m.get("meta")) for m in data["messages"]]
        return MessageList(messages=messages, has_more=data.get("has_more", False))


class PresenceNamespace:
    def __init__(self, t: HttpTransport) -> None:
        self._t = t

    def get_user(self, user_id: str) -> UserPresence:
        data = self._t.request("GET", f"/presence/{quote(user_id, safe='')}")
        return UserPresence(user_id=data["user_id"], status=data["status"], connections=data.get("connections", 0))

    def get_room(self, room_id: str) -> list[MemberPresenceEntry]:
        data = self._t.request("GET", f"/rooms/{quote(room_id, safe='')}/presence")
        return [MemberPresenceEntry(user_id=m["user_id"], status=m["status"]) for m in data["members"]]

    def get_cursors(self, room_id: str) -> list[Cursor]:
        data = self._t.request("GET", f"/rooms/{quote(room_id, safe='')}/cursors")
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


class HeraldAdmin:
    """Herald HTTP admin client for Python backends."""

    def __init__(self, url: str, token: str, *, timeout: int = 30) -> None:
        t = HttpTransport(url, token, timeout=timeout)
        self.rooms = RoomNamespace(t)
        self.members = MemberNamespace(t)
        self.messages = MessageNamespace(t)
        self.presence = PresenceNamespace(t)
        self.tenants = TenantNamespace(t)
        self._transport = t

    @classmethod
    def from_options(cls, opts: HeraldAdminOptions) -> "HeraldAdmin":
        return cls(opts.url, opts.token)

    def health(self) -> HealthResponse:
        data = self._transport.request("GET", "/health")
        return HealthResponse(status=data["status"], connections=data.get("connections", 0), rooms=data.get("rooms", 0), uptime_secs=data.get("uptime_secs", 0))

    def connections(self) -> Any:
        return self._transport.request("GET", "/admin/connections")

    def events(self, *, limit: int | None = None) -> Any:
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
