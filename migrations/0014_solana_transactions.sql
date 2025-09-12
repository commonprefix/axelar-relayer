CREATE TABLE IF NOT EXISTS solana_transactions (
    signature               TEXT         PRIMARY KEY,
    slot                    BIGINT       NOT NULL,
    logs                    TEXT[],
    ixs                     TEXT[],
    events                  TEXT[],
    cost_units              BIGINT       DEFAULT 0,
    retries                 INTEGER      DEFAULT 10,
    created_at              TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);
