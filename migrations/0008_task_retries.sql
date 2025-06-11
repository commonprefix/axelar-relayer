CREATE TABLE IF NOT EXISTS task_retries (
    message_id	                TEXT NOT NULL,
    retries	                INTEGER NOT NULL DEFAULT 0,
    updated_at	TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (message_id)
);