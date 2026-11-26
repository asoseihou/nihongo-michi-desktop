-- Nihongo Michi desktop — schema v2: sync between PCs.

-- Deleted records (SRS cards, agenda events, "done" marks), so a deletion
-- reaches the other PCs instead of the old copy coming back.
CREATE TABLE tombstones (
  kind TEXT NOT NULL,                     -- card | event | occ
  id   TEXT NOT NULL,                     -- occ: "<event_id>|<occ>"
  at   INTEGER NOT NULL,
  PRIMARY KEY (kind, id)
);

-- Version of each record at the last sync ("doc:profile", "day:2026-11-26",
-- "card:<id>", "event:<id>", "occ:<event>|<occ>"). Tells a change made here
-- from a change made on the other PC.
CREATE TABLE sync_base (
  key TEXT PRIMARY KEY,
  t   INTEGER NOT NULL
);
