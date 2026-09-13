// context — ScriptContext für Script-Ausführung und Event-Dispatch
// (docs/Lua-Scripting-System.md §6, §8).
//
// Überreicht dem Script ein Kontext-Table (siehe convert.rs::context_to_lua).
// Nicht gesetzte Kontextfelder erscheinen NICHT im Lua-Table; das Script
// prüft die Anwesenheit mit `ctx.player_id ~= nil` usw.

use serde_json::Value as Json;

/// Kontext für einen einzelnen Script-Aufruf (Aufruf-/Event-Kontext).
#[derive(Debug, Clone, Default)]
pub struct ScriptContext {
    /// Charakter-/Spieler-ID (String, Realm-Konvention).
    pub player_id: Option<String>,
    /// NPC-Definitions-ID (String, Realm-Konvention).
    pub npc_id: Option<String>,
    /// Item-Definitions-ID (String, Realm-Konvention).
    pub item_id: Option<String>,
    /// Item-Instanz-/Serien-ID (String).
    pub item_instance_id: Option<String>,
    /// Quest-ID (String).
    pub quest_id: Option<String>,
    /// Zone-ID (u32, Realm-Konvention).
    pub zone_id: Option<u32>,
    /// Region-ID (String).
    pub region_id: Option<String>,
    /// Ability-ID (String, defs/abilities/*).
    pub ability_id: Option<String>,
    /// Domänenspezifischer Zusatzinhalt.
    pub payload: Option<Json>,
}

impl ScriptContext {
    /// Leerer Kontext.
    pub fn new() -> Self {
        Self::default()
    }
}
