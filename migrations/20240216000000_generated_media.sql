CREATE TABLE IF NOT EXISTS generated_media (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    endpoint_path VARCHAR(255) NOT NULL,
    prompt TEXT NOT NULL,
    prompt_hash VARCHAR(64) NOT NULL,
    s3_key VARCHAR(512) NOT NULL,
    s3_url TEXT NOT NULL,
    media_type VARCHAR(32) NOT NULL,
    file_size_bytes BIGINT NOT NULL DEFAULT 0,
    payer_address VARCHAR(42),
    payment_tx VARCHAR(66),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '30 days')
);

CREATE INDEX IF NOT EXISTS idx_generated_media_cache_lookup
    ON generated_media (prompt_hash, endpoint_path);

CREATE INDEX IF NOT EXISTS idx_generated_media_expires_at
    ON generated_media (expires_at);
