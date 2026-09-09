# Kampfsystem V3 — Wiederverwendung bestehender Module

Kurze Übersicht, welche bestehenden Module in Combat V3 (Ability-System, Effekte, Cast, AoE) wiederverwendet werden können und wofür.

---

| Vorhandenes Modul | Kann für Combat V3 wiederverwendet werden für |
|---|---|
| `combat.rs` → `combat/mod.rs` (HitResult, resolve_attack, CombatState, combat_tick, class_cap, armor_reduction_pct, SplitMix64, CombatRng) | Gemeinsamer physischer Schadenskern für Auto-Angriffe und Ability-Schaden (Basis für alle Direct-Damage-Fähigkeiten). `combat_tick` verbleibt als Auto-Attack-Tick; V3-Fähigkeiten werden darüber hinaus über `ability_tick` verarbeitet. |
| `world.rs` (Player, World, Shared, world_tick, disconnect_player, apply_move, ensure_visible, AOFB-Radius) | Spieler-State, Verbindungsverwaltung, Sichtbarkeits-Broadcast, Bewegungslogik (Speed-Cap, Dispatch). Player bekommt neue Felder (mana, effects, cooldowns, active_cast). `apply_move` wird von `handle_move` genutzt; Bewegungs-Unterbrechung von Casts wird danach aufgerufen. |
| `npc.rs` (Npc, NpcStatus, npc_tick, build_npcs, aggro_trigger, broadcast_state, ContextOverride) | NPC/Monster-Kampfkern. Npc bekommt neue Felder (effects, cooldowns, active_cast). Aggro-Trigger wird für Ability-Aggro (Taunt, Direktangriff) wiederverwendet. Evade/Return-Logik inkl. Cooldown-/Effect-Reset. |
| `handlers.rs` (Ctx, handle_attack, handle_move) | Handle-Framework: `Ctx` wird für `handle_ability` genutzt. `handle_attack` bleibt für Auto-Attack; `handle_move` bekommt einen Hook für Cast-Unterbrechung. |
| `net.rs` (dispatch, serve) | WebSocket-Dispatch: neue `c2s::ABILITY`-Message wird dispatched. |
| `protocol.rs` (c2s, s2c, Frame) | Drahtformat: neue IDs `c2s::ABILITY=12`, `s2c::ABILITY=16`, `s2c::EFFECT=17`. `Frame` wird für alle Ability-/Effekt-Events genutzt. |
| `config.rs` (CombatCfg, NpcCfg) | Konfiguration: bestehende Weapon- und NPC-Werte bleiben. Keine zusätzlichen Config-Felder für V3 (Content-Werte kommen aus `ability_definitions`). |
| `db.rs` (Character, load_character, NpcDefRow, NpcSpawnRow, load_npc_*) | Persistenz-Schicht: Character bekommt `mana`/`mana_max`; neue Loader für `ability_definitions`, `character_abilities`. NPC-Loader bekommt `abilities`-Spalte. |
| `migrations.rs` (apply_migrations, parse_migration_name, collect_files) | Migrations-Framework: 010_combat_v3.sql wird automatisch erkannt und ausgeführt. |
| `combat::resolve_attack` | Wird für INSTANT-Direkt-Schaden mit physischer Trefferauflösung wiederverwendet (Schadenskategorie: SINGLE_TARGET_DAMAGE mit Waffen-Dauer). Magischer Schaden (DoTs) überspringt Rüstung (vorläufig, §7). |
| `combat::class_cap` / `armor_reduction_pct` | Rüstungsreduktion für physische Ability-Schadenseffekte. |
| `combat::SplitMix64` / `CombatRng` | Deterministischer RNG für Ability-Trefferauflösung (Blind-Schaden, Crit-Schaden), falls benötigt. |
| `world::Player.tx` / `Frame::encode` | Versand von S2C-Ability- und Effekt-Frames an alle sichtbaren Spieler. |
| `world::disconnect_player` | Effekte und Cast-State werden beim Disconnect aufgeräumt (death cleanup). |
| `npc::build_npcs` / `db::NpcDefRow` | NPC-Content-Layer bekommt `abilities`-Spalte: Ability-IDs werden als Komma-getrennte Liste geladen und als `Vec<String>` auf den Npc geschrieben. |
| `config::Config.tick_ms` | Takt-Intervall für `ability_tick` (Cast-Progress, DoT/HoT-Ticks, Effect-Ablauf). |
