CREATE TABLE IF NOT EXISTS media (
    feed_id INTEGER NOT NULL,
    media_id INTEGER NOT NULL,
    media_type TEXT NOT NULL, -- Image/Video/Audio
    media_url TEXT NOT NULL,
    file_path TEXT NOT NULL, -- path to zip
    media_path TEXT NOT NULL, -- path inside zip
    thumbnail BLOB, -- png thumbnail
    deleted_at INTEGER,
    FOREIGN KEY (file_path) REFERENCES files (file_path),
    UNIQUE (feed_id, media_url),
    PRIMARY KEY (feed_id, media_id)
);