CREATE TABLE IF NOT EXISTS feeds (
    feed_id INTEGER NOT NULL,
    user_name INTEGER NOT NULL,
    retweet_id INTEGER NOT NULL,
    retweet_user_name TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (CAST(strftime('%s','now') AS INTEGER)),
    feed_at INTEGER NOT NULL,
    twitter_url TEXT NOT NULL,
    contents TEXT,
    reply_to_feed_id INTEGER,
    reply_to_user_name TEXT,
    UNIQUE (feed_id, user_name, retweet_id, retweet_user_name)
    PRIMARY KEY (feed_id, user_name, retweet_id, retweet_user_name)
);