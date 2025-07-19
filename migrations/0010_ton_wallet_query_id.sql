CREATE TABLE ton_wallet_query_id (
     address TEXT NOT NULL PRIMARY KEY,
     shift INTEGER NOT NULL DEFAULT 0,
     bitnumber INTEGER NOT NULL DEFAULT 0,
     expires_at TIMESTAMPTZ NOT NULL ,
     created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
     updated_at TIMESTAMPTZ
)