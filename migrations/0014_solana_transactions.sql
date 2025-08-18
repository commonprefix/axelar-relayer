-- Reuse existing enums: status_enum, source_enum (created in earlier migrations)

CREATE TABLE IF NOT EXISTS solana_transactions (
    signature               TEXT         PRIMARY KEY,
    signatures              TEXT[]       DEFAULT NULL,
    tx                      TEXT         NOT NULL,
    status                  status_enum  NOT NULL,
    source                  source_enum  NOT NULL,
    confirmation_status     TEXT         DEFAULT NULL,
    verify_task             TEXT         DEFAULT NULL,
    verify_tx               TEXT         DEFAULT NULL,
    quorum_reached_task     TEXT         DEFAULT NULL,
    route_tx                TEXT         DEFAULT NULL,
    block_time              TIMESTAMPTZ  DEFAULT NULL,
    logs                    TEXT[]       NOT NULL DEFAULT '{}',
    ixs                     JSONB        NOT NULL DEFAULT '[]',
    inner_ixs               JSONB        NOT NULL DEFAULT '[]',
    slot                    BIGINT       NOT NULL,
    fee_lamports            BIGINT       NOT NULL,
    meta_err                JSONB        DEFAULT NULL,
    version                 SMALLINT     NOT NULL,
    loaded_addresses        JSONB        DEFAULT NULL,
    account_keys            TEXT[]       NOT NULL DEFAULT '{}',
    payer                   TEXT         DEFAULT NULL,
    pre_post_balances       JSONB        DEFAULT NULL,
    pre_post_token_balances JSONB        DEFAULT NULL,
    compute_units           BIGINT       DEFAULT NULL,
    created_at              TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS solana_transactions_created_at_idx
    ON solana_transactions (created_at);

CREATE INDEX IF NOT EXISTS solana_transactions_expired_filter_idx
    ON solana_transactions (created_at)
    WHERE verify_tx IS NOT NULL AND quorum_reached_task IS NULL;

-- Query accelerators
CREATE INDEX IF NOT EXISTS solana_transactions_slot_idx
    ON solana_transactions (slot);

CREATE INDEX IF NOT EXISTS solana_transactions_block_time_idx
    ON solana_transactions (block_time);

CREATE INDEX IF NOT EXISTS solana_transactions_confirmation_status_idx
    ON solana_transactions (confirmation_status);

CREATE INDEX IF NOT EXISTS solana_transactions_payer_idx
    ON solana_transactions (payer);

-- GIN indexes for JSONB/array containment queries
CREATE INDEX IF NOT EXISTS solana_transactions_ixs_gin_idx
    ON solana_transactions USING GIN (ixs);

CREATE INDEX IF NOT EXISTS solana_transactions_inner_ixs_gin_idx
    ON solana_transactions USING GIN (inner_ixs);

CREATE INDEX IF NOT EXISTS solana_transactions_account_keys_gin_idx
    ON solana_transactions USING GIN (account_keys);


