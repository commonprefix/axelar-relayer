CREATE TYPE xrpl_queued_transaction_status as ENUM ('queued', 'confirmed', 'dropped', 'expired');

CREATE TABLE IF NOT EXISTS xrpl_queued_transactions (
    tx_hash TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    status xrpl_queued_transaction_status NOT NULL,
    retries INTEGER DEFAULT 0,
    submitted TIMESTAMP DEFAULT now() NOT NULL,
    last_checked TIMESTAMP DEFAULT NULL
);