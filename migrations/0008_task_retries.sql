CREATE TABLE IF NOT EXISTS task_retries (
    msg_id	                TEXT NOT NULL,
    retries	                INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (msg_id)
);