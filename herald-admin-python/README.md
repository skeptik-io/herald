# herald-admin

Python HTTP admin client for Herald. Zero external dependencies.

## Install

Download from GitHub Releases.

## Usage

```python
from herald_admin import HeraldAdmin

admin = HeraldAdmin("https://herald.example.com", "your-api-token")

# Rooms
admin.rooms.create("general", "General Chat")
room = admin.rooms.get("general")
admin.rooms.delete("general")

# Members
admin.members.add("general", "alice", role="owner")
members = admin.members.list("general")
admin.members.remove("general", "alice")

# Messages
result = admin.messages.send("general", sender="system", body="Welcome!")
history = admin.messages.list("general", limit=50)

# Presence
presence = admin.presence.get_user("alice")
room_presence = admin.presence.get_room("general")
cursors = admin.presence.get_cursors("general")

# Health
health = admin.health()
```
