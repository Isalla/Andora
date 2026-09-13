// sandbox — Sandbox-Aufbau für die Lua-Content-Schicht
// (docs/Lua-Scripting-System.md §6).
//
// Whitelist-Prinzip:
//   - Neue VM mit StdLib::NONE (mlua lädt immer nur die Basisbibliothek).
//   - Danach werden nur die erlaubten Standardbibliotheken geladen.
//   - Unerlaubte Globals (dofile, load, loadfile, collectgarbage, print,
//     warn, io, os, package, require, debug, _G) werden aus den echten
//     Globals entfernt (Defense-in-Depth innerhalb der VM).
//   - Für jedes Script wird eine frische Env-Tabelle mit Metatable
//     (__index = Surface) erzeugt: Das Surface stellt nur die erlaubten
//     Funktionen/Bibliotheken bereit; die echten Globals sind für Scripte
//     unerreichbar.
//   - getmetatable/setmetatable sind bewusst NICHT im Surface — kein Zugriff
//     auf die Env-Metatable und damit kein Ausbruch über die Kette.
//   - Ein eigenes `print`-Log richtet Ausgaben an env_logger (info).

use mlua::{Lua, Value, Variadic};

use super::error::LuaError;

/// Erlaubte Standardbibliotheken (mlua-Flags).
pub fn safe_libs() -> mlua::StdLib {
    mlua::StdLib::TABLE | mlua::StdLib::STRING | mlua::StdLib::MATH | mlua::StdLib::UTF8
}

/// Im Surface bereitgestellte Basis-Tabellen (Whitelist).
pub const LIBS: [&str; 3] = ["string", "math", "utf8"];

/// Im Surface bereitgestellte Basis-Funktionen (Whitelist).
///
/// Bewusst ausgeschlossen: getmetatable, setmetatable (Sandbox-Härtung),
/// dofile/load/loadfile/collectgarbage/print/warn (siehe GLOBAL_BLOCKLIST)
/// sowie io/os/package/debug (werden nie geladen).
pub const BASE_WHITELIST: [&str; 15] = [
    "assert", "error", "ipairs", "next", "pairs", "pcall", "rawequal", "rawget", "rawlen",
    "rawset", "select", "tonumber", "tostring", "type", "xpcall",
];

/// Globals, die aus der VM entfernt werden (Defense-in-Depth).
pub const GLOBAL_BLOCKLIST: [&str; 12] = [
    "dofile",
    "load",
    "loadfile",
    "collectgarbage",
    "print",
    "warn",
    "io",
    "os",
    "package",
    "require",
    "debug",
    "_G",
];

/// Erzeugt eine neue, gehärtete Lua-VM (SafeLibs geladen, schädliche
/// Globals entfernt). Jede VM wird ausschließlich vom zugewiesenen
/// ScriptManager genutzt (mlua::Lua ist nicht Send).
pub fn new_vm() -> Result<Lua, LuaError> {
    let lua = Lua::new_with(mlua::StdLib::NONE, mlua::LuaOptions::default())
        .map_err(|e| LuaError::internal(format!("Lua-VM erstellen: {e}")))?;
    lua.load_std_libs(safe_libs())
        .map_err(|e| LuaError::internal(format!("Standardbibliotheken laden: {e}")))?;
    sanitize_globals(&lua)?;
    Ok(lua)
}

/// Entfernt unerlaubte Einträge aus den echten Globals der VM.
pub fn sanitize_globals(lua: &Lua) -> Result<(), LuaError> {
    let globals = lua.globals();
    for key in GLOBAL_BLOCKLIST {
        globals
            .set(key, Value::Nil)
            .map_err(|e| LuaError::internal(format!("Globals säubern ('{key}'): {e}")))?;
    }
    Ok(())
}

/// Baut das „Surface-Table“, das den Scripten als `__index` der Env dient.
pub fn build_sandbox(lua: &Lua) -> Result<mlua::Table, LuaError> {
    let globals = lua.globals();
    let surface = lua
        .create_table()
        .map_err(|e| LuaError::internal(format!("Surface-Tabelle erstellen: {e}")))?;

    for key in BASE_WHITELIST {
        let value: Value = globals
            .get(key)
            .map_err(|e| LuaError::internal(format!("Surface-Global '{key}' lesen: {e}")))?;
        if !matches!(value, Value::Nil) {
            surface
                .set(key, value)
                .map_err(|e| LuaError::internal(format!("Surface-Global '{key}' setzen: {e}")))?;
        }
    }
    for lib in LIBS {
        let value: Value = globals
            .get(lib)
            .map_err(|e| LuaError::internal(format!("Surface-Lib '{lib}' lesen: {e}")))?;
        surface
            .set(lib, value)
            .map_err(|e| LuaError::internal(format!("Surface-Lib '{lib}' setzen: {e}")))?;
    }

    // Eigenes print -> env_logger (info). Kein Kontakt zu echten IO.
    let print = lua
        .create_function(|_lua, args: Variadic<Value>| {
            let parts: Vec<String> = args.iter().map(display).collect();
            log::info!("[lua] {}", parts.join("\t"));
            Ok(())
        })
        .map_err(|e| LuaError::internal(format!("print-Funktion erstellen: {e}")))?;
    surface
        .set("print", print)
        .map_err(|e| LuaError::internal(format!("Surface-print setzen: {e}")))?;

    Ok(surface)
}

/// Kompakte, sichere Darstellung eines Lua-Werts für Logs.
fn display(value: &Value) -> String {
    match value {
        Value::Nil => "nil".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.to_string_lossy(),
        other => format!("<{}>", other.type_name()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_shows_only_whitelist() {
        let lua = new_vm().unwrap();
        let surface = build_sandbox(&lua).unwrap();
        for key in BASE_WHITELIST {
            let v: Value = surface.get(key).unwrap();
            assert!(!matches!(v, Value::Nil), "{key} fehlt im Surface");
        }
        for name in [
            "getmetatable",
            "setmetatable",
            "io",
            "os",
            "package",
            "require",
            "debug",
        ] {
            let v: Value = surface.get(name).unwrap();
            assert!(
                matches!(v, Value::Nil),
                "{name} ist unerlaubterweise im Surface"
            );
        }
    }

    #[test]
    fn globals_are_cleaned() {
        let lua = new_vm().unwrap();
        let globals = lua.globals();
        for key in GLOBAL_BLOCKLIST {
            let v: Value = globals.get(key).unwrap();
            assert!(
                matches!(v, Value::Nil),
                "Global '{key}' wurde nicht entfernt"
            );
        }
    }

    #[test]
    fn whitelisted_libs_available() {
        let lua = new_vm().unwrap();
        let surface = build_sandbox(&lua).unwrap();
        for lib in LIBS {
            let v: Value = surface.get(lib).unwrap();
            assert!(!matches!(v, Value::Nil), "'{lib}' fehlt im Surface");
        }
    }
}
