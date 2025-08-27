CREATE TABLE IF NOT EXISTS solana_signatures (
    signature               TEXT         PRIMARY KEY,
    created_at              TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);



