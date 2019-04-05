CREATE TABLE
IF NOT EXISTS push_record
(
    uaid               TEXT     NOT NULL,
    channel_id         TEXT     NOT NULL UNIQUE,
    endpoint           TEXT     NOT NULL UNIQUE,
    scope              TEXT     NOT NULL,
    key                TEXT     NOT NULL,
    ctime              INTEGER  NOT NULL,
    app_server_key     TEXT,
    native_id          TEXT,
    PRIMARY KEY
(uaid, channel_id)
);

CREATE UNIQUE INDEX
IF NOT EXISTS channel_id_idx ON push_record
(channel_id);

CREATE TABLE
IF NOT EXISTS meta_data
(
    key                TEXT    NOT NULL UNIQUE,
    value              TEXT    NOT NULL
);

CREATE TABLE
IF NOT EXISTS delivery_data
(
    channel_id         TEXT    NOT NULL UNIQUE,
    svc_name           TEXT    NOT NULL UNIQUE,
    is_system          INTEGER DEFAULT 0,
    quota              INTEGER DEFAULT 0,
    last_recvd         INTEGER,
    recv_count         INTEGER DEFAULT 0,
    recipient_info,
    PRIMARY KEY
(channel_id)
);