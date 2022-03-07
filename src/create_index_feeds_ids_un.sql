CREATE INDEX IF NOT EXISTS feeds_ids_un_idx
ON feeds(feed_id, retweet_id, user_name);