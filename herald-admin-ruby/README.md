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

# Streams
client.streams.create('general', 'General Chat')
stream = client.streams.get('general')
client.streams.delete('general')

# Members
client.members.add('general', 'alice', role: 'owner')
members = client.members.list('general')
client.members.remove('general', 'alice')

# Events
result = client.events.publish('general', sender: 'system', body: 'Welcome!')
history = client.events.list('general', limit: 50)

# Presence
presence = client.presence.get_user('alice')
stream_presence = client.presence.get_stream('general')
cursors = client.presence.get_cursors('general')

# Health
health = client.health
```
