ALTER TABLE xrpl_transactions
    ADD COLUMN created_at TIMESTAMPTZ DEFAULT NULL;
