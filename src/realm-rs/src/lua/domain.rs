// domain — die acht V1-Script-Domänen (docs/Lua-Scripting-System.md §9).
//
// Jede Domäne besitzt einen eigenen Content-Ordner unterhalb der vom Realm
// zur Laufzeit übergebenen Script-Wurzel (nicht hartkodiert im Rust-Code),
// z. B. quests/npc-415/aufstieg.lua. Zusätzliche Unterordner innerhalb einer
// Domäne (z. B. abilities/fighter/warrior/) sind reine Ordnerorganisation
// des Content-Autors und KEINE eigenständige Autorität: Sie werden nur für
// die Lesbarkeit/DATEIPFAD-Namensgebung genutzt (siehe ScriptId::full_id).

use std::fmt;

/// Die acht V1-Script-Domänen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ScriptDomain {
    /// Content rund um Quests (Minimal-Skripts: Informationen/Text).
    Quest,
    /// NPC-Verhalten (Minimal-Skripts: Informationen/Dialog-Stellen mit
    /// Rückverweis auf NPC-Definitionen in defs/).
    Npc,
    /// Item-Beschreibungstexte und -Eigenschaften.
    Item,
    /// Ability-/Skill-Content (Referenzen auf defs/abilities/*).
    Ability,
    /// Zone-Content (Zoneninfos, Zonenaktionen, Besatz/Territorium).
    Zone,
    /// Region-Content (Regionsinfos, Regionsaktionen).
    Region,
    /// Interaktionen/Trigger.
    Interaction,
    /// Cutscenes (Sequenzen aus geskripteten Ereignissen).
    Cutscene,
}

impl ScriptDomain {
    /// Content-Ordner dieser Domäne (z. B. für Script-Dateien).
    pub const fn folder(self) -> &'static str {
        match self {
            Self::Quest => "quests",
            Self::Npc => "npcs",
            Self::Item => "items",
            Self::Ability => "abilities",
            Self::Zone => "zones",
            Self::Region => "regions",
            Self::Interaction => "interactions",
            Self::Cutscene => "cutscenes",
        }
    }

    /// Kurzname für Fehlermeldungen (deutschfreundlich, deterministisch).
    pub const fn label(self) -> &'static str {
        match self {
            Self::Quest => "Quest",
            Self::Npc => "NPC",
            Self::Item => "Item",
            Self::Ability => "Ability",
            Self::Zone => "Zone",
            Self::Region => "Region",
            Self::Interaction => "Interaktion",
            Self::Cutscene => "Cutscene",
        }
    }

    /// Alle acht Domänen (deterministische Reihenfolge).
    pub const ALL: [ScriptDomain; 8] = [
        Self::Quest,
        Self::Npc,
        Self::Item,
        Self::Ability,
        Self::Zone,
        Self::Region,
        Self::Interaction,
        Self::Cutscene,
    ];
}

impl fmt::Display for ScriptDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}
