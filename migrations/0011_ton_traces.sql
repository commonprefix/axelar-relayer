CREATE TABLE ton_traces (
        trace_id TEXT PRIMARY KEY,
        is_incomplete BOOLEAN,
        start_lt BIGINT,
        end_lt BIGINT,
        transactions JSONB,
        retries INT DEFAULT 10,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        updated_at TIMESTAMPTZ
);
