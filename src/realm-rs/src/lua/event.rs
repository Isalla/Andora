// event — ScriptEvent-Transport für den Event-Dispatch
// (docs/Lua-Scripting-System.md §8).
//
// Die Eventnamen sind V1-intern und werden von zukünftigen Realm-Schichten
// (world.rs, npc.rs, item.rs, ability.rs, zone-/region-Tick, cutscene)
// über die Dispatch-Grundlage in runtime.rs verwendet. Die öffentlichen
// Namen werden Dokument-seitig erst in einer späteren Phase festgelegt.

use super::context::ScriptContext;

/// Ein serverseitig ausgelöstes Script-Event mit Kontext.
#[derive(Debug, Clone)]
pub struct ScriptEvent {
    /// Event-Name.
    pub name: String,
    /// Kontext, der an alle Handler dieses Events übergeben wird.
    pub context: ScriptContext,
}

impl ScriptEvent {
    /// Neues Event mit Kontext.
    pub fn new(name: impl Into<String>, context: ScriptContext) -> Self {
        Self {
            name: name.into(),
            context,
        }
    }
}
