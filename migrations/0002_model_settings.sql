ALTER TABLE tv_agent_settings
    ADD COLUMN IF NOT EXISTS model_endpoint TEXT,
    ADD COLUMN IF NOT EXISTS model TEXT,
    ADD COLUMN IF NOT EXISTS tvbox_configs TEXT[];
