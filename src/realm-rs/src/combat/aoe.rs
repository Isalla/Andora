// aoe — Zielauswahl für Flächenfähigkeiten (Combat V3, Ability-System.md §4).
//
// Vier Grundtypen: single, target_radius, caster_radius, ground.
// Kein künstliches Ziellimit: alle gültigen Ziele im Wirkungsbereich.
use crate::world::World;

/// AoE-Typ (Ability-System.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AoeType {
    Single,
    TargetRadius,
    CasterRadius,
    Ground,
}

impl AoeType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "target_radius" => AoeType::TargetRadius,
            "caster_radius" => AoeType::CasterRadius,
            "ground" => AoeType::Ground,
            _ => AoeType::Single,
        }
    }
}

/// Ergebnis einer AoE-Zielauswahl.
#[derive(Debug, Clone)]
pub struct AoeResult {
    pub targets: Vec<String>,
    #[allow(dead_code)] // AoE-Zentrum für spätere S2C-Anzeige der Wirkfläche
    pub center_x: f64,
    #[allow(dead_code)]
    pub center_y: f64,
}

/// Wählt Ziele für eine AoE-Fähigkeit aus.
/// `caster_id`: Auslösender Spieler.
/// `target_id`: Gewähltes Ziel (bei Single/TargetRadius).
/// `ground_x/y`: Bodenposition (bei Ground).
/// `radius`: Wirkungsradius (bei AoE-Typen).
/// `host_effect`: true = feindlich, false = freundlich.
pub fn select_targets(
    world: &World,
    caster_id: &str,
    aoe_type: AoeType,
    target_id: Option<&str>,
    ground_x: Option<f64>,
    ground_y: Option<f64>,
    radius: f64,
    host_effect: bool,
) -> AoeResult {
    let caster = match world.players.get(caster_id) {
        Some(p) => p,
        None => return AoeResult { targets: Vec::new(), center_x: 0.0, center_y: 0.0 },
    };

    match aoe_type {
        AoeType::Single => {
            // Einzelziel: Target muss separat validiert sein
            let targets = target_id
                .map(|t| vec![t.to_string()])
                .unwrap_or_default();
            AoeResult {
                targets,
                center_x: caster.x,
                center_y: caster.y,
            }
        }

        AoeType::TargetRadius => {
            // Zentrum = Zielposition; alle gültigen Ziele im Radius
            let (cx, cy) = match target_id.and_then(|t| world.npcs.get(t).map(|n| (n.x, n.y))
                .or_else(|| world.players.get(t).map(|p| (p.x, p.y))))
            {
                Some(pos) => pos,
                None => return AoeResult { targets: Vec::new(), center_x: caster.x, center_y: caster.y },
            };
            let targets = collect_in_radius(world, caster_id, cx, cy, radius, host_effect);
            AoeResult { targets, center_x: cx, center_y: cy }
        }

        AoeType::CasterRadius => {
            // Zentrum = Caster
            let cx = caster.x;
            let cy = caster.y;
            let targets = collect_in_radius(world, caster_id, cx, cy, radius, host_effect);
            AoeResult { targets, center_x: cx, center_y: cy }
        }

        AoeType::Ground => {
            let cx = ground_x.unwrap_or(caster.x);
            let cy = ground_y.unwrap_or(caster.y);
            // Reichweitenprüfung muss außerhalb erfolgen
            let targets = collect_in_radius(world, caster_id, cx, cy, radius, host_effect);
            AoeResult { targets, center_x: cx, center_y: cy }
        }
    }
}

/// Sammelt alle gültigen Ziele (Spieler + NPCs) in einem Radius um (cx, cy).
/// Exkludiert den Caster.
fn collect_in_radius(
    world: &World,
    caster_id: &str,
    cx: f64,
    cy: f64,
    radius: f64,
    host_effect: bool,
) -> Vec<String> {
    let r2 = radius * radius;
    let mut targets = Vec::new();

    // Spieler
    for (pid, p) in &world.players {
        if pid == caster_id {
            continue; // Caster nie als Ziel
        }
        if p.hp <= 0 {
            continue;
        }
        let dx = p.x - cx;
        let dy = p.y - cy;
        if dx * dx + dy * dy <= r2 {
            if host_effect {
                // Feindliche AoE: Spieler als Ziel (kein PvP → grundsätzlich alle)
                targets.push(pid.clone());
            } else {
                // Freundliche AoE: alle lebendigen Spieler
                targets.push(pid.clone());
            }
        }
    }

    // NPCs
    for (nid, n) in &world.npcs {
        if n.hp <= 0 || n.status != crate::npc::NpcStatus::Alive {
            continue;
        }
        let dx = n.x - cx;
        let dy = n.y - cy;
        if dx * dx + dy * dy <= r2 {
            if host_effect && n.effective_attackable() {
                targets.push(nid.clone());
            } else if !host_effect {
                // Freundliche AoE auf NPCs: grundsätzlich lebendige
                targets.push(nid.clone());
            }
        }
    }

    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Player;
    use std::collections::HashSet;
    use std::time::Instant;
    use tokio::sync::mpsc;

    fn make_world() -> World {
        let mut w = World::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let a = Player {
            id: "a".into(), name: "a".into(),
            x: 0.0, y: 0.0, face: 0.0, ping_ms: 0, zone_id: 0,
            hp: 100, max_hp: 100, lang: "de".into(),
            account_id: 0, session_id: String::new(),
            entities: HashSet::new(), last_activity: Instant::now(), tx,
            char_class: "Warrior".into(), level: 1, armor: 0,
            weapon_skill: 1, combat: None,
            mana: 50, max_mana: 50,
            effects: Vec::new(), cooldowns: std::collections::BTreeMap::new(),
            active_cast: None,
            learned_abilities: HashSet::new(),
            sitting: false,
            attributes: Default::default(),
            max_hp_base: 100,
            max_mana_base: 50,
            hp_regen_bonus: 0.0,
            mana_regen_bonus: 0.0,
            hp_regen_carry: 0.0,
            mana_regen_carry: 0.0,
        };
        w.players.insert("a".into(), a);

        // NPC in Reichweite
        use crate::npc::{Npc, NpcStatus};
        let n = Npc {
            id: "npc_1".into(), spawn_id: 1, name: "Wolf".into(),
            kind: "normal".into(),
            attackable: true, aggressive: true,
            aggro_range: 8.0, attack_range: 1.5, attack_duration_ms: 1500,
            weapon_damage: 10, weapon_skill: 1, armor: 0,
            max_hp: 100, move_speed: 4.0, respawn_ms: 300000,
            faction: None, pack_id: None,
            home_x: 5.0, home_y: 0.0, home_radius: 5.0, leash_radius: 15.0,
            status: NpcStatus::Alive, hp: 100,
            x: 5.0, y: 0.0,
            target_id: None, last_attack: Instant::now(),
            no_link_since: None, return_started_at: None,
            respawn_after: None, claimed_by: None,
            override_ctx: None,
            effects: Vec::new(),
            cooldowns: std::collections::BTreeMap::new(),
            active_cast: None,
        };
        w.npcs.insert("npc_1".into(), n);
        w
    }

    #[test]
    fn single_target() {
        let w = make_world();
        let r = select_targets(&w, "a", AoeType::Single, Some("npc_1"), None, None, 0.0, true);
        assert_eq!(r.targets, vec!["npc_1"]);
    }

    #[test]
    fn target_radius_hits_nearby() {
        let w = make_world();
        // npc_1 bei (5,0), Radius 10
        let r = select_targets(&w, "a", AoeType::TargetRadius, Some("npc_1"), None, None, 10.0, true);
        assert!(r.targets.contains(&"npc_1".to_string()));
        assert_eq!(r.center_x, 5.0);
    }

    #[test]
    fn caster_radius_hits_nearby() {
        let w = make_world();
        let r = select_targets(&w, "a", AoeType::CasterRadius, None, None, None, 10.0, true);
        assert!(r.targets.contains(&"npc_1".to_string()));
    }

    #[test]
    fn ground_target() {
        let w = make_world();
        let r = select_targets(&w, "a", AoeType::Ground, None, Some(5.0), Some(0.0), 10.0, true);
        assert!(r.targets.contains(&"npc_1".to_string()));
        assert_eq!(r.center_x, 5.0);
    }

    #[test]
    fn caster_excluded() {
        let w = make_world();
        let r = select_targets(&w, "a", AoeType::CasterRadius, None, None, None, 100.0, true);
        assert!(!r.targets.contains(&"a".to_string()));
    }
}
