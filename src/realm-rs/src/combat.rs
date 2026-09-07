// combat — Combat V1 (vertikaler Schnitt), Realm-autoritativ.
// Umgesetzt nach docs/Kampfsystem.md (§§1–7, 17): klassisches Targeting,
// manuell gestarteter/beendeter Auto-Grundangriff, Waffen-Duration,
// physische Trefferauflösung (Miss/Dodge/Parry/Block/Normal/Crit),
// Waffenschaden, Rüstungsreduktion mit Klassen-Caps.
//
// Alle Zahlenwerte sind VORLÄUFIGE Balancingwerte aus config.env
// (CombatCfg) und werden anhand späterer Praxistests angepasst.
// Die Trefferentscheidung ist eine reine Funktion mit injizierbarem,
// reproduzierbarem RNG — damit in Tests deterministisch testbar.
use std::time::{Duration, Instant};

use crate::config::CombatCfg;
use crate::protocol::{s2c, Frame};
use crate::world::World;

/// Ergebnis einer physischen Trefferauflösung (docs/Kampfsystem.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitResult {
    Miss,
    Dodge,
    Parry,
    Block,
    Normal,
    Critical,
}

impl HitResult {
    /// Kompakter Schlüssel fürs Drahtformat (s2c::DAMAGE "hit"-Feld).
    pub fn key(&self) -> &'static str {
        match self {
            HitResult::Miss => "miss",
            HitResult::Dodge => "dodge",
            HitResult::Parry => "parry",
            HitResult::Block => "block",
            HitResult::Normal => "normal",
            HitResult::Critical => "crit",
        }
    }
}

/// Injektierbarer Zufallsgenerator (Traits erlauben Scripted/Seed-RNG).
pub trait CombatRng {
    /// Gleichverteilt in [0,1).
    fn next(&mut self) -> f64;
}

/// Deterministischer SplitMix64 (reproduzierbar; Produktion pro Start geseedet).
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Seed aus Systemzeit (kein fester Start, damit Kämpfe nicht über
    /// Neustarts hinweg vorhersagbar identisch sind).
    pub fn time_seed() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
    }
}

impl CombatRng for SplitMix64 {
    fn next(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Rüstungs-Cap je Klasse (docs/Kampfsystem.md §7): Tank 50 %, Magier 20 %,
/// Sonstige (vorläufig) 30 %. Klassennamen aus dem DB-Attribut char_class;
/// legacy-Werte (Warrior/Mage) und deutsche Klassennamen werden erkannt.
fn class_cap(cfg: &CombatCfg, class: &str) -> u32 {
    let c = class.trim().to_lowercase();
    const TANK: &[&str] = &["krieger", "paladin", "kämpfer", "kampfer", "warrior"];
    const MAGE: &[&str] = &["magier", "hexer", "mentalist", "mage"];
    if TANK.contains(&c.as_str()) {
        cfg.armor_cap_tank
    } else if MAGE.contains(&c.as_str()) {
        cfg.armor_cap_mage
    } else {
        cfg.armor_cap_default
    }
}

/// Prozentuale physische Schadensreduktion aus Rüstung (vorläufig linear:
/// `armor_pct_per_point` Prozent je Rüstungspunkt, gedeckelt auf das
/// Klassen-Cap). Reine Funktion, testbar.
pub fn armor_reduction_pct(cfg: &CombatCfg, armor: i32, class_cap: u32) -> u32 {
    if armor <= 0 {
        return 0;
    }
    let raw = (armor as u32).saturating_mul(cfg.armor_pct_per_point);
    raw.min(class_cap)
}

/// Physische Trefferauflösung (docs/Kampfsystem.md §5): reine Funktion mit
/// injizierbarem RNG. Reihenfolge: Miss → Dodge → Parry → Block → Normal/Crit.
/// Der Waffenskill reduziert die Miss-Chance (wesentlicher Bestandteil der
/// Trefferwahrscheinlichkeit, §4/§5). Block reduziert Schaden (kein volles
/// Ausweichen, §5). Rüstung reduziert den Schaden mit Klassen-Cap (§7).
pub fn resolve_attack(
    cfg: &CombatCfg,
    rng: &mut dyn CombatRng,
    weapon_damage: i32,
    weapon_skill: u32,
    armor: i32,
    cap: u32,
) -> (HitResult, i32) {
    let base = weapon_damage.max(0);
    let mut miss = cfg.hit_miss_permille;
    // Skill-Punkte über 1 verringern die Miss-Chance pro Punkt (vorläufig).
    let skill_bonus = weapon_skill
        .saturating_sub(1)
        .saturating_mul(cfg.skill_hit_bonus_permille);
    miss = miss.saturating_sub(skill_bonus);

    let dodge = cfg.hit_dodge_permille;
    let parry = cfg.hit_parry_permille;
    let block = cfg.hit_block_permille;
    let crit = cfg.hit_crit_permille;

    let roll = rng.next() * 1000.0;
    let result = if roll < miss as f64 {
        HitResult::Miss
    } else if roll < (miss + dodge) as f64 {
        HitResult::Dodge
    } else if roll < (miss + dodge + parry) as f64 {
        HitResult::Parry
    } else if roll < (miss + dodge + parry + block) as f64 {
        HitResult::Block
    } else if rng.next() * 1000.0 < crit as f64 {
        HitResult::Critical
    } else {
        HitResult::Normal
    };

    let dmg = match result {
        HitResult::Miss | HitResult::Dodge | HitResult::Parry => 0,
        HitResult::Block => {
            // Block reduziert Schaden, verhindert ihn nicht vollständig.
            let reduce = cfg.hit_block_reduce_percent.min(100) as i32;
            base * (100 - reduce) / 100
        }
        HitResult::Normal => base,
        HitResult::Critical => base * cfg.hit_crit_mult_percent.min(1000) as i32 / 100,
    };

    let reduction = armor_reduction_pct(cfg, armor, cap);
    let dmg = dmg * (100 - reduction as i32) / 100;
    (result, dmg.max(0))
}

/// Autoritativer Kampfzustand eines Spielers (einer pro Spieler in V1).
#[derive(Debug, Clone)]
pub struct CombatState {
    pub target_id: String,
    pub last_attack: Instant,
}

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    (ax - bx).hypot(ay - by)
}

/// Welt-Tick des Kampfs (nach world_tick): verarbeitet alle bewaffneten
/// Auto-Angriffe. Regeln (docs/Kampfsystem.md §§3, 7, 9, 13):
/// - Angriff läuft, solange ein gültiges, lebendes, erreichbares Ziel
///   besteht und die Waffen-Duration verstrichen ist.
/// - Außer Reichweite → Tick übersprungen, Angriff bleibt aktiv (pausiert).
/// - Ziel tot/weg → Auto-Angriff endet (auch für alle, die dasselbe Ziel
///   anvisieren). Bei 0 HP: KILL-Broadcast, HP nie unter 0.
/// Sender: DAMAGE (+ hp via world_tick-STATE) und KILL an alle Sichtbaren.
pub fn combat_tick(
    world: &mut World,
    cfg: &CombatCfg,
    rng: &mut dyn CombatRng,
    now: Instant,
    aofb: f64,
) {
    // 1) Bewaffnete Angriffe als Snapshot (vermeidet Borrow-Konflikte).
    let armed: Vec<(String, String, u32)> = world
        .players
        .values()
        .filter_map(|p| {
            p.combat
                .as_ref()
                .map(|c| (p.id.clone(), c.target_id.clone(), p.weapon_skill))
        })
        .collect();

    // tote Angreifer entwaffnen (tote greifen nicht an).
    for (aid, _, _) in &armed {
        if let Some(a) = world.players.get(aid) {
            if a.hp <= 0 {
                if let Some(a) = world.players.get_mut(aid) {
                    a.combat = None;
                }
            }
        }
    }

    // 2) Je Tick auswertbar? (noch lebendiges, erreichbares Ziel, Duration
    //    abgelaufen). Ergebnis wird je Angreifer aufgezeichnet.
    let mut outcomes: Vec<(String, String, f64, f64, HitResult, i32)> = Vec::new();
    for (aid, tid, skill) in &armed {
        let Some(a) = world.players.get(aid) else {
            continue;
        };
        let Some(c) = a.combat.as_ref() else {
            continue;
        };
        let Some(elapsed) = now.checked_duration_since(c.last_attack) else {
            continue;
        };
        if elapsed < Duration::from_millis(cfg.weapon_duration_ms) {
            continue;
        }
        let Some(t) = world.players.get(tid) else {
            // Ziel weg → Auto-Angriff beendet.
            if let Some(a) = world.players.get_mut(aid) {
                a.combat = None;
            }
            continue;
        };
        if t.hp <= 0 {
            // Ziel tot → Auto-Angriff beendet.
            if let Some(a) = world.players.get_mut(aid) {
                a.combat = None;
            }
            continue;
        }
        if dist(a.x, a.y, t.x, t.y) > cfg.weapon_range {
            continue; // außer Reichweite: pausieren, Angriff bleibt aktiv.
        }
        let cap = class_cap(cfg, &t.char_class);
        let (result, dmg) = resolve_attack(cfg, rng, cfg.weapon_damage, *skill, t.armor, cap);
        outcomes.push((aid.clone(), tid.clone(), a.x, a.y, result, dmg));
    }

    // 3) Schaden anwenden (eigene Schleife, keine konkurrierenden Borrows).
    //    Broadcasts werden erst nach den Mutations gesammelt und versendet.
    let mut kills: Vec<(String, String)> = Vec::new();
    let mut broadcasts: Vec<(String, f64, f64, Frame)> = Vec::new();
    for (aid, tid, ax, ay, result, dmg) in &outcomes {
        if let Some(a) = world.players.get_mut(aid) {
            if let Some(c) = a.combat.as_mut() {
                c.last_attack = now;
            }
        }
        let killed = if let Some(t) = world.players.get_mut(tid) {
            t.hp = (t.hp - dmg).max(0);
            t.hp == 0
        } else {
            false
        };
        let hit = result.key();
        broadcasts.push((
            aid.clone(),
            *ax,
            *ay,
            Frame::new(
                0,
                s2c::DAMAGE,
                serde_json::json!({"id": tid, "amount": dmg, "from_id": aid, "hit": hit}),
            ),
        ));
        if killed {
            kills.push((aid.clone(), tid.clone()));
        }
    }
    for (aid, ax, ay, frame) in &broadcasts {
        for p in world.players.values() {
            if p.id == *aid || dist(p.x, p.y, *ax, *ay) <= aofb {
                p.send(frame);
            }
        }
    }

    // 4) Tode: KILL broadcasten; Angreifer, deren Ziel tot ist, entwaffnen &
    //    tote Spieler selbst entwaffnen.
    for (aid, tid) in &kills {
        let frame = Frame::new(
            0,
            s2c::KILL,
            serde_json::json!({"id": tid, "killer_id": aid}),
        );
        for p in world.players.values() {
            p.send(&frame);
        }
        // Blackboard: jeder (inkl. des Killers), der dieses Ziel anvisiert,
        // beendet seinen Auto-Angriff; der Tote greift nicht weiter an.
        for p in world.players.values_mut() {
            let targets_dead = p
                .combat
                .as_ref()
                .map(|c| c.target_id == *tid)
                .unwrap_or(false);
            if targets_dead || p.id == *tid {
                p.combat = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use tokio::sync::mpsc;

    /// Scripted-RNG: liefert exakt die vorgegebenen Werte (deterministisch).
    struct ScriptedRng(VecDeque<f64>);
    impl ScriptedRng {
        fn from(v: &[f64]) -> Self {
            Self(v.iter().copied().collect())
        }
    }
    impl CombatRng for ScriptedRng {
        fn next(&mut self) -> f64 {
            self.0.pop_front().expect("scripted rng exhausted")
        }
    }

    fn test_cfg() -> CombatCfg {
        CombatCfg {
            weapon_skill_id: "schwerter".into(),
            weapon_damage: 100,
            weapon_duration_ms: 2000,
            weapon_range: 2.0,
            hit_miss_permille: 100,
            hit_dodge_permille: 100,
            hit_parry_permille: 50,
            hit_block_permille: 100,
            hit_crit_permille: 100,
            hit_crit_mult_percent: 150,
            hit_block_reduce_percent: 50,
            armor_pct_per_point: 2,
            armor_cap_tank: 50,
            armor_cap_mage: 20,
            armor_cap_default: 30,
            skill_hit_bonus_permille: 5,
        }
    }

    fn player(
        id: &str,
        hp: i32,
        class: &str,
    ) -> (crate::world::Player, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            crate::world::Player {
                id: id.into(),
                name: id.into(),
                x: 0.0,
                y: 0.0,
                face: 0.0,
                ping_ms: 0,
                zone_id: 0,
                hp,
                max_hp: hp,
                lang: "de".into(),
                account_id: 0,
                session_id: String::new(),
                entities: Default::default(),
                last_activity: Instant::now(),
                tx,
                char_class: class.into(),
                level: 1,
                armor: 0,
                weapon_skill: 1,
                combat: None,
            },
            rx,
        )
    }

    #[test]
    fn splitmix64_is_deterministic_and_in_unit_range() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        let seq_a: Vec<f64> = (0..5).map(|_| a.next()).collect();
        let seq_b: Vec<f64> = (0..5).map(|_| b.next()).collect();
        assert_eq!(seq_a, seq_b);
        for v in &seq_a {
            assert!((0.0..1.0).contains(v));
        }
    }

    #[test]
    fn miss_dodge_parry_deal_no_damage() {
        let cfg = test_cfg();
        // 0.00 → miss (0..100)
        let (r, d) = resolve_attack(&cfg, &mut ScriptedRng::from(&[0.0]), 100, 1, 0, 30);
        assert_eq!(r, HitResult::Miss);
        assert_eq!(d, 0);
        // 0.15 → dodge (100..200)
        let (r, d) = resolve_attack(&cfg, &mut ScriptedRng::from(&[0.15]), 100, 1, 0, 30);
        assert_eq!(r, HitResult::Dodge);
        assert_eq!(d, 0);
        // 0.24 → parry (200..250)
        let (r, d) = resolve_attack(&cfg, &mut ScriptedRng::from(&[0.24]), 100, 1, 0, 30);
        assert_eq!(r, HitResult::Parry);
        assert_eq!(d, 0);
    }

    #[test]
    fn block_reduces_damage_no_full_evade() {
        let cfg = test_cfg();
        // 0.30 → block (250..350), Basis 100, Reduktion 50 %
        let (r, d) = resolve_attack(&cfg, &mut ScriptedRng::from(&[0.30]), 100, 1, 0, 30);
        assert_eq!(r, HitResult::Block);
        assert_eq!(d, 50);
    }

    #[test]
    fn normal_hit_full_damage_without_armor() {
        let cfg = test_cfg();
        // 0.80 trifft (nach 0..350), Krit-Wurf 0.90 → Normal.
        let (r, d) = resolve_attack(&cfg, &mut ScriptedRng::from(&[0.80, 0.90]), 100, 1, 0, 30);
        assert_eq!(r, HitResult::Normal);
        assert_eq!(d, 100);
    }

    #[test]
    fn critical_hit_multiplies() {
        let cfg = test_cfg();
        let (r, d) = resolve_attack(&cfg, &mut ScriptedRng::from(&[0.80, 0.05]), 100, 1, 0, 30);
        assert_eq!(r, HitResult::Critical);
        assert_eq!(d, 150);
    }

    #[test]
    fn weapon_skill_reduces_miss_chance() {
        let mut cfg = test_cfg();
        cfg.hit_dodge_permille = 0;
        cfg.hit_parry_permille = 0;
        cfg.hit_block_permille = 0;
        // 0.097 (97‰): bei Skill 1 (Miss 100) → verfehlt.
        let (r, _) = resolve_attack(&cfg, &mut ScriptedRng::from(&[0.097]), 100, 1, 0, 30);
        assert_eq!(r, HitResult::Miss);
        // Skill 2 → Miss 95, 0.097 trifft (Krit-Wurf 0.9 → Normal).
        let (r, _) = resolve_attack(&cfg, &mut ScriptedRng::from(&[0.097, 0.9]), 100, 2, 0, 30);
        assert_eq!(r, HitResult::Normal);
    }

    #[test]
    fn armor_reduction_respects_class_cap() {
        let cfg = test_cfg();
        // Tank: 15 Rüstung → 30 % Reduktion (2 %/Pkt), ca. 70.
        let (_, d) = resolve_attack(&cfg, &mut ScriptedRng::from(&[0.8, 0.9]), 100, 1, 15, 50);
        assert_eq!(d, 70);
        // Tank: viel Rüstung → Cap 50 %.
        let (_, d) = resolve_attack(&cfg, &mut ScriptedRng::from(&[0.8, 0.9]), 100, 1, 999, 50);
        assert_eq!(d, 50);
        // Magier: 999 Rüstung → Cap 20 %.
        let (_, d) = resolve_attack(&cfg, &mut ScriptedRng::from(&[0.8, 0.9]), 100, 1, 999, 20);
        assert_eq!(d, 80);
    }

    fn armed_player(player: &mut crate::world::Player, target: &str, now: Instant) {
        player.combat = Some(CombatState {
            target_id: target.into(),
            last_attack: now - Duration::from_millis(2000),
        });
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<String>) -> Vec<String> {
        let mut v = Vec::new();
        while let Ok(m) = rx.try_recv() {
            v.push(m);
        }
        v
    }

    #[test]
    fn auto_attack_duration_gates_follow_ups() {
        let cfg = test_cfg();
        let mut w = World::new();
        let (mut a, _) = player("a", 100, "Warrior");
        a.x = 0.0;
        let (mut b, mut rb) = player("b", 1000, "Mage"); // überlebt mehrere Treffer
        b.x = 1.0;
        let now = Instant::now();
        armed_player(&mut a, "b", now);
        w.players.insert("a".into(), a);
        w.players.insert("b".into(), b);

        // Erster Angriff sofort (Duration als abgelaufen gesetzt).
        let t1 = now + Duration::from_millis(1);
        combat_tick(&mut w, &cfg, &mut ScriptedRng::from(&[0.8, 0.9]), t1, 20.0);
        let msgs = drain(&mut rb);
        assert_eq!(msgs.len(), 1, "DAMAGE-Broadcast erwartet");
        assert!(msgs[0].contains("\"type\":5") && msgs[0].contains("\"amount\":100"));
        assert_eq!(w.players["b"].hp, 900);

        // Noch innerhalb der Duration → kein Folgeangriff.
        let t2 = t1 + Duration::from_millis(1000);
        combat_tick(&mut w, &cfg, &mut ScriptedRng::from(&[0.8, 0.9]), t2, 20.0);
        assert!(
            drain(&mut rb).is_empty(),
            "kein Angriff vor Duration-Ablauf"
        );
        assert!(w.players["a"].combat.is_some(), "Auto-Angriff bleibt aktiv");

        // Nach der Duration → Folgeangriff.
        let t3 = t1 + Duration::from_millis(2000);
        combat_tick(&mut w, &cfg, &mut ScriptedRng::from(&[0.8, 0.9]), t3, 20.0);
        assert_eq!(drain(&mut rb).len(), 1);
    }

    #[test]
    fn out_of_range_pauses_but_keeps_state() {
        let cfg = test_cfg();
        let mut w = World::new();
        let (mut a, _) = player("a", 100, "Warrior");
        a.x = 0.0;
        let (mut b, mut rb) = player("b", 100, "Mage");
        b.x = 100.0; // weit außerhalb der Waffen-Reichweite (2 m).
        let now = Instant::now();
        armed_player(&mut a, "b", now);
        w.players.insert("a".into(), a);
        w.players.insert("b".into(), b);

        combat_tick(
            &mut w,
            &cfg,
            &mut ScriptedRng::from(&[0.8, 0.9]),
            now + Duration::from_millis(10),
            20.0,
        );
        assert!(drain(&mut rb).is_empty());
        assert!(
            w.players["a"].combat.is_some(),
            "außer Reichweite bleibt bewaffnet"
        );
    }

    #[test]
    fn invalid_or_dead_target_ends_auto_attack() {
        let cfg = test_cfg();
        // Ziel existiert nicht → entwaffnen.
        let mut w = World::new();
        let (mut a, _) = player("a", 100, "Warrior");
        let now = Instant::now();
        armed_player(&mut a, "ghost", now);
        w.players.insert("a".into(), a);
        combat_tick(
            &mut w,
            &cfg,
            &mut ScriptedRng::from(&[0.8, 0.9]),
            now + Duration::from_millis(10),
            20.0,
        );
        assert!(
            w.players["a"].combat.is_none(),
            "verschwundenes Ziel beendet Angriff"
        );

        // Ziel tot → entwaffnen.
        let mut w = World::new();
        let (mut a, _) = player("a", 100, "Warrior");
        let (b, _) = player("b", 0, "Mage");
        armed_player(&mut a, "b", now);
        w.players.insert("a".into(), a);
        w.players.insert("b".into(), b);
        combat_tick(
            &mut w,
            &cfg,
            &mut ScriptedRng::from(&[0.8, 0.9]),
            now + Duration::from_millis(10),
            20.0,
        );
        assert!(w.players["a"].combat.is_none());
    }

    #[test]
    fn death_clamps_hp_and_broadcasts_kill_and_disarms_all() {
        let cfg = test_cfg();
        let mut w = World::new();
        let (mut a, mut ra) = player("a", 100, "Warrior");
        let (c, mut rc) = player("c", 100, "Warrior"); // dritter Beobachter
        let (b, mut rb) = player("b", 40, "Mage"); // 40 HP < 100 Schaden
        let (mut d, _) = player("d", 100, "Mage"); // weiterer Angreifer auf b
        let now = Instant::now();
        armed_player(&mut a, "b", now);
        armed_player(&mut d, "b", now);
        w.players.insert("a".into(), a);
        w.players.insert("b".into(), b);
        w.players.insert("c".into(), c);
        w.players.insert("d".into(), d);

        // Beide Angreifer treffen b für je 100 → b stirbt beim ersten Treffer.
        combat_tick(
            &mut w,
            &cfg,
            &mut ScriptedRng::from(&[0.8, 0.9, 0.8, 0.9]),
            now + Duration::from_millis(10),
            20.0,
        );

        assert_eq!(w.players["b"].hp, 0, "HP nie unter 0");
        assert!(
            w.players["a"].combat.is_none(),
            "Ziel tot → Killer entwaffnet"
        );
        assert!(
            w.players["d"].combat.is_none(),
            "alle Angreifer auf b entwaffnet"
        );
        let rm = drain(&mut rb);
        assert!(
            rm.iter().any(|m| m.contains("\"type\":6")),
            "KILL an Ziel: {rm:?}"
        );
        assert!(rm
            .iter()
            .any(|m| m.contains("\"type\":5") && m.contains("\"amount\":100")));
        let rc_msgs = drain(&mut rc);
        assert!(
            rc_msgs.iter().any(|m| m.contains("\"type\":6")),
            "KILL an Beobachter"
        );
        let ra_msgs = drain(&mut ra);
        assert!(
            ra_msgs.iter().any(|m| m.contains("\"type\":6")),
            "KILL an Killer selbst"
        );
    }

    #[test]
    fn dead_attacker_does_not_attack() {
        let cfg = test_cfg();
        let mut w = World::new();
        let (mut a, _) = player("a", 0, "Warrior");
        let (b, mut rb) = player("b", 100, "Mage");
        let now = Instant::now();
        armed_player(&mut a, "b", now);
        w.players.insert("a".into(), a);
        w.players.insert("b".into(), b);
        combat_tick(
            &mut w,
            &cfg,
            &mut ScriptedRng::from(&[0.8, 0.9]),
            now + Duration::from_millis(10),
            20.0,
        );
        assert!(drain(&mut rb).is_empty());
        assert!(
            w.players["a"].combat.is_none(),
            "toter Angreifer entwaffnet"
        );
    }
}
