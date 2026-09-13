// host — Host-/Realm-API für Scripte (docs/Lua-Scripting-System.md §10).
//
// `andora.emit(name, payload)` und `andora.request(action, params)`
// sammeln Anfragen an den Rust-Realm in einem pro-Aufruf geleerten
// RequestCollector. Die Ausführung der Anfragen obliegt dem Realm
// (Stub-Grenze: V1 sammelt und berichtet; der Realm entscheidet
// autoritativ, ob und was er ausführt). Die Namen sind V1-intern.
//
// Validierung (nicht vom Script umgehbar, da sie in Rust erfolgt):
//   - Name/Action: nicht leer, ≤ 128 Bytes, Zeichensatz [A-Za-z0-9._:/_-].
//   - Payload: nur JSON-fähige Werte, Tiefe max. convert::MAX_DEPTH.
// Ablehnungen werden als typisierte Lua-Fehler (HostRejected /
// SandboxRejected) an das Script zurückgegeben.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Lua, Table, Value};
use serde_json::{Map as JsonMap, Value as Json};

use super::convert::{lua_value_to_json, PayloadError};
use super::error::{HostRejected, LuaError, SandboxRejected};

/// Eine konsolidierte Anfrage eines Scripts an den Realm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealmRequest {
    /// `andora.emit(name, payload)`: Content-/Orchestrierungsereignis an
    /// den Realm (z. B. „quest_progress“, „spawn_npc“).
    Emit { name: String, payload: Json },
    /// `andora.request(action, params)`: Funktionsanfrage (z. B. Aktion
    /// „respawn_region“ mit Params).
    Request { action: String, params: Json },
}

impl RealmRequest {
    /// Kurzbeschreibung für Log-/Fehlermeldungen.
    pub fn describe(&self) -> String {
        match self {
            Self::Emit { name, .. } => format!("emit('{name}')"),
            Self::Request { action, .. } => format!("request('{action}')"),
        }
    }
}

/// Sammelt RealmRequest-Anfragen eines einzelnen Script-Aufrufs.
///
/// Der Collector wird als `Rc<RefCell<Vec<RealmRequest>>>` an die
/// Host-Closures gereicht (mlua-Closures brauchen `'static`) und nach jedem
/// Script-Aufruf geleert.
#[derive(Debug, Clone, Default)]
pub struct RequestCollector(pub Rc<RefCell<Vec<RealmRequest>>>);

impl RequestCollector {
    /// Neuer, leerer Collector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Nimmt eine Anfrage entgegen (während der Script-Ausführung).
    pub fn push(&self, request: RealmRequest) {
        self.0.borrow_mut().push(request);
    }

    /// Entnimmt alle bislang gesammelten Anfragen (nach einem Aufruf).
    pub fn drain(&self) -> Vec<RealmRequest> {
        std::mem::take(&mut self.0.borrow_mut())
    }

    /// Anzahl der gesammelten Anfragen.
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    /// `true`, wenn noch keine Anfragen gesammelt wurden.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Erlaubter Zeichensatz für Host-Namen/Aktionen ([A-Za-z0-9._:/_-]).
fn allowed_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '-')
}

/// Infix-Prüfung für Host-Namen; andernfalls Rückgabe als HostError-Text.
fn validate_host_name(ty: &'static str, value: &str) -> Result<String, HostRejected> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(HostRejected(format!("{ty}: Name darf nicht leer sein")));
    }
    if trimmed.len() > 128 {
        return Err(HostRejected(format!(
            "{ty}: Name darf höchstens 128 Zeichen lang sein"
        )));
    }
    if !trimmed.chars().all(allowed_host_char) {
        return Err(HostRejected(format!(
            "{ty}: Name enthält unerlaubte Zeichen (erlaubt: [A-Za-z0-9._:/_-])"
        )));
    }
    Ok(trimmed.to_string())
}

/// Übersetzt einen Lua-Payload in JSON.
///
/// Fehler werden in die passende typisierte ExternalError-Wurzel gewandelt
/// (nicht die Laufzeit als solche, wohl aber der Aufruf/Inhalt schlägt fehl).
fn payload_to_json(value: Option<Table>) -> Result<Json, PayloadError> {
    match value {
        None => Ok(Json::Object(JsonMap::new())),
        Some(t) => {
            let value = Value::Table(t);
            lua_value_to_json(&value, 1)
        }
    }
}

/// Wandelt einen Payload-Fehler in eine typisierte Sandbox-ExternalError-Wurzel.
fn as_external(err: PayloadError) -> mlua::Error {
    mlua::Error::external(SandboxRejected(err.into_payload_message()))
}

/// Installiert die `andora`-Host-Tabelle in die Env und koppelt sie an den
/// Collector.
pub fn install(lua: &Lua, env: &Table, collector: RequestCollector) -> Result<(), LuaError> {
    let host = lua
        .create_table()
        .map_err(|e| LuaError::internal(format!("andora-Tabelle erstellen: {e}")))?;

    let emit_collector = collector.clone();
    let emit = lua
        .create_function(move |_lua, (name, payload): (String, Option<Table>)| {
            let name = validate_host_name("emit", &name).map_err(host_rejected_to_mlua)?;
            let payload = payload_to_json(payload).map_err(as_external)?;
            emit_collector.push(RealmRequest::Emit { name, payload });
            Ok(true)
        })
        .map_err(|e| LuaError::internal(format!("andora.emit erstellen: {e}")))?;
    host.set("emit", emit)
        .map_err(|e| LuaError::internal(format!("andora.emit installieren: {e}")))?;

    let req_collector = collector.clone();
    let request = lua
        .create_function(move |_lua, (action, params): (String, Option<Table>)| {
            let action = validate_host_name("request", &action).map_err(host_rejected_to_mlua)?;
            let params = payload_to_json(params).map_err(as_external)?;
            req_collector.push(RealmRequest::Request { action, params });
            Ok(true)
        })
        .map_err(|e| LuaError::internal(format!("andora.request erstellen: {e}")))?;
    host.set("request", request)
        .map_err(|e| LuaError::internal(format!("andora.request installieren: {e}")))?;

    env.set("andora", host)
        .map_err(|e| LuaError::internal(format!("andora in Env installieren: {e}")))?;
    Ok(())
}

/// Übersetzt HostRejected in einen mlua-Fehler.
fn host_rejected_to_mlua(err: HostRejected) -> mlua::Error {
    mlua::Error::external(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_host_name_rules() {
        assert!(validate_host_name("emit", "quest_progress").is_ok());
        assert!(validate_host_name("emit", "npc.spawn/zone-1:10").is_ok());
        assert!(validate_host_name("emit", "").is_err());
        assert!(validate_host_name("emit", "   ").is_err());
        assert!(validate_host_name("emit", "a b").is_err());
        assert!(validate_host_name("emit", "ä").is_err());
        let long = "x".repeat(129);
        assert!(validate_host_name("emit", &long).is_err());
    }

    #[test]
    fn collector_drains_isolated() {
        let c = RequestCollector::new();
        c.push(RealmRequest::Emit {
            name: "e".into(),
            payload: Json::Null,
        });
        assert_eq!(c.len(), 1);
        let drained = c.drain();
        assert_eq!(drained.len(), 1);
        assert!(c.is_empty());
    }
}
