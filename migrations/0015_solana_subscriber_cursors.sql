CREATE TABLE IF NOT EXISTS solana_subscriber_cursors (
    context	TEXT NOT NULL,
    signature	TEXT NOT NULL,
    updated_at	TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (context)
);