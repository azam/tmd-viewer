CREATE UNIQUE INDEX IF NOT EXISTS media_unique_idx
ON media(feed_id, media_url);