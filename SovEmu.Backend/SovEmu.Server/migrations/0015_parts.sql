-- What a multi-part game's files are (`disc`, `disk`, `tape`, `side` or `part`; see
-- library.rs): the clients show its count with the right icon. NULL for a single file.
ALTER TABLE games ADD COLUMN parts TEXT;
