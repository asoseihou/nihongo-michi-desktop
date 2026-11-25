-- Nihongo Michi desktop — schema v1
-- Documents mirror the artifact's Store (profile, journey, kana, mined,
-- calendar, desk…); study days and SRS cards get their own tables.

CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- Whole JSON documents: profile, journey, kana, mined, calendar, desk, …
CREATE TABLE docs (
  name       TEXT PRIMARY KEY,
  data       TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

-- One row per study day (the day starts at 04:00). `data` is the full
-- `day:YYYY-MM-DD` document; xp/minutes are copies for quick queries.
CREATE TABLE days (
  day        TEXT PRIMARY KEY,
  xp         REAL NOT NULL DEFAULT 0,
  minutes    REAL NOT NULL DEFAULT 0,
  data       TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

-- SRS cards: [due, ivl, ease, reps, lapses, state, step]
CREATE TABLE cards (
  id         TEXT PRIMARY KEY,
  due        REAL NOT NULL,
  ivl        REAL NOT NULL,
  ease       REAL NOT NULL,
  reps       REAL NOT NULL,
  lapses     REAL NOT NULL,
  state      INTEGER NOT NULL,
  step       REAL NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX cards_state_due ON cards(state, due);

-- Agenda events (series). Times are local wall clock ("floating").
CREATE TABLE events (
  id         TEXT PRIMARY KEY,
  kind       TEXT NOT NULL,               -- rotina | estudo | vlog | viagem | meu
  title      TEXT NOT NULL,
  jp         TEXT NOT NULL DEFAULT '',
  notes      TEXT NOT NULL DEFAULT '',
  all_day    INTEGER NOT NULL DEFAULT 0,
  date       TEXT NOT NULL,               -- first date, YYYY-MM-DD
  time       TEXT NOT NULL DEFAULT '',    -- HH:MM
  dur        INTEGER NOT NULL,            -- minutes
  rrule      TEXT,                        -- FREQ=…;BYDAY=…;UNTIL=…
  exdates    TEXT NOT NULL DEFAULT '[]',  -- JSON: skipped occurrence dates
  reminders  TEXT NOT NULL DEFAULT '[]',  -- JSON: minutes before start
  color      TEXT,
  go         TEXT,
  source     TEXT NOT NULL DEFAULT 'user',-- user | rotina | viagem
  source_id  TEXT,
  overrides  TEXT NOT NULL DEFAULT '[]',  -- JSON: moved occurrences
  off        INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX events_source ON events(source, source_id);

-- Occurrences marked as done from the agenda or the reminder popup.
CREATE TABLE occ_done (
  event_id TEXT NOT NULL,
  occ      TEXT NOT NULL,                 -- original occurrence date
  done_at  INTEGER NOT NULL,
  PRIMARY KEY (event_id, occ)
);

-- Reminder history (notification centre). `key` = event|occ|minutesBefore,
-- so each reminder fires once.
CREATE TABLE notifications (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  key          TEXT NOT NULL UNIQUE,
  event_id     TEXT NOT NULL,
  occ          TEXT NOT NULL,
  title        TEXT NOT NULL,
  jp           TEXT NOT NULL DEFAULT '',
  body         TEXT NOT NULL DEFAULT '',
  kind         TEXT NOT NULL,
  color        TEXT NOT NULL,
  go           TEXT,
  at           TEXT NOT NULL DEFAULT '',
  dur          INTEGER NOT NULL DEFAULT 0,
  fire_at      INTEGER NOT NULL,
  status       TEXT NOT NULL,             -- shown | done | snoozed | dismissed | missed | silenced | opened
  snooze_until INTEGER,
  read         INTEGER NOT NULL DEFAULT 0,
  acted_at     INTEGER
);
CREATE INDEX notifications_fire ON notifications(fire_at);
CREATE INDEX notifications_status ON notifications(status);
