-- Reuse existing enums: status_enum, source_enum (created in earlier migrations)

CREATE TABLE IF NOT EXISTS solana_transactions (
    signature             TEXT         PRIMARY KEY,
    tx                    TEXT         NOT NULL,
    status                status_enum  NOT NULL,
    source                source_enum  NOT NULL,
    verify_task           TEXT         DEFAULT NULL,
    verify_tx             TEXT         DEFAULT NULL,
    quorum_reached_task   TEXT         DEFAULT NULL,
    route_tx              TEXT         DEFAULT NULL,
    timestamp             TIMESTAMPTZ  DEFAULT NULL,
    logs                  TEXT[]       NOT NULL DEFAULT '{}',
    ixs                   JSONB        NOT NULL,
    slot                  BIGINT       NOT NULL,
    cost_in_lamports      BIGINT       NOT NULL,
    created_at            TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS solana_transactions_created_at_idx
    ON solana_transactions (created_at);

CREATE INDEX IF NOT EXISTS solana_transactions_expired_filter_idx
    ON solana_transactions (created_at)
    WHERE verify_tx IS NOT NULL AND quorum_reached_task IS NULL;


