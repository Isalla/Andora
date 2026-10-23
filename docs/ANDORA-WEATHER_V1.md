# ANDORA – WEATHER_V1 – Abschlussbericht

Serverautoritatives Wettersystem mit tatsächlichen Auswirkungen auf die
Spielmechaniken. Erweitert Auftrag, WIP. **Kein Commit/Push/Deployment**
performanced (Projektgrenzen §7). Includes the WEATHER_V1_1_SEASONS extension
(Schnee, Bewölkung, Kalenderjahresseasons) — see section 7.

## 1. Implementierte Funktionen

### 1.1 Realm (Rust, `src/realm-rs`)

* **`src/weather.rs` (NEU)** – Autoritative Wettercore.
  * `WeatherState`: `Sunny`/`Rain`/`Fog`/`Storm` (Sonnig/Regen/Nebel/Gewitter,
    Auftrag §2; = CLEAR/RAIN/FOG/STORM-Subset von docs §8).
  * `Weather` struct: `current`, `next`, `intensity` (0..100), `started_at_ms`,
    `next_at_ms`, `revision` (Monotoner Change-Counter).
  * `advance(world, cfg, rng, now)` – Realm-managed Wechsel: Transition at
    `next_at`, deterministic RNG-Picking (no self-Transition), new duration+
    intensity, `revision++` only on a genuine change.
  * Factors: `sight_factor`, `aggro_factor`, `respawn_factor`, `pickup_factor`.
  * `json()`/`frame()` – S2C WEATHER-Payload/Frame.
  * `coordinator_context()` – Realm-decided world-string for the KI.
  * `WeatherRow` + `from_row`/`to_row` – Persistenz (fail-closed on unknown keys).
* **`config.rs`** – `WeatherCfg` (`WEATHER_TRANSITION_MIN_MS`,
  `WEATHER_TRANSITION_MAX_MS`, `WEATHER_PERSIST_INTERVAL_MS`) + `Config.weather`.
* **`world.rs`** – `World.weather` field; `world_tick` scales the AOFB broadcast
  radius by `sight_factor` (serverseitig Sicht-Rule); `broadcast_weather()` sends
  the WEATHER-Frame to all online players.
* **`protocol.rs`** – S2C `WEATHER = 26` (no C2S: the client never sets weather).
* **`main.rs`** – Startup-Load from `realm_state` (resumed, NOT reset, docs §12);
  `weather_tick` in the 10-Hz ticker; broadcast on change; periodic persist.
* **`handlers.rs`** – WEATHER-Push after WELCOME on login (docs §13).
* **`npc.rs`** – `aggro_range * aggro_factor` (Kreaturenverhalten);
  **`combat/mod.rs`** – `respawn_ms * respawn_factor` (Spawn-Condition).
* **`handlers.rs` (pickup)** – `pickup_radius * pickup_factor` (Sammelreach).
* **`quest.rs` / `npc_quest.rs`** – Wetter-dependent Quest-Conditions:
  `QuestDefinition.weather` (Lua `requirements.weather`), gated via the
  EXISTING availability path (`prerequisites_met`/`state_for_at`/`accept_at`).
* **`db.rs`** – `load_weather_state`/`save_weather_state` (single row id=0).
* **`migrations/027_weather.sql`** – `weather_state` table (realm-wide).

### 1.2 Coordinator (Go, `src/coordinator`)

* **`job.go`** – `jobContext.Weather` (`json:"weather"`).
* **`validate.go`** – renders a `[Weather]` block into the Ollama prompt.
* **`conversation.rs` (Realm)** – fills `context.world` (descriptive) AND
  `context.weather` (short key) from the autoritative state. The Coordinator
  adds NO world state of its own (job.go/queue.go contract respected).

### 1.3 Client (Godot 3.6)

* **`shared/protocol.gd`** – S2C `WEATHER = 26` (mirrors `protocol.rs`).
* **`testclient/tc_main.gd`** – WEATHER-Frame → `world.set_weather(...)`.
* **`testclient/tc_world.gd`** – weather overlay (fog/storm tint, rain
  particles, storm flash) + sight-limited vignette from `sight_factor`. The
  existing Punktgrafik (circles) is KEPT; Pi4-cheap (capped particles, no
  shaders, drawn in `_draw()`).

## 2. tatsächliche Wettereffekte (server-checked)

| Wetter | Sicht (AOFB) | Aggro/Spawn | Sammel (Pickup) | Respawn |
|---|---|---|---|---|
| Sonnig | 1.00 | 1.00 | 1.00 | 1.0× |
| Regen  | ~0.85 | 0.90 | 0.80 | 1.0× |
| Nebel  | ~0.55 | 0.60 | 0.70 | 1.1× |
| Gewitter | ~0.70 | 0.75 | 0.70 | 1.5× |

* Intensität modulae the reduction slightly; sight/aggro/pickup NEVER above
  1.0 (weather never extends), respawn NEVER below 1.0 (only delays).
* Quests with a `weather` requirement are AVAILABLE/acceptable only in that
  weather (server-side `NotAvailable` otherwise).
* NO blanket Kampfwertchanges, NO new Gegenstands (Auftrag §3 respected).

## 3. Testresults

* `cargo test`: **997 passed; 0 failed** (was 994 before; +3 weather tests,
  migration-gapless test updated to 027).
* New weather tests (`weather.rs`): state roundtrip, factor bounds,
  advance-at-boundary + revision, no-self-transition, persisted-row-resume
  (not reset), unknown-key fail-closed, S2C-carries-only-realm-state,
  coordinator-context.
* `world.rs::fog_reduces_visible_radius` – serverseitig Sicht-Rule (15 m not
  seen in fog at aofb 20).
* `quest.rs::weather_gated_quest_available_only_in_matching_weather` –
  Hidden/Available + accept-gate + weather-agnostic caller.
* Go coordinator: `go build ./...` + `go test ./...` → **ok**.

## 4. Abgrenzung / Reconnect / Manipulation

* **Reconnect during a weather change:** the HELLO/login path pushes the
  CURRENT WEATHER (handlers.rs) after WELCOME; the existing Takeover/Adopted
  tests keep the RAM-Player authoritative. Weather is realm-wide, so a
  reconnecting player receives the current state (docs §13).
* **Invalid/manipulated client input:** weather is S2C-ONLY. There is NO C2S
  weather message; `security.rs::is_known_c2s` is unchanged, so a client cannot
  send/set weather. All effects are computed from the autoritative RAM state.

## 5. noch offene Punkte (NOT improvised – no parallel mechanisms)

* **Regional Wetterregionen** (docs §7/§18): the world data model has NO
  weather-region partition (only `zone_id`, not a weather region). Weather is
  modelled REALM-WIDE. Regional climate stays open.
* **Schnee / Cloudy / Jahresseasons / Weltzeit-Coupling** (docs §8/§18): not
  implemented (Auftrag named only the 4 states).
* **Availability of SPECIFIC resources by weather** (Auftrag §3): the existing
  gathering-ish system is Loot (drops + pickup radius); a per-resource weather
  availability table does not exist in the content model → named open, only the
  pickup-REACH is weather-influenced.
* **KI-dynamic-Quest weather:** the KI-Personalisation-Questsystem
  (docs/KI_Personalisierungs_Questsystem_v2.md) is design-only; weather is
  exposed via the same `jobContext` for when it is implemented.
* **Client weather VISUALS are representational only** (docs §3/§15): the
  server is the source of truth; the Godot overlay never decides state.

## 6. Files touched

Realm: `weather.rs` (new), `protocol.rs`, `config.rs`, `world.rs`, `main.rs`,
`handlers.rs`, `npc.rs`, `combat/mod.rs`, `quest.rs`, `npc_quest.rs`,
`conversation.rs`, `db.rs`, `migrations.rs`, `migrations/027_weather.sql` (new).
Coordinator: `job.go`, `validate.go`. Client: `shared/protocol.gd`,
`testclient/tc_main.gd`, `testclient/tc_world.gd`.

---

# ANDORA – WEATHER_V1_1_SEASONS – Abschlussbericht (Extension)

Extends the existing Weather V1 with Schnee, Bewölkung and reale Kalender-
jahresseasons. **Kein Commit/Push/Deployment** (Projektgrenzen).

## 1. Implementierte Funktionen (V1.1)

* **`weather.rs`** – `WeatherState` extended to SIX states: `Snow` (Schnee) and
  `Cloudy` (Bewölkung) added; `key()`/`from_key()`/`all()` updated (all() now
  `[WeatherState;6]`, canonical index order = season-weight-table order
  [Sunny,Rain,Fog,Storm,Snow,Cloudy]).
* **`Season` enum** (Spring/Summer/Autumn/Winter) + `key()`/`from_key()`/
  `from_month()`.
* **UTC-calendar derivation:** `utc_month_from_epoch_ms(ms)` (closed-form civil-
  date algorithm, Howard Hinnant `civil_from_days`; leap-year-correct) +
  `season_from_epoch_ms(ms)`. Frühling 3–5, Sommer 6–8, Herbst 9–11, Winter
  12–2.
* **Season-dependent probabilities:** `season_weights(Season) -> [u32;6]`
  (relative weights; 0 forbids a state). INVARIANT: every season has >= 2
  non-zero weights → NO season ever allows only one state (tested).
* **`advance`** now uses a shared `pick_season_weighted(current, season, rng)`
  (season-weighted, excludes current + weight-0 states; fail-closed uniform
  fallback). Season re-derived from `now_ms` each change.
* **`Weather.season`** field (DERIVED from the UTC clock, NOT persisted;
  recomputed at startup/`from_row`/`advance`).
* **Dev-side simulation:** `simulate(seed, cfg, start_ms, steps) -> Vec<(Weather,
  Season)>` — pure, deterministic, NEVER reads `SystemTime`, NEVER mutates a
  `World` (V1.1 §8: production state + system clock untouched).

## 2. tatsächliche Wettereffekte (new states)

| Wetter | Sight | Aggro | Pickup | Respawn |
|---|---|---|---|---|
| Bewolung (Cloudy) | 0.90 | 0.95 | 0.90 | 1.0× |
| Schnee (Snow)     | 0.60 | 0.70 | 0.60 | 1.3× |

* NO combat-/loot-/quest RULES changed: only the existing weather FACTORS gained
  values for the two new states; the quest weather-gate now also accepts
  `snow`/`cloudy` keys (same code path).

## 3. Season-Weather-Weights (provisional)

| Season | Sunny | Rain | Fog | Storm | Snow | Cloudy |
|---|---|---|---|---|---|---|
| Spring | 40 | 25 | 10 | 15 | 5 | 25 |
| Summer | 55 | 20 | 10 | 30 | 0 | 20 |
| Autumn | 20 | 35 | 25 | 20 | 5 | 35 |
| Winter | 10 | 10 | 25 | 10 | 45 | 30 |

Summer forbids snow; Winter favours it. Every row has >= 2 non-zero.

## 4. Coordinator + Client exposure (V1.1)

* **Coordinator (Go):** `jobContext.Season` (`json:"season"`) + `[Season]` block
  in `validate.go` prompt. Realm fills `context.season` in `conversation.rs`.
* **Protocol:** S2C WEATHER payload gains `season` (additive field; older
  clients ignore unknown keys → compatible). Comment updated to
  `sunny|rain|fog|storm|snow|cloudy` + `season`.
* **Godot 3.6:** `tc_main.gd` passes `season` to `set_weather`; `tc_world.gd`
  adds snow particles (slow-fall), cloudy tint, and a subtle season tint. Punkt-
  grafik kept; Pi4-cheap (capped particles, no shaders).

## 5. Testresults (V1.1)

* `cargo test`: **1004 passed; 0 failed** (V1 had 997; +7 season tests).
* New tests: `season_keys_roundtrip`, `month_to_season_boundaries`,
  `utc_month_matches_calendar_including_leap_year` (2024-02-29 leap, 2023-03-01,
  2024-12-31), `no_season_allows_only_one_state`, `snow_and_cloudy_have_factors`,
  `simulate_is_deterministic_and_clock_free` (no snow in simulated summer),
  `weather_frame_carries_season`.
* Go coordinator `build`+`test` → ok. Protocol WEATHER=26 both sides.

## 6. Persistenz / Reconnect / Protokoll-Kompatibilität (V1.1)

* **Persistence:** weather row unchanged (current/next/intensity/started/next_at/
  revision). SEASON is NOT persisted — it is a pure function of the UTC clock,
  recomputed on load (docs §12: position resumed, not reset).
* **Reconnect:** login HELLO pushes the current WEATHER incl. `season`; the
  Takeover/Adopted RAM-authority tests are untouched.
* **Protocol compatibility:** `season` is an ADDITIVE payload key; the message
  ID (26) and all existing keys stay → older clients keep working.

## 7. noch offene Punkte (V1.1, NOT improvised)

* **Regional climate:** weather stays REALM-WIDE (no weather-region partition in
  the data model). Season is global (UTC calendar). Regional per-climate season
  offsets remain open (docs §18).
* **Configurable season weights:** currently a documented provisional table in
  `weather.rs`; per-season config.env override not yet wired (open).
* **Weltzeit/Tag-Nacht coupling:** the Realm has no separate world-clock; seasons
  use the real UTC calendar (Auftrag §3). A world-time-based season would need
  that system first.
* **Schnee/Bewolung gameplay depth:** only the four existing factors are
  influenced; no new mechanics invented.
