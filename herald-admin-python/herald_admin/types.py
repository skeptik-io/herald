from __future__ import annotations
from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class Stream:
    id: str
    name: str
    meta: Any = None
    public: bool = False
    archived: bool = False
    created_at: int = 0


@dataclass(frozen=True)
class Member:
    stream_id: str
    user_id: str
    role: str = "member"
    joined_at: int = 0


@dataclass(frozen=True)
class Event:
    id: str
    stream: str
    seq: int
    sender: str
    body: str
    sent_at: int
    meta: Any = None
    parent_id: str | None = None
    edited_at: int | None = None


@dataclass(frozen=True)
class EventList:
    events: list[Event] = field(default_factory=list)
    has_more: bool = False


@dataclass(frozen=True)
class EventPublishResult:
    id: str
    seq: int
    sent_at: int


@dataclass(frozen=True)
class UserPresence:
    user_id: str
    status: str
    connections: int = 0


@dataclass(frozen=True)
class MemberPresenceEntry:
    user_id: str
    status: str


@dataclass(frozen=True)
class Cursor:
    user_id: str
    seq: int


@dataclass(frozen=True)
class HealthResponse:
    status: str
    connections: int = 0
    streams: int = 0
    uptime_secs: int = 0


@dataclass(frozen=True)
class AuditEvent:
    id: str
    timestamp: int
    operation: str
    resource_type: str
    resource_id: str
    actor: str
    result: str
    tenant_id: str
    diff: Any = None
    metadata: Any = None


@dataclass(frozen=True)
class AuditQueryResult:
    events: list[AuditEvent] = field(default_factory=list)
    matched: int = 0
