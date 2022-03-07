CREATE UNIQUE INDEX IF NOT EXISTS media_ids_idx
ON media(feed_id, media_id);