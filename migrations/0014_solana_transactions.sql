CREATE TABLE IF NOT EXISTS solana_transactions (
    signature               TEXT         PRIMARY KEY,
    created_at              TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);



