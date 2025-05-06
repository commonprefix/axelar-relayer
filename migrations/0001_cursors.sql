CREATE TABLE IF NOT EXISTS subscriber_cursors (
    chain	TEXT NOT NULL,
    context	TEXT NOT NULL,
    height	BIGINT  NOT NULL,
    updated_at	TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chain, context)
);

CREATE TABLE IF NOT EXISTS distributor_cursors (
    chain	TEXT NOT NULL,
    context	TEXT NOT NULL,
    task_id	TEXT NOT NULL,
    updated_at	TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chain, context)
);