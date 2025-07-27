CREATE TABLE IF NOT EXISTS gmp_events (
    id BIGSERIAL PRIMARY KEY,
    event_id TEXT NOT NULL,
    message_id TEXT,
    event_type TEXT NOT NULL,
    event JSONB NOT NULL,
    response JSONB,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE
);

CREATE INDEX CONCURRENTLY IF NOT EXISTS gmp_events_event_id_idx ON gmp_events(event_id);

CREATE TABLE IF NOT EXISTS gmp_tasks (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT NOT NULL,
    chain TEXT NOT NULL,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    task_type TEXT NOT NULL,
    message_id TEXT,
    task JSONB NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE
);

CREATE INDEX CONCURRENTLY IF NOT EXISTS gmp_tasks_task_id_idx ON gmp_tasks(task_id);