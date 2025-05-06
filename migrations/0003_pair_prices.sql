CREATE TABLE IF NOT EXISTS pair_prices (
    pair	TEXT NOT NULL,
    price	TEXT NOT NULL,
    updated_at	TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (pair)
);