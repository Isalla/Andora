// weather — Serverautoritatives Wettersystem (docs/Weltzeit_und_Wettersystem.md).
//
// Leitgedanke (§2/§3): DER REALM bestimmt Wetterzustand und Wetterwechsel
// autoritativ. Der Client stellt densselben bestatzten Zustand dar und
// entscheidet es nicht. Keine parallelen Client-Welt existiert.
//
// Scope of THIS implementation (erweiterter Auftrag ANDORA-WEATHER_V1):
//   * Wetterzustsinnig/Regen/Nebel/Gewitter (§2 of Auftrag) — konzeptionelle
//     CLEAR/RAIN/FOG/STORM-Subset of docs §8. SNOW/CLOUDY remain OPEN (docs §18).
//   * Realm-managed Wechsel + Übergang + Intensität + Revision (§3, §9, §10).
//   * Serverseitig Gameplay-Auswirkungen, SO WEIT available systems support it:
//       - Sichtweite (AOFB-Radius, world.rs)            -> sight_factor
//       - Kreaturenverhalten (aggro_range, npc.rs)      -> aggro_factor
//       - Spawn-Condition (Respawn-Timer, npc.rs)       -> respawn_factor
//       - Sammel-/Pickupreach (loot pickup_radius)      -> pickup_factor
//       - Wetter-dependent Quest-Conditions (quest.rs)  -> weather requirement
//   * Persistenter Realm-Weltzustand (docs §10/§12): loaded at startup, saved
//     periodically; a Realm-Neustart does NOT reset to a fixed standard state.
//
// EXPLICITLY OPEN (docs §18, NOT improvised here — no parallel mechanisms):
//   * Regional Wetterregionen: the world data model has no weather-region
//     partition yet (zone_id exists but is not a weather region). Weather is
//     modelled REALM-WIDE. Regional climate is a named open point.
//   * Jahres seasons, Weltzeit/Tag-Nacht coupling, Schnee, Cloudy.
use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime};

use crate::combat::CombatRng;
use crate::combat::SplitMix64;
use crate::config::WeatherCfg;
use crate::protocol::{s2c, Frame};
use crate::world::World;

/// The six implemented weather states (V1 §2 + V1.1 §2). `key()` is the stable
/// wire/content token; `from_key()` parses it fail-closed (unknown -> None).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherState {
    Sunny,  // Sonnig   (docs §8 CLEAR)
    Rain,   // Regen    (docs §8 RAIN)
    Fog,    // Nebel    (docs §8 FOG)
    Storm,  // Gewitter (docs §8 STORM)
    Snow,   // Schnee   (docs §8 SNOW, added V1.1)
    Cloudy, // Bewölkung(docs §8 CLOUDY, added V1.1)
}

impl WeatherState {
    pub fn key(&self) -> &'static str {
        match self {
            WeatherState::Sunny => "sunny",
            WeatherState::Rain => "rain",
            WeatherState::Fog => "fog",
            WeatherState::Storm => "storm",
            WeatherState::Snow => "snow",
            WeatherState::Cloudy => "cloudy",
        }
    }

    pub fn from_key(key: &str) -> Option<WeatherState> {
        match key.trim().to_lowercase().as_str() {
            "sunny" => Some(WeatherState::Sunny),
            "rain" => Some(WeatherState::Rain),
            "fog" => Some(WeatherState::Fog),
            "storm" => Some(WeatherState::Storm),
            "snow" => Some(WeatherState::Snow),
            "cloudy" => Some(WeatherState::Cloudy),
            _ => None,
        }
    }

    /// All states in a stable order. The index order is the canonical order used
    /// by the season weight table (`season_weights`).
    pub fn all() -> [WeatherState; 6] {
        [
            WeatherState::Sunny,
            WeatherState::Rain,
            WeatherState::Fog,
            WeatherState::Storm,
            WeatherState::Snow,
            WeatherState::Cloudy,
        ]
    }
}

/// Kalender-Jahresseasons (V1.1 §3), derived from the UTC calendar month:
/// Frühling März–Mai, Sommer Juni–August, Herbst September–November,
/// Winter Dezember–February.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn key(&self) -> &'static str {
        match self {
            Season::Spring => "spring",
            Season::Summer => "summer",
            Season::Autumn => "autumn",
            Season::Winter => "winter",
        }
    }

    pub fn from_key(key: &str) -> Option<Season> {
        match key.trim().to_lowercase().as_str() {
            "spring" => Some(Season::Spring),
            "summer" => Some(Season::Summer),
            "autumn" => Some(Season::Autumn),
            "winter" => Some(Season::Winter),
            _ => None,
        }
    }

    /// Season from a UTC calendar month (1..12). Out-of-range months fall back
    /// to Winter (fail-closed, never panics).
    pub fn from_month(month: u32) -> Season {
        match month {
            3 | 4 | 5 => Season::Spring,
            6 | 7 | 8 => Season::Summer,
            9 | 10 | 11 => Season::Autumn,
            _ => Season::Winter, // 12,1,2 and any invalid
        }
    }
}

/// Derives the UTC calendar month (1..12) from epoch milliseconds. Uses the
/// closed-form civil-date algorithm (Howard Hinnant `civil_from_days`), which
/// handles leap years inherently (e.g. 2024-02-29 exists, 2023-02-29 does not).
/// Negative epochs (pre-1970) are handled via floor division.
pub fn utc_month_from_epoch_ms(ms: i64) -> u32 {
    let day_ms = 86_400_000_i64;
    // floor(ms / day_ms) -> days since 1970-01-01 (correct for negatives).
    let mut z = if ms >= 0 { ms / day_ms } else { (ms - day_ms + 1) / day_ms };
    z += 719_468; // shift epoch to 0000-03-01 (algorithm's era base)
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let r = z - era * 146_097; // [0, 146096]
    let yoe = (r - r / 1460 + r / 36_524 - r / 146_096) / 365; // [0,399]
    let doy = r - (365 * yoe + yoe / 4 - yoe / 100); // [0,365]
    let mp = (5 * doy + 2) / 153; // [0,11]
    // month: mp + (mp < 10 ? 3 : -9)
    let m = mp + (if mp < 10 { 3 } else { -9 });
    m as u32
}

/// Season from epoch milliseconds (UTC).
pub fn season_from_epoch_ms(ms: i64) -> Season {
    Season::from_month(utc_month_from_epoch_ms(ms))
}

/// Autoritative Realm-Weltzustand des Wetter (realm-wide, docs §7 open regions).
/// `revision` increments on every actual change so the client and the
/// Coordinator can detect a genuine change (no Tick-spam, mirrors VITALS/STATE
/// gating).
#[derive(Debug, Clone)]
pub struct Weather {
    pub current: WeatherState,
    pub next: WeatherState,
    /// Intensität 0..=100 (docs §8). Carried for client visuals + Coordinator.
    pub intensity: u8,
    /// Wallclock-Start of the current state (epoch ms) — persisted (docs §10).
    pub started_at_ms: i64,
    /// Wallclock at which the transition to `next` becomes effective.
    pub next_at_ms: i64,
    pub revision: u64,
    /// Current calendar season (derived from the UTC clock; NOT persisted).
    pub season: Season,
}

impl Weather {
    /// Deterministic seed state for a fresh Realm (docs §12: NOT a fixed
    /// standard reset — the persisted row is preferred at startup; this is
    /// only the fallback when no persisted row exists).
    pub fn initial(now_ms: i64, cfg: &WeatherCfg) -> Self {
        Weather {
            current: WeatherState::Sunny,
            next: WeatherState::Sunny,
            intensity: 40,
            started_at_ms: now_ms,
            next_at_ms: now_ms + cfg.transition_min_ms.max(1) as i64,
            revision: 0,
            season: season_from_epoch_ms(now_ms),
        }
    }

    /// Effective sight radius factor (docs §15 open; here a real server rule).
    /// Fog/Storm reduce what a player sees; the AOFB broadcast radius is scaled
    /// by this in world.rs. Never above 1.0 (weather never EXTENDS sight).
    pub fn sight_factor(&self) -> f64 {
        let base = match self.current {
            WeatherState::Sunny => 1.0,
            WeatherState::Cloudy => 0.9,
            WeatherState::Rain => 0.85,
            WeatherState::Storm => 0.7,
            WeatherState::Snow => 0.6,
            WeatherState::Fog => 0.55,
        };
        // Intensität modulae the reduction slightly (higher intensity -> less).
        let damp = (self.intensity as f64 / 100.0) * 0.15;
        (base - damp).clamp(0.2, 1.0)
    }

    /// Creature aggro/detection factor (docs §15; real rule via npc.rs).
    pub fn aggro_factor(&self) -> f64 {
        let base = match self.current {
            WeatherState::Sunny => 1.0,
            WeatherState::Cloudy => 0.95,
            WeatherState::Rain => 0.9,
            WeatherState::Storm => 0.75,
            WeatherState::Snow => 0.7,
            WeatherState::Fog => 0.6,
        };
        let damp = (self.intensity as f64 / 100.0) * 0.1;
        (base - damp).clamp(0.2, 1.0)
    }

    /// Respawn/spawn-condition factor (docs §15; real rule via npc.rs).
    /// >= 1.0 means weather can only DELAY respawns, never instant-spawn.
    pub fn respawn_factor(&self) -> f64 {
        match self.current {
            WeatherState::Sunny => 1.0,
            WeatherState::Cloudy => 1.0,
            WeatherState::Rain => 1.0,
            WeatherState::Fog => 1.1,
            WeatherState::Snow => 1.3,
            WeatherState::Storm => 1.5,
        }
    }

    /// Gathering/pickup reach factor (docs §15; real rule via loot pickup).
    pub fn pickup_factor(&self) -> f64 {
        let base = match self.current {
            WeatherState::Sunny => 1.0,
            WeatherState::Cloudy => 0.9,
            WeatherState::Rain => 0.8,
            WeatherState::Fog => 0.7,
            WeatherState::Storm => 0.7,
            WeatherState::Snow => 0.6,
        };
        let damp = (self.intensity as f64 / 100.0) * 0.1;
        (base - damp).clamp(0.2, 1.0)
    }

    /// S2C WEATHER payload (docs §13: client receives the confirmed state on
    /// join and on change). Carries only the realm-decided state, never secrets.
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "weather": self.current.key(),
            "intensity": self.intensity,
            "sight_factor": self.sight_factor(),
            "season": self.season.key(),
            "revision": self.revision,
        })
    }

    /// S2C WEATHER-Frame (seq 0 = Push, like INVENTORY/VITALS pushes).
    pub fn frame(&self) -> Frame {
        Frame::new(0, s2c::WEATHER, self.json())
    }

    /// Realm-decided world-context string for the Coordinator (docs §16/§17:
    /// NPCs may READ weather, never change it). Fills jobContext.world.
    pub fn coordinator_context(&self) -> String {
        let label = match self.current {
            WeatherState::Sunny => "sunny and clear",
            WeatherState::Cloudy => "overcast / cloudy",
            WeatherState::Rain => "raining",
            WeatherState::Fog => "thick fog, low visibility",
            WeatherState::Storm => "a thunderstorm with lightning",
            WeatherState::Snow => "snowing",
        };
        let season = match self.season {
            Season::Spring => "spring",
            Season::Summer => "summer",
            Season::Autumn => "autumn",
            Season::Winter => "winter",
        };
        format!(
            "Current season: {0}. Current weather: {1} (intensity {2}%).",
            season,
            label,
            self.intensity,
        )
    }
}

/// Season-dependent weather weights (V1.1 §4). Indexed by `WeatherState::all()`
/// order: [Sunny, Rain, Fog, Storm, Snow, Cloudy]. Values are RELATIVE weights
/// (not probabilities); a weight of 0 forbids that state in that season.
/// INVARIANT (tested): every season has at least TWO non-zero weights, so NO
/// season ever allows only a single weather state. Values are provisional and
/// adapted by later tests (docs §6/§14 Server-first); no final balancing.
pub fn season_weights(s: Season) -> [u32; 6] {
    match s {
        Season::Spring => [40, 25, 10, 15, 5, 25],
        Season::Summer => [55, 20, 10, 30, 0, 20], // snow forbidden in summer
        Season::Autumn => [20, 35, 25, 20, 5, 35],
        Season::Winter => [10, 10, 25, 10, 45, 30], // snow favoured in winter
    }
}

/// Advance the autoritative weather by one tick. Pure over (world, rng, now):
/// when the current state's duration elapsed, transition to `next`, roll a new
/// `next` + duration + intensity via the SEASON-WEIGHTED distribution, and bump
/// the revision. Returns true iff a genuine change happened (the caller
/// broadcasts WEATHER only then). The season is re-derived from `now_ms`.
pub fn advance(world: &mut World, cfg: &WeatherCfg, rng: &mut SplitMix64, now_ms: i64) -> bool {
    if now_ms < world.weather.next_at_ms {
        return false;
    }
    let current = world.weather.current;
    let season = season_from_epoch_ms(now_ms);
    let new_next = pick_season_weighted(current, season, rng);

    let span = cfg.transition_max_ms.max(cfg.transition_min_ms);
    let lo = cfg.transition_min_ms as i64;
    let dur = lo + (rng.next() * (span - cfg.transition_min_ms) as f64).floor() as i64;
    let intensity = (rng.next() * 101.0).floor() as u8;

    // Mutate the autoritative state in place (WeatherState is Copy; no move-out
    // of the struct). Order keeps the started_at/next_at relation consistent.
    world.weather.current = world.weather.next;
    world.weather.started_at_ms = world.weather.next_at_ms;
    world.weather.next = new_next;
    world.weather.intensity = intensity;
    world.weather.next_at_ms = world.weather.started_at_ms + dur;
    world.weather.revision = world.weather.revision.wrapping_add(1);
    world.weather.season = season;
    true
}

/// Shared successor-pick used by BOTH the production `advance` and the dev-side
/// `simulate`: season-weighted, excluding the current state (no no-op
/// self-transition) and any state forbidden this season (weight 0). Fail-closed
/// fallback to a uniform pick if the season leaves no candidate.
pub fn pick_season_weighted(current: WeatherState, season: Season, rng: &mut SplitMix64) -> WeatherState {
    let states = WeatherState::all();
    let weights = season_weights(season);
    let mut pool: Vec<(WeatherState, u32)> = Vec::with_capacity(6);
    let mut total = 0_u32;
    for (i, st) in states.iter().enumerate() {
        let w = weights[i];
        if w > 0 && *st != current {
            pool.push((*st, w));
            total += w;
        }
    }
    if total == 0 {
        let others: Vec<WeatherState> = states.iter().filter(|s| **s != current).map(|s| *s).collect();
        let roll = rng.next();
        let idx = (roll * others.len() as f64).floor() as usize;
        return others[idx.min(others.len() - 1)];
    }
    let roll = rng.next(); // [0,1)
    let target = (roll * total as f64).floor() as u32;
    let mut acc = 0_u32;
    for (st, w) in &pool {
        acc += w;
        if target < acc {
            return *st;
        }
    }
    pool[pool.len() - 1].0
}

/// Development-side simulation of weather + season WITHOUT touching production
/// state or the system clock (V1.1 §8). Pure and deterministic: it never reads
/// `SystemTime` and never mutates a `World`. Given a seed, a start epoch-ms and
/// a number of steps, it returns the sequence of (weather, season) the Realm
/// WOULD pass through — for tests, tooling and design exploration only.
pub fn simulate(seed: u64, cfg: &WeatherCfg, start_ms: i64, steps: u32) -> Vec<(WeatherState, Season)> {
    let mut cur = WeatherState::Sunny;
    let mut started = start_ms;
    let mut clock = start_ms;
    let mut rng = SplitMix64::new(seed);
    let mut out: Vec<(WeatherState, Season)> = Vec::with_capacity(steps.saturating_add(1) as usize);
    out.push((cur, season_from_epoch_ms(clock)));
    for _ in 0..steps {
        let span = cfg.transition_max_ms.max(cfg.transition_min_ms);
        let lo = cfg.transition_min_ms as i64;
        let dur = lo + (rng.next() * (span - cfg.transition_min_ms) as f64).floor() as i64;
        clock = started + dur;
        let season = season_from_epoch_ms(clock);
        cur = pick_season_weighted(cur, season, &mut rng);
        started = clock;
        out.push((cur, season));
    }
    out
}

/// Persistenz-Row (realm-wide, single row). Mirrors the NPC-state pattern.
#[derive(Debug, Clone)]
pub struct WeatherRow {
    pub current: String,
    pub next: String,
    pub intensity: i32,
    pub started_at_ms: i64,
    pub next_at_ms: i64,
    pub revision: i64,
}

/// Rekonstruieren a persisted row into a Weather, fail-closed on unknown keys.
pub fn from_row(row: &WeatherRow, now_ms: i64, cfg: &WeatherCfg) -> Weather {
    let current = WeatherState::from_key(row.current.as_str()).unwrap_or(WeatherState::Sunny);
    let next = WeatherState::from_key(row.next.as_str()).unwrap_or(current);
    Weather {
        current,
        next,
        intensity: row.intensity.clamp(0, 100) as u8,
        started_at_ms: row.started_at_ms,
        // A stale next_at in the past is honoured (docs §12: the persisted
        // position is resumed, not reset); the next tick will advance it.
        next_at_ms: row.next_at_ms,
        revision: row.revision.max(0) as u64,
        // Season is calendar-derived (not persisted): recompute from the clock.
        season: season_from_epoch_ms(now_ms),
    }
}

/// Serialise the current weather to a persist row.
pub fn to_row(w: &Weather) -> WeatherRow {
    WeatherRow {
        current: w.current.key().to_string(),
        next: w.next.key().to_string(),
        intensity: w.intensity as i32,
        started_at_ms: w.started_at_ms,
        next_at_ms: w.next_at_ms,
        revision: w.revision as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    fn cfg() -> WeatherCfg {
        WeatherCfg {
            transition_min_ms: 1000,
            transition_max_ms: 4000,
            persist_interval_ms: 30000,
        }
    }

    #[test]
    fn state_keys_roundtrip() {
        for s in WeatherState::all() {
            assert_eq!(WeatherState::from_key(s.key()), Some(s));
        }
        // V1.1 added snow + cloudy; unknown keys still fail closed.
        assert_eq!(WeatherState::from_key("hail"), None);
        assert_eq!(WeatherState::from_key("  SNOW "), Some(WeatherState::Snow));
        assert_eq!(WeatherState::from_key("cloudy"), Some(WeatherState::Cloudy));
    }

    #[test]
    fn factors_are_state_bounded() {
        let mk = |st: WeatherState, i: u8| {
            Weather {
                current: st,
                next: st,
                intensity: i,
                started_at_ms: 0,
                next_at_ms: 0,
                revision: 0,
                season: Season::Spring,
            }
        };
        // Sight never above 1.0 and never below the floor.
        for st in WeatherState::all() {
            let w = mk(st, 0);
            assert!(w.sight_factor() <= 1.0 + 1e-9);
            assert!(w.aggro_factor() <= 1.0 + 1e-9);
            assert!(w.pickup_factor() <= 1.0 + 1e-9);
            assert!(w.respawn_factor() >= 1.0 - 1e-9);
        }
        // Fog reduces sight more than sunny.
        assert!(mk(WeatherState::Fog, 50).sight_factor() < mk(WeatherState::Sunny, 0).sight_factor());
        // Storm delays respawn most.
        assert!(mk(WeatherState::Storm, 0).respawn_factor() > mk(WeatherState::Sunny, 0).respawn_factor());
    }

    #[test]
    fn advance_only_at_boundary_and_bumps_revision() {
        let mut w = World::new();
        w.weather = Weather::initial(0, &cfg());
        let mut rng = SplitMix64::new(12345);
        // Before the boundary: no change, revision stays.
        assert_eq!(advance(&mut w, &cfg(), &mut rng, 500), false);
        assert_eq!(w.weather.revision, 0);
        // At/after the boundary: exactly one change, revision +1.
        assert_eq!(advance(&mut w, &cfg(), &mut rng, 1000), true);
        assert_eq!(w.weather.revision, 1);
        // Immediately again (now before the new next_at): no double change.
        assert_eq!(advance(&mut w, &cfg(), &mut rng, 1001), false);
    }

    #[test]
    fn no_self_transition_on_change() {
        let mut w = World::new();
        w.weather = Weather::initial(0, &cfg());
        let mut rng = SplitMix64::new(7);
        let before = w.weather.current;
        assert_eq!(advance(&mut w, &cfg(), &mut rng, 1000), true);
        // current becomes the previous `next` (Sunny), which differs from the
        // freshly rolled `next` (never equals the new current).
        assert_ne!(w.weather.current, w.weather.next);
        assert_eq!(before, WeatherState::Sunny);
    }

    #[test]
    fn persisted_row_survives_restart_not_reset() {
        // docs §12: a restart resumes the persisted position, not Tag1/12:00/CLEAR.
        let row = WeatherRow {
            current: "storm".into(),
            next: "fog".into(),
            intensity: 80,
            started_at_ms: 50_000,
            next_at_ms: 60_000,
            revision: 9,
        };
        let w = from_row(&row, 55_000, &cfg());
        assert_eq!(w.current, WeatherState::Storm);
        assert_eq!(w.revision, 9);
        assert_eq!(w.intensity, 80);
        // Unknown persisted key fails closed to a safe state, never panics.
        let bad = WeatherRow {
            current: "blizzard".into(),
            next: "??".into(),
            intensity: 999,
            started_at_ms: 1,
            next_at_ms: 2,
            revision: -3,
        };
        let w2 = from_row(&bad, 10, &cfg());
        assert_eq!(w2.current, WeatherState::Sunny);
        assert_eq!(w2.intensity, 100);
        assert_eq!(w2.revision, 0);
    }

    #[test]
    fn to_from_row_is_stable() {
        let w = Weather::initial(1000, &cfg());
        let back = from_row(&to_row(&w), 1000, &cfg());
        assert_eq!(back.current, w.current);
        assert_eq!(back.next, w.next);
        assert_eq!(back.intensity, w.intensity);
        assert_eq!(back.revision, w.revision);
    }

    #[test]
    fn s2c_frame_carries_only_realm_state() {
        let w = Weather {
            current: WeatherState::Rain,
            next: WeatherState::Fog,
            intensity: 60,
            started_at_ms: 0,
            next_at_ms: 0,
            revision: 3,
            season: Season::Autumn,
        };
        let f = w.frame();
        assert_eq!(f.msg_type, s2c::WEATHER);
        assert_eq!(f.data["weather"].as_str().unwrap(), "rain");
        assert_eq!(f.data["revision"].as_i64().unwrap(), 3);
        // No secrets/positions leak into the weather frame.
        let s = f.encode();
        assert!(!s.contains("session") && !s.contains("handoff"));
    }

    #[test]
    fn coordinator_context_names_weather_for_npc() {
        let w = Weather {
            current: WeatherState::Storm,
            next: WeatherState::Storm,
            intensity: 90,
            started_at_ms: 0,
            next_at_ms: 0,
            revision: 1,
            season: Season::Summer,
        };
        let ctx = w.coordinator_context();
        assert!(ctx.contains("thunderstorm"));
        assert!(ctx.contains("90%"));
    }

    // ── V1.1 SEASONS ──────────────────────────────────────────────────────

    #[test]
    fn season_keys_roundtrip() {
        for s in [Season::Spring, Season::Summer, Season::Autumn, Season::Winter] {
            assert_eq!(Season::from_key(s.key()), Some(s));
        }
        assert_eq!(Season::from_key("monsoon"), None);
    }

    #[test]
    fn month_to_season_boundaries() {
        // Frühling März–Mai, Sommer Juni–August, Herbst Sep–Nov, Winter Dez–Feb.
        assert_eq!(Season::from_month(3), Season::Spring);
        assert_eq!(Season::from_month(5), Season::Spring);
        assert_eq!(Season::from_month(6), Season::Summer);
        assert_eq!(Season::from_month(8), Season::Summer);
        assert_eq!(Season::from_month(9), Season::Autumn);
        assert_eq!(Season::from_month(11), Season::Autumn);
        assert_eq!(Season::from_month(12), Season::Winter);
        assert_eq!(Season::from_month(1), Season::Winter);
        assert_eq!(Season::from_month(2), Season::Winter);
        // Out-of-range fails closed to Winter.
        assert_eq!(Season::from_month(0), Season::Winter);
        assert_eq!(Season::from_month(13), Season::Winter);
    }

    #[test]
    fn utc_month_matches_calendar_including_leap_year() {
        // Known epoch-ms (UTC midnight) for boundary dates.
        // 2024-02-29 (leap day exists) -> month 2 (Winter).
        // 2024 is a leap year; 2023-02-29 does not exist (we test 2023-03-01).
        let ms_2024_feb29 = epoch_ms(2024, 2, 29);
        let ms_2024_mar01 = epoch_ms(2024, 3, 1);
        let ms_2023_mar01 = epoch_ms(2023, 3, 1);
        let ms_2024_dec31 = epoch_ms(2024, 12, 31);
        assert_eq!(utc_month_from_epoch_ms(ms_2024_feb29), 2);
        assert_eq!(season_from_epoch_ms(ms_2024_feb29), Season::Winter);
        assert_eq!(utc_month_from_epoch_ms(ms_2024_mar01), 3);
        assert_eq!(season_from_epoch_ms(ms_2024_mar01), Season::Spring);
        assert_eq!(season_from_epoch_ms(ms_2023_mar01), Season::Spring);
        assert_eq!(season_from_epoch_ms(ms_2024_dec31), Season::Winter);
    }

    #[test]
    fn no_season_allows_only_one_state() {
        // V1.1 §4 invariant: every season has >= 2 non-zero weights.
        for s in [Season::Spring, Season::Summer, Season::Autumn, Season::Winter] {
            let nz = season_weights(s).iter().filter(|w| **w > 0).count();
            assert!(nz >= 2, "season {0} must allow at least two states, got {1}", s.key(), nz);
        }
        // Summer forbids snow but allows the rest; Winter favours snow.
        assert_eq!(season_weights(Season::Summer)[4], 0); // snow index 4
        assert!(season_weights(Season::Winter)[4] > 0);
    }

    #[test]
    fn snow_and_cloudy_have_factors() {
        let mk = |st: WeatherState, i: u8| {
            Weather {
                current: st,
                next: st,
                intensity: i,
                started_at_ms: 0,
                next_at_ms: 0,
                revision: 0,
                season: Season::Winter,
            }
        };
        // Snow reduces sight/pickup/aggro and delays respawn; Cloudy is mild.
        assert!(mk(WeatherState::Snow, 0).sight_factor() < mk(WeatherState::Sunny, 0).sight_factor());
        assert!(mk(WeatherState::Cloudy, 0).sight_factor() < mk(WeatherState::Sunny, 0).sight_factor());
        assert!(mk(WeatherState::Snow, 0).respawn_factor() > mk(WeatherState::Cloudy, 0).respawn_factor());
        assert!(mk(WeatherState::Cloudy, 0).respawn_factor() == 1.0);
    }

    #[test]
    fn simulate_is_deterministic_and_clock_free() {
        // V1.1 §8: dev simulation never reads the system clock; same inputs ->
        // same outputs. It also never touches a World.
        let a = simulate(42, &cfg(), epoch_ms(2024, 1, 1), 12);
        let b = simulate(42, &cfg(), epoch_ms(2024, 1, 1), 12);
        assert_eq!(a, b);
        assert_eq!(a.len(), 13);
        // Seasons progress across the simulated year (Jan winter -> ... ).
        assert_eq!(a[0].1, Season::Winter); // start 2024-01-01
        // No simulated step ever yields a state forbidden that season (e.g. no
        // snow in summer months).
        for (st, season) in &a {
            if *season == Season::Summer {
                assert_ne!(*st, WeatherState::Snow);
            }
        }
    }

    #[test]
    fn weather_frame_carries_season() {
        let w = Weather::initial(epoch_ms(2024, 7, 15), &cfg());
        let f = w.frame();
        assert_eq!(f.data["season"].as_str().unwrap(), "summer");
        assert_eq!(f.data["weather"].as_str().unwrap(), "sunny");
    }

    /// Epoch-ms for a UTC date (y, m, d) at 00:00. Inverse of the civil-date
    /// algorithm, for test fixtures only.
    fn epoch_ms(y: i64, m: i64, d: i64) -> i64 {
        // Days since 1970-01-01 via the civil_from_days inverse (Hinnant).
        let y0 = if m <= 2 { y - 1 } else { y };
        let era = (if y0 >= 0 { y0 } else { y0 - 399 }) / 400;
        let yoe = y0 - era * 400; // [0,399]
        let mp = if m > 2 { m - 3 } else { m + 9 }; // [0,11]
        let doy = (153 * mp + 2) / 5 + d - 1; // [0,365]
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0,146096]
        let days = era * 146_097 + doe - 719_468;
        days * 86_400_000
    }
}
