// Herald multi-image build configuration.
//
// Build all images:   docker buildx bake
// Build one image:    docker buildx bake herald
// Build for push:     docker buildx bake --set "*.platform=linux/amd64,linux/arm64"

variable "REGISTRY" {
  default = "herald"
}

variable "TAG" {
  default = "latest"
}

group "default" {
  targets = ["herald", "chat", "presence", "social"]
}

// Transport only — streams, events, subscribe, replay, auth.
// No chat features, no rich presence, no reactions/typing/blocks.
target "herald" {
  dockerfile = "Dockerfile"
  tags       = ["${REGISTRY}/herald:${TAG}"]
  args = {
    HERALD_FEATURES = ""
  }
}

// Transport + chat engine — adds event edit/delete, reactions, read cursors,
// typing indicators, user blocks, and block-based fanout filtering.
target "chat" {
  dockerfile = "Dockerfile"
  tags       = ["${REGISTRY}/chat:${TAG}"]
  args = {
    HERALD_FEATURES = "chat"
  }
}

// Transport + presence engine — adds manual presence overrides (away, dnd,
// offline), per-override expiry ("until Monday 9am"), watchlist with override
// awareness, batch presence queries, admin presence API, and WAL-persisted
// overrides that survive restarts.
target "presence" {
  dockerfile = "Dockerfile"
  tags       = ["${REGISTRY}/presence:${TAG}"]
  args = {
    HERALD_FEATURES = "presence"
  }
}

// Transport + chat + presence — the full social/messaging stack.
target "social" {
  dockerfile = "Dockerfile"
  tags       = ["${REGISTRY}/social:${TAG}"]
  args = {
    HERALD_FEATURES = "chat,presence"
  }
}
