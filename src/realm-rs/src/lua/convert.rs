// convert — Konvertierungen zwischen Lua-Werten und JSON
// (docs/Lua-Scripting-System.md §6, §10).
//
// Nur JSON-fähige Werte dürfen zwischen Script und Host fließen:
//	nil, boolean, Ganzzahl, Zahl, String, Tabelle (Array oder Objekt).
// Funktionen, UserData, Threads und LightUserData werden abgelehnt
// (SandboxRejected). Verschachtelungstiefe ist auf MAX_DEPTH begrenzt.

use mlua::{Lua, Value};
use serde_json::{Map as JsonMap, Value as Json};

use super::context::ScriptContext;
use super::error::LuaError;

/// Maximale Verschachtelungstiefe für Payloads/Tabellen (Schutz vor
/// Rekursion- und Speicherproblemen). Gilt für den Weg Script -> Host.
pub const MAX_DEPTH: usize = 32;

/// Tiefenlimit für den Weg Host -> Script (Kontext). Server-JSON ist
/// ohnehin begrenzt; großzügig, da hier kein unkontrollierter Lua-Zustand
/// tiefe Rekursion erzeugen kann.
pub const CONTEXT_DEPTH_LIMIT: usize = 256;

/// Fehler beim Serialisieren eines Lua-Werts in JSON.
///
/// Bewusst KEIN LuaError-Subtyp: Der Host (host.rs) übersetzt diese Fehler
/// in die passenden ExternalErrors ([`super::error::HostRejected`] bzw.
/// [`super::error::SandboxRejected`]), damit das Script sie als Lua-Fehler
/// sieht und die Rust-Runtime sie typisiert auswerten kann.
#[derive(Debug)]
pub enum PayloadError {
    /// Zu tief verschachtelt.
    Depth,
    /// Werttyp ist für die Host-API nicht erlaubt.
    Unsupported { ty: &'static str },
}

impl PayloadError {
    pub(crate) fn into_payload_message(self) -> String {
        match self {
            Self::Depth => format!("Payload zu tief verschachtelt (Maximum {MAX_DEPTH} Ebenen)"),
            Self::Unsupported { ty } => {
                format!("Payload enthält nicht erlaubten Wert (Typ: {ty}) — nur nil/boolean/Zahl/String/Tabelle erlaubt")
            }
        }
    }
}

/// Übersetzt einen Rust-JSON-Wert in einen Lua-Wert (für Kontexte).
///
/// `null` wird zu nil; Booleans/Zahlen/Strings 1:1; Arrays werden zu
/// Tabellen mit Indizes ab 1; Objekte zu Tabellen. Ein JSON-null in einem
/// Array erzeugt eine Lücke (Setzen auf nil) — semantisch dem JSON-null
/// am nächsten, ausdrücklich dokumentiert.
pub fn json_to_lua_value(lua: &Lua, json: &Json, depth: usize) -> Result<Value, LuaError> {
    if depth > CONTEXT_DEPTH_LIMIT {
        return Err(LuaError::internal(format!(
            "Kontext zu tief verschachtelt (Maximum {CONTEXT_DEPTH_LIMIT} Ebenen)"
        )));
    }
    match json {
        Json::Null => Ok(Value::Nil),
        Json::Bool(b) => Ok(Value::Boolean(*b)),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Integer(i))
            } else if let Some(u) = n.as_u64() {
                // u64 > i64::MAX hat kein Lua-Integer-Gegenstück; dann als
                // Gleitkommazahl übernehmen.
                match i64::try_from(u) {
                    Ok(i) => Ok(Value::Integer(i)),
                    Err(_) => Ok(Value::Number(u as f64)),
                }
            } else {
                n.as_f64()
                    .map(Value::Number)
                    .ok_or_else(|| LuaError::internal("Zahl nicht als Lua-Zahl darstellbar"))
            }
        }
        Json::String(s) => {
            let ls = lua
                .create_string(s)
                .map_err(|e| LuaError::internal(format!("String in Lua-String umwandeln: {e}")))?;
            Ok(Value::String(ls))
        }
        Json::Array(items) => {
            let t = lua.create_table().map_err(|e| {
                LuaError::internal(format!("Tabelle für Kontext-Array erstellen: {e}"))
            })?;
            for (idx, item) in items.iter().enumerate() {
                let value = json_to_lua_value(lua, item, depth + 1)?;
                if !matches!(value, Value::Nil) {
                    t.set(idx + 1, value)
                        .map_err(|e| LuaError::internal(format!("Kontext-Array befüllen: {e}")))?;
                }
            }
            Ok(Value::Table(t))
        }
        Json::Object(entries) => {
            let t = lua.create_table().map_err(|e| {
                LuaError::internal(format!("Tabelle für Kontext-Objekt erstellen: {e}"))
            })?;
            for (key, item) in entries {
                let value = json_to_lua_value(lua, item, depth + 1)?;
                if !matches!(value, Value::Nil) {
                    t.set(key.as_str(), value)
                        .map_err(|e| LuaError::internal(format!("Kontext-Objekt befüllen: {e}")))?;
                }
            }
            Ok(Value::Table(t))
        }
    }
}

/// Baut den Lua-Kontext-Table aus einem [`ScriptContext`].
///
/// Nicht gesetzte Felder werden NICHT eingetragen (das Script prüft mit
/// `ctx.player_id ~= nil`). Ein gesetztes `payload` wird über
/// [`json_to_lua_value`] in `ctx.payload` überführt.
pub fn context_to_lua(lua: &Lua, ctx: &ScriptContext) -> Result<mlua::Table, LuaError> {
    let t = lua
        .create_table()
        .map_err(|e| LuaError::internal(format!("Kontext-Table erstellen: {e}")))?;
    macro_rules! set_opt_string {
        ($key:literal, $field:expr) => {
            if let Some(v) = $field.as_ref() {
                t.set($key, v.as_str()).map_err(|e| {
                    LuaError::internal(format!("Kontext-Feld '{}' setzen: {e}", $key))
                })?;
            }
        };
    }
    set_opt_string!("player_id", ctx.player_id);
    set_opt_string!("npc_id", ctx.npc_id);
    set_opt_string!("item_id", ctx.item_id);
    set_opt_string!("item_instance_id", ctx.item_instance_id);
    set_opt_string!("quest_id", ctx.quest_id);
    if let Some(v) = ctx.zone_id {
        t.set("zone_id", v)
            .map_err(|e| LuaError::internal(format!("Kontext-Feld 'zone_id' setzen: {e}")))?;
    }
    set_opt_string!("region_id", ctx.region_id);
    set_opt_string!("ability_id", ctx.ability_id);
    if let Some(payload) = &ctx.payload {
        let value = json_to_lua_value(lua, payload, 1)?;
        if !matches!(value, Value::Nil) {
            t.set("payload", value)
                .map_err(|e| LuaError::internal(format!("Kontext-Feld 'payload' setzen: {e}")))?;
        }
    }
    Ok(t)
}

/// Übersetzt einen Lua-Wert in einen erlaubten JSON-Wert.
///
/// Tabellen werden als Array (wenn indices 1..=n lückenlos) oder Objekt
/// behandelt. Nicht erlaubte Typen erzeugen [`PayloadError::Unsupported`],
/// zu tiefe Verschachtelung [`PayloadError::Depth`].
pub fn lua_value_to_json(value: &Value, depth: usize) -> Result<Json, PayloadError> {
    if depth > MAX_DEPTH {
        return Err(PayloadError::Depth);
    }
    match value {
        Value::Nil => Ok(Json::Null),
        Value::Boolean(b) => Ok(Json::Bool(*b)),
        Value::Integer(i) => Ok(Json::from(*i)),
        Value::Number(n) => {
            serde_json::Number::from_f64(*n)
                .map(Json::Number)
                .ok_or(PayloadError::Unsupported {
                    ty: "nicht-darstellbare Zahl",
                })
        }
        Value::String(s) => Ok(Json::String(s.to_string_lossy())),
        Value::Table(t) => lua_table_to_json(t, depth + 1),
        other => Err(PayloadError::Unsupported {
            ty: other.type_name(),
        }),
    }
}

/// Tabelle nach JSON: Array genau dann, wenn die Integer-Schlüssel exakt
/// 1..=n bilden (keine Lücke, keine anderen Schlüssel). Tabelle ohne
/// Integer-Start bei 1, mit Lücken oder mit Namensschlüsseln wird Objekt —
/// dabei bleiben alle Schlüssel erhalten (keine Datenverluste).
fn lua_table_to_json(t: &mlua::Table, depth: usize) -> Result<Json, PayloadError> {
    let mut dict: JsonMap<String, Json> = JsonMap::new();
    let mut is_object = false; // String-Schlüssel, Nicht-/Negativ-Integer oder Lücke gesehen
    let mut int_max: i64 = 0;
    let mut int_count: usize = 0;

    for pair in t.pairs::<Value, Value>() {
        let (key, item) = pair.map_err(|_| PayloadError::Unsupported { ty: "Tabelle" })?;
        let converted = lua_value_to_json(&item, depth)?;
        match key {
            Value::Integer(i) => {
                int_count += 1;
                if i > int_max {
                    int_max = i;
                }
                if i < 1 {
                    is_object = true;
                }
                dict.insert(i.to_string(), converted);
            }
            Value::String(s) => {
                is_object = true;
                dict.insert(s.to_string_lossy(), converted);
            }
            other => {
                return Err(PayloadError::Unsupported {
                    ty: other.type_name(),
                })
            }
        }
    }

    if !is_object && int_count > 0 && int_max as usize == int_count {
        let mut arr = Vec::with_capacity(int_count);
        for idx in 1..=int_count {
            arr.push(dict.remove(&idx.to_string()).unwrap_or(Json::Null));
        }
        Ok(Json::Array(arr))
    } else {
        Ok(Json::Object(dict))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_null_becomes_nil() {
        let lua = Lua::new();
        let value = json_to_lua_value(&lua, &Json::Null, 1).unwrap();
        assert!(matches!(value, Value::Nil));
    }

    #[test]
    fn table_as_array_roundtrip() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        t.set(1, "a").unwrap();
        t.set(2, "b").unwrap();
        t.set(3, 5).unwrap();
        let json = lua_value_to_json(&Value::Table(t), 1).unwrap();
        assert_eq!(json, serde_json::json!(["a", "b", 5]));
    }

    #[test]
    fn table_as_object_roundtrip() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        t.set("name", "Waldwolf").unwrap();
        t.set("level", 3).unwrap();
        let json = lua_value_to_json(&Value::Table(t), 1).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj["name"], Json::String("Waldwolf".into()));
        assert_eq!(obj["level"], Json::from(3));
    }

    #[test]
    fn function_rejected() {
        let lua = Lua::new();
        let f = lua.create_function(|_lua, _: ()| Ok(())).unwrap();
        let err = lua_value_to_json(&Value::Function(f), 1).unwrap_err();
        assert!(matches!(err, PayloadError::Unsupported { .. }));
    }

    #[test]
    fn depth_limit_enforced() {
        let lua = Lua::new();
        let inner = lua.create_table().unwrap();
        let outer = lua.create_table().unwrap();
        outer.set(1, inner).unwrap();
        let err = lua_value_to_json(&Value::Table(outer), MAX_DEPTH + 1).unwrap_err();
        assert!(matches!(err, PayloadError::Depth));
    }

    #[test]
    fn context_to_lua_omits_unset_fields() {
        let lua = Lua::new();
        let ctx = ScriptContext {
            player_id: Some("char-1".into()),
            zone_id: Some(7),
            ..Default::default()
        };
        let t = context_to_lua(&lua, &ctx).unwrap();
        assert_eq!(
            t.get::<mlua::Value>("player_id").unwrap(),
            mlua::Value::String(lua.create_string("char-1").unwrap())
        );
        assert_eq!(
            t.get::<mlua::Value>("zone_id").unwrap(),
            mlua::Value::Integer(7)
        );
        assert!(matches!(
            t.get::<mlua::Value>("npc_id").unwrap(),
            Value::Nil
        ));
    }
}
