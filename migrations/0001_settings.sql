CREATE TABLE IF NOT EXISTS tv_agent_settings (
    tenant_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    secret BYTEA,
    PRIMARY KEY (tenant_id, user_id)
);
