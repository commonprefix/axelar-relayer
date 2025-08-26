CREATE TYPE subscriber_mode as ENUM ('stream', 'poll');

CREATE TABLE IF NOT EXISTS solana_subscriber_cursors (
    context	TEXT NOT NULL,
    signature	TEXT NOT NULL,
    updated_at	TIMESTAMPTZ NOT NULL DEFAULT now(),
    mode subscriber_mode NOT NULL,
    PRIMARY KEY (context, mode)
);