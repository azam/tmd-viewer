CREATE TABLE IF NOT EXISTS files (
    file_path TEXT NOT NULL,
    added_at INTEGER NOT NULL DEFAULT (CAST(strftime('%s','now') AS INTEGER)),
    scan_started_at INTEGER,
    scan_ended_at INTEGER,
    PRIMARY KEY (file_path)
);