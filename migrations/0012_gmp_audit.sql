CREATE TABLE IF NOT EXISTS gmp_events (
    id TEXT PRIMARY KEY,
    message_id TEXT,
    event_type TEXT NOT NULL,
    event JSONB NOT NULL,
    response JSONB,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE
);

CREATE TABLE IF NOT EXISTS gmp_tasks (
    id TEXT PRIMARY KEY,
    chain TEXT NOT NULL,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    task_type TEXT NOT NULL,
    message_id TEXT,
    task JSONB NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE
);

-- For now, we are not going to create indices because they will slow down
-- writes and we will only ever use this table for debugging. Once indices
-- are needed, we can do something like:
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS gmp_events_message_id_idx ON gmp_events(message_id);
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS gmp_events_event_type_idx ON gmp_events(event_type);
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS gmp_tasks_message_id_idx ON gmp_tasks(message_id);
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS gmp_tasks_task_type_idx ON gmp_tasks(task_type);

-- Remember to add CONCURRENTLY is this is done on live db!
