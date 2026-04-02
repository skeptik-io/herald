# herald-admin

Ruby HTTP admin client for Herald.

## Install

```bash
gem install herald-admin --source https://rubygems.pkg.github.com/skeptik-io
```

## Usage

```ruby
require 'herald_admin'

client = HeraldAdmin::Client.new(
  url: 'https://herald.example.com',
  token: 'your-api-token'
)

# Rooms
client.rooms.create('general', 'General Chat')
room = client.rooms.get('general')
client.rooms.delete('general')

# Members
client.members.add('general', 'alice', role: 'owner')
members = client.members.list('general')
client.members.remove('general', 'alice')

# Messages
result = client.messages.send('general', sender: 'system', body: 'Welcome!')
history = client.messages.list('general', limit: 50)

# Presence
presence = client.presence.get_user('alice')
room_presence = client.presence.get_room('general')
cursors = client.presence.get_cursors('general')

# Health
health = client.health
```
