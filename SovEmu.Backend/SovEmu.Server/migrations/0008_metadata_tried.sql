-- The services asked about a game, e.g. "fbneo,screenscraper". A game no service found is
-- asked again once a service it was not asked is configured, so adding credentials later fills
-- it in without anyone having to fetch everything again.
ALTER TABLE game_metadata ADD COLUMN tried TEXT NOT NULL DEFAULT '';
