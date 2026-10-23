-- 027_weather.sql — realm_state: Serverautoritatives Wetter (ANDORA-WEATHER_V1).
--
-- docs/Weltzeit_und_Wettersystem.md §10/§12: Weltzeit/Wetter are persistent
-- Realm-/Weltzustand in realm_state_<realm> (Realm-Daten, Datenbank_Architektur
-- §5/§10). A Realm-Neustart RESUMES this state; it is never reset to a fixed
-- standard (Tag1/12:00/CLEAR).
--
-- Scope: REALM-WIDE weather (single row, id=0). Regional Wetterregionen remain
-- an OPEN point (docs §18): the world data model has no weather-region
-- partition yet, so no parallel region mechanism is improvised here.
--
-- States implemented: sunny | rain | fog | storm (Auftrag §2). snow/cloudy stay
-- open (docs §8/§18).

CREATE TABLE IF NOT EXISTS weather_state (
  id              INT          NOT NULL PRIMARY KEY,
  -- current + next weather (transition target), stable content tokens.
  current         VARCHAR(16)  NOT NULL DEFAULT 'sunny',
  next            VARCHAR(16)  NOT NULL DEFAULT 'sunny',
  -- Intensität 0..100 (docs §8).
  intensity       INT          NOT NULL DEFAULT 40,
  -- Wallclock epoch-ms of the current state's begin + planned change.
  started_at_ms   BIGINT       NOT NULL DEFAULT 0,
  next_at_ms      BIGINT       NOT NULL DEFAULT 0,
  -- Monotoner change-counter for client sync / Coordinator (no Tick-spam).
  revision        BIGINT       NOT NULL DEFAULT 0,
  last_persist_at TIMESTAMP    NULL DEFAULT CURRENT_TIMESTAMP
);

-- Seed exactly one realm-wide row (id=0). Values are provisional and adapted
-- by later tests (docs §6/§14 Server-first); no final balancing.
INSERT INTO weather_state (id) VALUES (0);
