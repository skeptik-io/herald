from __future__ import annotations
from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class Room:
    id: str
    name: str
    encryption_mode: str = "plaintext"
    meta: Any = None
    created_at: int = 0


@dataclass(frozen=True)
class Member:
    room_id: str
    user_id: str
    role: str = "member"
    joined_at: int = 0


@dataclass(frozen=True)
class Message:
    id: str
    room: str
    seq: int
    sender: str
    body: str
    sent_at: int
    meta: Any = None


@dataclass(frozen=True)
class MessageList:
    messages: list[Message] = field(default_factory=list)
    has_more: bool = False


@dataclass(frozen=True)
class MessageSendResult:
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
    rooms: int = 0
    uptime_secs: int = 0
