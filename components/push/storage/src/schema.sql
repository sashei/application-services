-- XXX: maybe push_sub?
-- MAKE SURE TO SEPARATE EACH COMMAND WITH "\n\n"!

CREATE TABLE IF NOT EXISTS push_record (
    uaid               TEXT     NOT NULL,
    channel_id         TEXT     NOT NULL UNIQUE,
    endpoint           TEXT     NOT NULL UNIQUE,
    scope              TEXT     NOT NULL,
    key                TEXT     NOT NULL,
    ctime              INTEGER  NOT NULL,
    app_server_key     TEXT,
    native_id          TEXT,
    PRIMARY KEY (uaid, channel_id)
);

CREATE UNIQUE INDEX channel_id_idx ON push_record(channel_id);

CREATE TABLE IF NOT EXISTS meta_data (
    key                TEXT    NOT NULL UNIQUE,
    value              TEXT    NOT NULL
);