CREATE TABLE IF NOT EXISTS messages_with_payload (
    cc_id	                TEXT NOT NULL,
    message_with_payload	TEXT NOT NULL,
    updated_at	            TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (cc_id)
);