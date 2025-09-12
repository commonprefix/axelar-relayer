CREATE TYPE account_poller_enum AS ENUM ('gas_service', 'gateway');

CREATE TABLE IF NOT EXISTS solana_subscriber_cursors (
    context	TEXT NOT NULL,
    account_type account_poller_enum NOT NULL,
    signature	TEXT NOT NULL,
    updated_at	TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (context, account_type)
);

