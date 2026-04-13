# herald-admin

Python HTTP admin client for Herald. Zero external dependencies.

## Install

Download from GitHub Releases.

## Usage

```python
from herald_admin import HeraldAdmin

# With key + secret (Basic auth)
admin = HeraldAdmin("https://herald.example.com", key="your-tenant-key", secret="your-tenant-secret")

# Or with an API token (Bearer auth)
# admin = HeraldAdmin("https://herald.example.com", "your-api-token")

# Streams
admin.streams.create("general", "General Chat")
stream = admin.streams.get("general")
admin.streams.delete("general")

# Members
admin.members.add("general", "alice", role="owner")
members = admin.members.list("general")
admin.members.remove("general", "alice")

# Events
result = admin.events.publish("general", sender="system", body="Welcome!")
history = admin.events.list("general", limit=50)

# Presence
presence = admin.presence.get_user("alice")
stream_presence = admin.presence.get_stream("general")

# Health
health = admin.health()
```
