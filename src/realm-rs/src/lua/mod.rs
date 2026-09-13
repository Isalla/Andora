// lua — Lua-Content-Scripting V1 (docs/Lua-Scripting-System.md).
//
// Serverseitige Content-/Orchestrierungsschicht: Lua beschreibt Content
// und fordert Aktionen über eine kontrollierte Host-/Realm-API an; der
// Rust-Realm bleibt für alle verbindlichen Spielregeln und den autoritativen
// Zustand zuständig (Combat, Progression, Items/Inventory, Loot, Queststatus,
// NPC-/World-State, Berechtigungen).
//
// V1-Grundlage (Auftragsdoku):
//   - Runtime: Lua 5.5 via mlua 0.12.x (Features lua55 + vendored).
//   - Sandbox: Whitelist-Prinzip (kein io/os/package/debug/Dateisystem/
//     Netz/DB), siehe sandbox.rs.
//   - ScriptManager (runtime.rs) als zentrale Runtime-Abstraktion: Laden,
//     Ausführen, Event-Dispatch, Fehlerisolation. Keine verteilten Lua-
//     Aufrufe über den Code.
//   - Acht Script-Domänen (domain.rs): Quest, NPC, Item, Ability, Zone,
//     Region, Interaction, Cutscene.
//   - Event-Dispatch-Grundlage (event.rs, runtime.rs) für spätere
//     Anbindungen (Spawn/Respawn/Hail/Killed, examined/used, quest events,
//     ability cast, zone/region enter/leave/tick, cutscene started/finished).
//   - Host-/Realm-API (host.rs): kontrollierte Content-Anfragen an Rust;
//     Ausführung bleibt Rust/autoritativ (Stub-Grenze, kein Fake-Gameplay).
//
// Hinweis: Alle serverseitig verwendeten V1-Namen (Callback-/Event-/Host-
// API-Namen, Kontext-Schlüssel) sind V1-intern. Die Dokumentation legt die
// endgültigen öffentlichen API-Namen bewusst erst in einer späteren Phase
// fest. Es wird KEINE finale öffentliche API vorweggenommen.
#![allow(dead_code)]

pub mod context;
pub mod convert;
pub mod domain;
pub mod error;
pub mod event;
pub mod host;
pub mod runtime;
pub mod sandbox;
pub mod script;
