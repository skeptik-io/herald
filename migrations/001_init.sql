-- Herald multi-tenant schema

CREATE TABLE tenants (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    jwt_secret  TEXT NOT NULL,
    jwt_issuer  TEXT,
    plan        TEXT NOT NULL DEFAULT 'free',
    config      JSONB DEFAULT '{}',
    created_at  BIGINT NOT NULL
);

CREATE TABLE api_tokens (
    token       TEXT PRIMARY KEY,
    tenant_id   TEXT NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    created_at  BIGINT NOT NULL
);
CREATE INDEX idx_api_tokens_tenant ON api_tokens(tenant_id);

CREATE TABLE rooms (
    tenant_id       TEXT NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    id              TEXT NOT NULL,
    name            TEXT NOT NULL,
    encryption_mode TEXT NOT NULL DEFAULT 'plaintext',
    meta            JSONB,
    created_at      BIGINT NOT NULL,
    PRIMARY KEY (tenant_id, id)
);

CREATE TABLE members (
    tenant_id   TEXT NOT NULL,
    room_id     TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    role        TEXT NOT NULL DEFAULT 'member',
    joined_at   BIGINT NOT NULL,
    PRIMARY KEY (tenant_id, room_id, user_id),
    FOREIGN KEY (tenant_id, room_id) REFERENCES rooms(tenant_id, id) ON DELETE CASCADE
);

CREATE TABLE messages (
    id          TEXT PRIMARY KEY,
    tenant_id   TEXT NOT NULL,
    room_id     TEXT NOT NULL,
    seq         BIGINT NOT NULL,
    sender      TEXT NOT NULL,
    body        TEXT NOT NULL,
    meta        JSONB,
    sent_at     BIGINT NOT NULL,
    expires_at  BIGINT NOT NULL
);
CREATE INDEX idx_messages_tenant_room_seq ON messages(tenant_id, room_id, seq);
CREATE INDEX idx_messages_expires ON messages(expires_at);

CREATE TABLE cursors (
    tenant_id   TEXT NOT NULL,
    room_id     TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    seq         BIGINT NOT NULL,
    updated_at  BIGINT NOT NULL,
    PRIMARY KEY (tenant_id, room_id, user_id)
);
