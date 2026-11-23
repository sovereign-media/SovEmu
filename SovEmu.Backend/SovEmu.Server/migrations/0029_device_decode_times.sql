-- What each sign-in's device measured its video decoder to take a frame, by codec: the mean of
-- its latest session with that codec, as its once-a-second stats reported it. The server
-- prices a codec for a device by this where it has it, over what the device declares
-- (`sign_in_capabilities`): a declared hardware decoder can still be the slow one. Rows of
-- sign-ins that have ended go when a new measurement is kept.
CREATE TABLE device_decode_times (
    family_id   TEXT NOT NULL,
    codec       TEXT NOT NULL,
    decode_ms   REAL NOT NULL,
    samples     INTEGER NOT NULL,
    measured_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (family_id, codec)
);
