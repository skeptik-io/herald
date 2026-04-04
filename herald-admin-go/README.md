# herald-admin-go

Go HTTP admin client for Herald.

## Install

```bash
go get github.com/skeptik-io/herald/herald-admin-go
```

## Usage

```go
package main

import (
    "context"
    "fmt"
    herald "github.com/skeptik-io/herald/herald-admin-go"
)

func main() {
    client := herald.New(herald.Options{
        URL:   "https://herald.example.com",
        Token: "your-api-token",
    })

    ctx := context.Background()

    // Streams
    stream, _ := client.Streams.Create(ctx, "general", "General Chat", nil)
    fmt.Println(stream.Name)

    // Members
    client.Members.Add(ctx, "general", "alice", "owner")
    members, _ := client.Members.List(ctx, "general")

    // Events
    result, _ := client.Events.Publish(ctx, "general", "system", "Welcome!", nil)
    fmt.Println(result.Seq)

    // Presence
    presence, _ := client.Presence.GetUser(ctx, "alice")
    fmt.Println(presence.Status)

    // Health
    health, _ := client.Health(ctx)
    fmt.Println(health.Status)
}
```
