CREATE TYPE tx_type_enum AS ENUM ('TicketCreate', 'Payment', 'SignerListSet', 'TrustSet', 'Unexpected');
CREATE TYPE status_enum AS ENUM ('Detected', 'Initialized', 'Verified', 'Routed');
CREATE TYPE source_enum AS ENUM ('Prover', 'User');

CREATE TABLE IF NOT EXISTS xrpl_transactions (
    tx_hash                TEXT          PRIMARY KEY,
    tx_type                tx_type_enum  NOT NULL,
    message_id             TEXT          DEFAULT NULL,
    status                 status_enum   NOT NULL,
    verify_task            TEXT         DEFAULT NULL,
    verify_tx              TEXT          DEFAULT NULL,
    quorum_reached_task    TEXT         DEFAULT NULL,
    route_tx               TEXT          DEFAULT NULL,
    source                 source_enum   NOT NULL,
    sequence               BIGINT        DEFAULT NULL
);