// script — Script-Identität, -Quellen und -Scan
// (docs/Lua-Scripting-System.md §3, §8, §19).
//
// Ein ScriptId identifiziert ein Script eindeutig über (Domäne, Name). Der
// Name ist der Script-Dateipfad relativ zum Domänenordner ohne ".lua" —
// Unterordner (z. B. "fighter/warrior/Kick") bleiben Bestandteil des Namens,
// sind aber NUR Ordnerorganisation, keine Autorität. Das vollständige
// Chunk-Label, mit dem Scripte im Fehlerfall erscheinen, ergibt sich aus
// ScriptDomain::folder() + Name (z. B. "@abilities/fighter/warrior/Kick.lua").

use std::path::{Path, PathBuf};

use super::domain::ScriptDomain;
use super::error::LuaError;

/// Eindeutige Identität eines Scripts innerhalb der Runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptId {
    /// Domäne des Scripts.
    pub domain: ScriptDomain,
    /// Pfad-innerer Name (relativ zum Domänenordner, ohne ".lua").
    pub name: String,
}

impl ScriptId {
    /// Neues ScriptId mit Determinismus-Garantie ohne Validierung (erwartet
    /// bereits geprüfte/gescannte Namen).
    pub fn new(domain: ScriptDomain, name: impl Into<String>) -> Self {
        Self {
            domain,
            name: name.into(),
        }
    }

    /// Vollständiger, stabiler Chunk-/Diagnose-Name:
    /// `@{Domänenordner}/{Name}.lua`.
    pub fn full_id(&self) -> String {
        format!("@{}/{}.lua", self.domain.folder(), self.name)
    }

    /// Humane Bezeichnung für Fehlertexte.
    pub fn label(&self) -> String {
        format!("{} '{}'", self.domain.label(), self.name)
    }
}

/// Ein geladenes Script (Inhalt + Identität). Unveränderlich nach dem Laden;
/// erneutes Laden mit gleicher ID ersetzt kontrolliert den bisherigen
/// Inhalt (siehe ScriptManager::load_script) — KEINE automatische Live-
/// Nachladung von laufenden Zuständen in dieser Stufe.
#[derive(Debug, Clone)]
pub struct Script {
    /// Identität des Scripts.
    pub id: ScriptId,
    /// Lua-Quellcode.
    pub source: String,
}

/// Quelle, aus der ein [`Script`] geladen werden kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptSource {
    /// Lua-Quelltext direkt (Datenmodell-/Testpfad).
    Code {
        /// Script-Name (relativ zum Domänenordner, ohne ".lua").
        name: String,
        /// Lua-Quellcode.
        code: String,
    },
    /// Lua-Datei im Domänenordner der Script-Wurzel.
    File {
        /// Script-Name (relativ zum Domänenordner, ohne ".lua").
        name: String,
        /// Absoluter Dateipfad.
        path: PathBuf,
    },
}

impl ScriptSource {
    /// Intentional identische Namen, damit das gleiche Script unter
    /// [`ScriptId`] unabhängig von der Quelle ladbar bleibt.
    pub fn name(&self) -> &str {
        match self {
            Self::Code { name, .. } | Self::File { name, .. } => name,
        }
    }
}

/// Hält einen gefundenen Script-Dateipfad samt Ziel-Name.
pub(crate) struct ScriptFile {
    /// Name relativ zum Domänenordner, ohne ".lua".
    pub name: String,
    /// Absoluter Dateipfad.
    pub path: PathBuf,
}

/// Durchsucht den Domänenordner der angegebenen Script-Wurzel nach Script-
/// Dateien ("*.lua") und gibt sie in deterministischer Reihenfolge (nach
/// Pfad sortiert) zurück.
///
/// Regeln (docs §8):
///   - Nur Dateien mit der Endung ".lua" werden berücksichtigt.
///   - Symlinks werden übersprungen (kein Traversal außerhalb des Ordners,
///     kein Zyklusrisiko).
///   - Unterordner (auch verschachtelt) werden durchwandert; jeder
///     Unterordnerpfad ist Bestandteil des Script-Namens.
///   - Ein `shared/`-Ordner (Hilfscode) wird NICHT als Domänenscript
///     geladen.
pub(crate) fn scan_script_files(
    root: &Path,
    domain: ScriptDomain,
) -> Result<Vec<ScriptFile>, LuaError> {
    let dir = root.join(domain.folder());
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    let mut stack = vec![dir.clone()];
    let mut visited: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    while let Some(current) = stack.pop() {
        if !visited.insert(current.clone()) {
            continue; // Zyklusschutz
        }
        let entries = std::fs::read_dir(&current).map_err(|e| {
            LuaError::internal(format!("Script-Ordner '{}' lesen: {e}", current.display()))
        })?;
        let mut subdirs = Vec::new();
        for entry in entries {
            let entry = entry
                .map_err(|e| LuaError::internal(format!("Script-Ordner-Eintrag lesen: {e}")))?;
            let ft = entry.file_type().map_err(|e| {
                LuaError::internal(format!(
                    "Dateityp von '{}' ermitteln: {e}",
                    entry.path().display()
                ))
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if ft.is_symlink() {
                continue;
            }
            let target = entry.path();
            if ft.is_dir() {
                if name != "shared" {
                    subdirs.push(target);
                }
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            if !name.ends_with(".lua") {
                continue;
            }
            let rel = target
                .strip_prefix(&dir)
                .unwrap_or(&target)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            let scrip_name = rel.strip_suffix(".lua").unwrap_or(&rel).to_string();
            found.push(ScriptFile {
                name: scrip_name,
                path: target,
            });
        }
        stack.extend(subdirs);
    }

    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn tmp_root(tag: impl AsRef<std::path::Path>) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("pimmo-lua-scan-{}", tag.as_ref().display()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn scan_finds_only_lua_in_domain_folders() {
        let root = tmp_root("domains");
        fs::create_dir_all(root.join("quests/nested")).unwrap();
        fs::create_dir_all(root.join("npcs")).unwrap();
        fs::create_dir_all(root.join("abilities/fighter/warrior")).unwrap();
        fs::create_dir_all(root.join("shared")).unwrap();
        fs::write(root.join("quests/a.lua"), "return {}").unwrap();
        fs::write(root.join("quests/nested/b.lua"), "return {}").unwrap();
        fs::write(root.join("npcs/handl.lua"), "return {}").unwrap();
        fs::write(root.join("abilities/fighter/warrior/Kick.lua"), "return {}").unwrap();
        fs::write(root.join("shared/helper.lua"), "return {}").unwrap();
        fs::write(root.join("npcs/readme.txt"), "ignored").unwrap();
        fs::write(root.join("npcs/ignored.txt"), "ignored").unwrap();

        let quests = scan_script_files(&root, ScriptDomain::Quest).unwrap();
        let npcs = scan_script_files(&root, ScriptDomain::Npc).unwrap();
        let abilities = scan_script_files(&root, ScriptDomain::Ability).unwrap();
        assert_eq!(
            quests.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["a", "nested/b"]
        );
        assert_eq!(
            npcs.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["handl"]
        );
        assert_eq!(
            abilities
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["fighter/warrior/Kick"]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn full_id_matches_documentation_format() {
        let id = ScriptId::new(ScriptDomain::Ability, "fighter/warrior/Kick");
        assert_eq!(id.full_id(), "@abilities/fighter/warrior/Kick.lua");
    }
}
