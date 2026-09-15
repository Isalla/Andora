// runtime — ScriptManager: zentrale Runtime-Abstraktion für Scripts
// (docs/Lua-Scripting-System.md §3, §6, §8, §18; Controller-Konzept §19).
//
// ALLE Lua-Ausführungen laufen über diesen Manager — es gibt bewusst keine
// verteilten Lua-Aufrufe über andere Realm-Code. Der Manager kapselt:
//   - VM-Lebenszyklus (meine eigene Sandbox-VM),
//   - Scripts-Laden (Quelltext, Syntaxprüfung) und Registry,
//   - Ausführen (run: Top-Level, call: benannter Callback),
//   - Event-Dispatch-Grundlage (register_event_handler/dispatch_event),
//   - Fehlerisolation (jeder Fehler wird strukturiert zurückgegeben; kein
//     Script kann den Prozess oder andere Scripts beeinträchtigen),
//   - Host-API-Anfragen (andora.emit / andora.request, host.rs).
//
// Lebensdauer/Zuständigkeit: Der Manager gehört in V1 keinem Realm-Thread
// an (keine Produktions-Verdrahtung in dieser Stufe, docs §3-Eckfälle).
// Für V1 gilt: nicht Send (mlua-Lua), single-threaded Ausführung.

use std::collections::HashMap;
use std::path::Path;

use mlua::{Function, Table, Value};

use super::context::ScriptContext;
use super::convert;
use super::domain::ScriptDomain;
use super::error::LuaError;
use super::event::ScriptEvent;
use super::host::{self, RealmRequest, RequestCollector};
use super::sandbox;
use super::script::{scan_script_files, Script, ScriptId, ScriptSource};

/// Ergebnis eines Script-Laufs (Aufruf oder Event-Funktion): die vom Script
/// gesammelten Realm-Anfragen (andora.emit/andora.request) samt Script-ID.
#[derive(Debug, Clone)]
pub struct DispatchResult {
    /// Betroffenes Script.
    pub script_id: ScriptId,
    /// Vom Script gesammelte Realm-Anfragen (leer, wenn es keine gestellt hat).
    pub requests: Vec<RealmRequest>,
}

/// Zentraler Manager für alle Lua-Scripts (eine Instanz pro Realm).
#[derive(Debug)]
pub struct ScriptManager {
    /// Gehärtete Lua-VM (sandbox.rs).
    lua: mlua::Lua,
    /// Whitelist-Surface (__index aller Script-Env-Tabellen).
    surface: mlua::Table,
    /// Geladene Scripts (ID -> Inhalt).
    scripts: HashMap<ScriptId, Script>,
    /// Event-Handler-Registry: (Domäne, Event-Name) -> Script-IDs
    /// (docs §8, Server-Router).
    handlers: HashMap<(ScriptDomain, String), Vec<ScriptId>>,
}

impl ScriptManager {
    /// Erstellt eine neue, vollständig isolierte Script-Runtime.
    pub fn new() -> Result<Self, LuaError> {
        let lua = sandbox::new_vm()?;
        let surface = sandbox::build_sandbox(&lua)?;
        Ok(Self {
            lua,
            surface,
            scripts: HashMap::new(),
            handlers: HashMap::new(),
        })
    }

    /// Erzeugt eine frische Script-Runtime mit neuer Lua-VM.
    ///
    /// Wird beim Reset eines Dedicated Workers benötigt (§11): alter
    /// VM-/Script-Zustand wird vollständig verworfen, kein Kontext
    /// überlebt die Wiederverwendung.
    pub fn fresh_vm() -> Result<Self, LuaError> {
        Self::new()
    }

    /// Anzahl der geladenen Scripts.
    pub fn script_count(&self) -> usize {
        self.scripts.len()
    }

    /// `true`, wenn ein Script mit dieser ID geladen ist.
    pub fn has_script(&self, id: &ScriptId) -> bool {
        self.scripts.contains_key(id)
    }

    /// Geladenes Script per ID.
    pub fn script(&self, id: &ScriptId) -> Result<&Script, LuaError> {
        self.scripts.get(id).ok_or_else(|| {
            let domain = id.domain;
            LuaError::ScriptNotFound {
                domain,
                name: id.name.clone(),
            }
        })
    }

    /// Alle geladenen Script-IDs (unbestimmte Reihenfolge).
    pub fn script_ids(&self) -> impl Iterator<Item = &ScriptId> {
        self.scripts.keys()
    }

    /// Lädt ein Script aus einer Quelle, prüft Syntax und registriert es.
    ///
    /// Kontrollierter Reload: Lädt man unter derselben ID erneut, ersetzt der
    /// neue Inhalt den bisherigen deterministisch (kein Live-Hot-Reload von
    /// laufenden Zuständen).
    pub fn load_script(
        &mut self,
        domain: ScriptDomain,
        source: ScriptSource,
    ) -> Result<ScriptId, LuaError> {
        let name = source.name().trim().to_string();
        if name.is_empty() {
            return Err(LuaError::LoadError {
                domain,
                name: "<leer>".to_string(),
                message: "Script-Name darf nicht leer sein".to_string(),
            });
        }
        let code = match &source {
            ScriptSource::Code { code, .. } => code.clone(),
            ScriptSource::File { path, .. } => {
                std::fs::read_to_string(path).map_err(|e| LuaError::LoadError {
                    domain,
                    name: name.clone(),
                    message: format!("Script-Datei '{}' lesen: {e}", path.display()),
                })?
            }
        };
        let id = ScriptId::new(domain, name);

        let chunk = self.lua.load(&code).set_name(id.full_id());
        chunk
            .into_function()
            .map_err(|e| map_compile_error(&id, e))?;

        self.scripts.insert(
            id.clone(),
            Script {
                id: id.clone(),
                source: code,
            },
        );
        Ok(id)
    }

    /// Lädt alle Script-Dateien aus allen acht Domänenordnern unter `root`.
    ///
    /// Fehlerhafte Dateien brechen das Laden NICHT ab und werden gesammelt
    /// zurückgegeben (Fehlerisolation, §18). `shared/`-Dateien werden nicht
    /// geladen.
    pub fn load_script_dir(&mut self, root: &Path) -> (Vec<ScriptId>, Vec<LuaError>) {
        let mut ids = Vec::new();
        let mut errors = Vec::new();
        for domain in ScriptDomain::ALL {
            let files = match scan_script_files(root, domain) {
                Ok(files) => files,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            for file in files {
                match self.load_script(
                    domain,
                    ScriptSource::File {
                        name: file.name.clone(),
                        path: file.path.clone(),
                    },
                ) {
                    Ok(id) => ids.push(id),
                    Err(e) => errors.push(e),
                }
            }
        }
        (ids, errors)
    }

    /// Führt das Top-Level eines Scripts aus (ohne Callback-Anforderung).
    ///
    /// Das Script kann `andora.emit/request` aufrufen; die gesammelten
    /// Anfragen landen im [`DispatchResult`] (Rust-Realm entscheidet über
    /// Ausführung — Stub-Grenze).
    pub fn run(&mut self, id: &ScriptId, ctx: &ScriptContext) -> Result<DispatchResult, LuaError> {
        let script = self.script(id)?.clone();
        let requests = self.exec(&script, None, ctx)?;
        Ok(DispatchResult {
            script_id: id.clone(),
            requests,
        })
    }

    /// Führt einen benannten Callback aus der Callback-Tabelle des Scripts
    /// aus (z. B. einen Event-Handler).
    pub fn call(
        &mut self,
        id: &ScriptId,
        callback: &str,
        ctx: &ScriptContext,
    ) -> Result<DispatchResult, LuaError> {
        let script = self.script(id)?.clone();
        let requests = self.exec(&script, Some(callback), ctx)?;
        Ok(DispatchResult {
            script_id: id.clone(),
            requests,
        })
    }

    /// Registriert ein Script für ein (Domänen-, Event-Name-) Paar
    /// (docs §8, Server-Router-Tabelle).
    pub fn register_event_handler(
        &mut self,
        domain: ScriptDomain,
        event: impl Into<String>,
        id: ScriptId,
    ) {
        self.handlers
            .entry((domain, event.into()))
            .or_default()
            .push(id);
    }

    /// Alle (event-)registrierten Scripts; Diagnosehelfer.
    pub(crate) fn registered_event_handlers(
        &self,
    ) -> impl Iterator<Item = (&(ScriptDomain, String), &Vec<ScriptId>)> {
        self.handlers.iter()
    }

    /// Dispatch-Fundament: ruft alle Handler des Events der Reihe nach auf.
    ///
    /// Fehler eines Handlers isolieren sich: nur dessen Eintrag wird als
    /// Err gemeldet, die übrigen Handler laufen weiter (Fehlerisolation §18).
    pub fn dispatch_event(
        &mut self,
        domain: ScriptDomain,
        event: &ScriptEvent,
    ) -> Vec<Result<DispatchResult, LuaError>> {
        let ids: Vec<ScriptId> = self
            .handlers
            .get(&(domain, event.name.clone()))
            .cloned()
            .unwrap_or_default();
        let mut results = Vec::with_capacity(ids.len());
        for id in ids {
            match self.script(&id) {
                Ok(script) => {
                    let script = script.clone();
                    let requests = match self.exec(&script, Some(&event.name), &event.context) {
                        Ok(reqs) => reqs,
                        Err(e) => {
                            results.push(Err(e));
                            continue;
                        }
                    };
                    results.push(Ok(DispatchResult {
                        script_id: id,
                        requests,
                    }));
                }
                Err(e) => results.push(Err(e)),
            }
        }
        results
    }

    /// Baut eine frische Script-Env mit __index -> Surface und andora-Host.
    fn build_env(&self, collector: &RequestCollector) -> Result<Table, LuaError> {
        let env = self
            .lua
            .create_table()
            .map_err(|e| LuaError::internal(format!("Env erstellen: {e}")))?;
        let mt = self
            .lua
            .create_table()
            .map_err(|e| LuaError::internal(format!("Env-Metatable erstellen: {e}")))?;
        mt.set("__index", self.surface.clone())
            .map_err(|e| LuaError::internal(format!("Env-Metatable befüllen: {e}")))?;
        env.set_metatable(Some(mt))
            .map_err(|e| LuaError::internal(format!("Env-Metatable setzen: {e}")))?;
        host::install(&self.lua, &env, collector.clone())?;
        Ok(env)
    }

    /// Kern-Ausführung: Env aufbauen, Script kompilieren & als Chunk mit
    /// Kontext-Table aufrufen, optional den benannten Callback nachziehen
    /// und alle gesammelten Anfragen zurückgeben.
    fn exec(
        &self,
        script: &Script,
        callback: Option<&str>,
        ctx: &ScriptContext,
    ) -> Result<Vec<RealmRequest>, LuaError> {
        let collector = RequestCollector::new();
        let env = self.build_env(&collector)?;
        let ctx_table = convert::context_to_lua(&self.lua, ctx)?;

        let chunk = self
            .lua
            .load(&script.source)
            .set_name(script.id.full_id())
            .set_environment(env);
        let top: Value = chunk
            .call(ctx_table.clone())
            .map_err(|e| map_runtime_error(&script.id, e))?;

        let callbacks = match top {
            Value::Table(t) => t,
            Value::Nil if callback.is_none() => {
                // run-Modus: Rückgabewert optional (z. B. reine Top-Level-
                // Effekte), keine Callback-Funktion nötig.
                return Ok(collector.drain());
            }
            Value::Nil => {
                return Err(LuaError::InvalidScript {
                    domain: script.id.domain,
                    name: script.id.name.clone(),
                    message: "Script liefert keine Callback-Tabelle".to_string(),
                })
            }
            other => {
                return Err(LuaError::InvalidScript {
                    domain: script.id.domain,
                    name: script.id.name.clone(),
                    message: format!(
                        "Script liefert {}, erwartet wird eine Callback-Tabelle",
                        other.type_name()
                    ),
                })
            }
        };

        if let Some(callback) = callback {
            let cb: Value = callbacks
                .get::<mlua::Value>(callback)
                .map_err(|e| LuaError::internal(format!("Callback '{callback}' lesen: {e}")))?;
            let func: Function = match cb {
                Value::Function(f) => f,
                _other => {
                    return Err(LuaError::UnknownCallback {
                        domain: script.id.domain,
                        name: script.id.name.clone(),
                        callback: callback.to_string(),
                    })
                }
            };
            func.call::<mlua::Value>(ctx_table)
                .map_err(|e| map_runtime_error(&script.id, e))?;
        }

        Ok(collector.drain())
    }
}

/// Übersetzt einen Fehler beim Laden/Kompilieren in einen strukturierten
/// LuaError.
fn map_compile_error(id: &ScriptId, e: mlua::Error) -> LuaError {
    match e {
        mlua::Error::SyntaxError { message, .. } => LuaError::SyntaxError {
            domain: id.domain,
            name: id.name.clone(),
            message,
        },
        other => LuaError::LoadError {
            domain: id.domain,
            name: id.name.clone(),
            message: other.to_string(),
        },
    }
}

/// Übersetzt einen Laufzeitfehler in einen strukturierten LuaError.
///
/// Host-/Sandbox-Ablehnungen (Error::external-Wurzeln mit HostRejected /
/// SandboxRejected) werden typisiert wieder herausgelöst; alles andere ist
/// ein isolierter RuntimeError.
fn map_runtime_error(id: &ScriptId, e: mlua::Error) -> LuaError {
    if let Some(rej) = e.downcast_ref::<super::error::HostRejected>() {
        return LuaError::HostError {
            message: rej.0.clone(),
        };
    }
    if let Some(rej) = e.downcast_ref::<super::error::SandboxRejected>() {
        return LuaError::SandboxViolation {
            message: rej.0.clone(),
        };
    }
    let message = match &e {
        mlua::Error::SyntaxError { message, .. } => {
            return LuaError::SyntaxError {
                domain: id.domain,
                name: id.name.clone(),
                message: message.clone(),
            }
        }
        mlua::Error::RuntimeError(m) => m.clone(),
        mlua::Error::MemoryError(m) => format!("Speicherfehler: {m}"),
        mlua::Error::SafetyError(m) => format!("Sicherheitsfehler: {m}"),
        mlua::Error::CallbackError { cause, traceback } => format!("{cause}\n{traceback}"),
        other => other.to_string(),
    };
    LuaError::RuntimeError {
        domain: id.domain,
        name: id.name.clone(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn code_source(name: impl Into<String>, code: impl Into<String>) -> ScriptSource {
        ScriptSource::Code {
            name: name.into(),
            code: code.into(),
        }
    }

    fn minimal_returns(ctx: &str) -> String {
        format!("return {{ handle = function(c) {ctx} end }}")
    }

    fn tmp_root(tag: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("pimmo-lua-runtime-{tag}"));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn manager_new_and_empty() {
        let m = ScriptManager::new().unwrap();
        assert_eq!(m.script_count(), 0);
    }

    #[test]
    fn load_syntax_check_and_has_script() {
        let mut m = ScriptManager::new().unwrap();
        let id = m
            .load_script(
                ScriptDomain::Npc,
                code_source("handl", minimal_returns("return 1")),
            )
            .unwrap();
        assert!(m.has_script(&id));
        assert_eq!(m.script_count(), 1);
    }

    #[test]
    fn load_syntax_error_isolated() {
        let mut m = ScriptManager::new().unwrap();
        let err = m
            .load_script(ScriptDomain::Npc, code_source("kaputt", "function (("))
            .unwrap_err();
        assert!(matches!(err, LuaError::SyntaxError { .. }));
        assert_eq!(m.script_count(), 0);
        // Manager bleibt nutzbar.
        let id = m
            .load_script(ScriptDomain::Npc, code_source("ok", minimal_returns("")))
            .unwrap();
        assert!(m.has_script(&id));
    }

    #[test]
    fn reload_replaces_controlled() {
        let mut m = ScriptManager::new().unwrap();
        let id = m
            .load_script(
                ScriptDomain::Quest,
                code_source("aufstieg", "return { v = 1 }"),
            )
            .unwrap();
        let id2 = m
            .load_script(
                ScriptDomain::Quest,
                code_source("aufstieg", "return { v = 2 }"),
            )
            .unwrap();
        assert_eq!(id, id2);
        assert_eq!(m.script_count(), 1);
        let s = m.script(&id).unwrap();
        assert!(s.source.contains("v = 2"));
    }

    #[test]
    fn run_captures_top_level_requests() {
        let mut m = ScriptManager::new().unwrap();
        let id = m
            .load_script(
                ScriptDomain::Zone,
                code_source(
                    "gebiet",
                    "andora.request('area_info', { area = 'weiden' }); return {}",
                ),
            )
            .unwrap();
        let result = m.run(&id, &ScriptContext::new()).unwrap();
        assert_eq!(result.requests.len(), 1);
        match &result.requests[0] {
            RealmRequest::Request { action, params } => {
                assert_eq!(action, "area_info");
                assert_eq!(params, &serde_json::json!({ "area": "weiden" }));
            }
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }

    #[test]
    fn call_callback_receives_context() {
        let mut m = ScriptManager::new().unwrap();
        let id = m
            .load_script(
                ScriptDomain::Npc,
                code_source(
                    "handl",
                    minimal_returns(
                        "andora.emit('hail', { player = c.player_id, zone = c.zone_id })",
                    ),
                ),
            )
            .unwrap();
        let ctx = ScriptContext {
            player_id: Some("char-1".into()),
            zone_id: Some(7),
            ..Default::default()
        };
        let result = m.call(&id, "handle", &ctx).unwrap();
        let request = &result.requests[0];
        match request {
            RealmRequest::Emit { name, payload } => {
                assert_eq!(name, "hail");
                assert_eq!(
                    payload,
                    &serde_json::json!({ "player": "char-1", "zone": 7 })
                );
            }
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }

    #[test]
    fn unknown_callback_errors() {
        let mut m = ScriptManager::new().unwrap();
        let id = m
            .load_script(ScriptDomain::Npc, code_source("handl", minimal_returns("")))
            .unwrap();
        let err = m
            .call(&id, "gibt_es_nicht", &ScriptContext::new())
            .unwrap_err();
        assert!(matches!(
            err,
            LuaError::UnknownCallback { callback, .. }
                if callback == "gibt_es_nicht"
        ));
    }

    #[test]
    fn missing_script_errors() {
        let mut m = ScriptManager::new().unwrap();
        let id = ScriptId::new(ScriptDomain::Item, "unbekannt");
        let err = m.run(&id, &ScriptContext::new()).unwrap_err();
        assert!(matches!(err, LuaError::ScriptNotFound { .. }));
    }

    #[test]
    fn runtime_error_isolated_and_manager_stable() {
        let mut m = ScriptManager::new().unwrap();
        let bad = m
            .load_script(
                ScriptDomain::Npc,
                code_source("stürzt", minimal_returns("error('kaputt')")),
            )
            .unwrap();
        let err = m.call(&bad, "handle", &ScriptContext::new()).unwrap_err();
        assert!(matches!(err, LuaError::RuntimeError { .. }));

        // Manager und andere Scripts unberührt.
        let good = m
            .load_script(ScriptDomain::Npc, code_source("handl", minimal_returns("")))
            .unwrap();
        assert!(m.call(&good, "handle", &ScriptContext::new()).is_ok());
    }

    #[test]
    fn sandbox_blocks_dangerous_environment() {
        let mut m = ScriptManager::new().unwrap();
        let id = m
            .load_script(
                ScriptDomain::Npc,
                code_source(
                    "probe",
                    minimal_returns(&format!(
                        "local f = os and os.getenv or io;
                         andora.emit('probe', {{
                             has_os = os ~= nil,
                             has_io = io ~= nil,
                             has_require = require ~= nil,
                             has_debug = debug ~= nil,
                             has_load = load ~= nil,
                         }})"
                    )),
                ),
            )
            .unwrap();
        let result = m.call(&id, "handle", &ScriptContext::new()).unwrap();
        let payload = match &result.requests[0] {
            RealmRequest::Emit { payload, .. } => payload,
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        };
        assert_eq!(payload["has_os"], false);
        assert_eq!(payload["has_io"], false);
        assert_eq!(payload["has_require"], false);
        assert_eq!(payload["has_debug"], false);
        assert_eq!(payload["has_load"], false);
    }

    #[test]
    fn sandbox_errors_on_forbidden_use() {
        let mut m = ScriptManager::new().unwrap();
        let id = m
            .load_script(
                ScriptDomain::Npc,
                code_source("probe2", minimal_returns("error(os.getenv('HOME'))")),
            )
            .unwrap();
        let err = m.call(&id, "handle", &ScriptContext::new()).unwrap_err();
        // Zugriff auf nil (os) -> RuntimeError, kein Absturz.
        assert!(matches!(err, LuaError::RuntimeError { .. }));
    }

    #[test]
    fn host_validation_not_bypassable() {
        let mut m = ScriptManager::new().unwrap();
        let id = m
            .load_script(
                ScriptDomain::Interaction,
                code_source(
                    "boese",
                    minimal_returns(
                        "local ok, err = pcall(andora.emit, 'a b', { x = 1 });
                         if not ok then andora.emit('gerettet', { grund = tostring(err) }) end
                         return true",
                    ),
                ),
            )
            .unwrap();
        // emit mit Leerzeichen wird abgelehnt (HostRejected -> Lua-Fehler),
        // der pcall fängt das ab, die Folge-emit ist gültig.
        let result = m.call(&id, "handle", &ScriptContext::new()).unwrap();
        match &result.requests[0] {
            RealmRequest::Emit { name, .. } => assert_eq!(name, "gerettet"),
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }

    #[test]
    fn host_validation_deep_payload_rejected() {
        let mut m = ScriptManager::new().unwrap();
        let mut deep = serde_json::json!(null);
        for _ in 0..40 {
            deep = serde_json::json!({ "d": deep });
        }
        let id = m
            .load_script(
                ScriptDomain::Npc,
                code_source(
                    "tief",
                    "return { handle = function(c)
                                local ok, err = pcall(andora.emit, 'tief', c.payload)
                                if not ok then andora.emit('abgefangen', { m = tostring(err) }) end
                             end }",
                ),
            )
            .unwrap();
        let ctx = ScriptContext {
            payload: Some(deep),
            ..Default::default()
        };
        // context_to_lua baut den tiefen Kontext trotzdem (Bound auf JSON);
        // erst der emit-Convert meldet die Tiefe -> abgefangen -> gültige Folge-emit.
        let result = m.call(&id, "handle", &ctx).unwrap();
        assert_eq!(result.requests.len(), 1);
        match &result.requests[0] {
            RealmRequest::Emit { name, .. } => assert_eq!(name, "abgefangen"),
            other => panic!("unerwartete Anfrage: {}", other.describe()),
        }
    }

    #[test]
    fn domains_distinguishable() {
        let mut m = ScriptManager::new().unwrap();
        for (i, domain) in ScriptDomain::ALL.iter().enumerate() {
            let id = m
                .load_script(*domain, code_source(format!("d{i}"), minimal_returns("")))
                .unwrap();
            assert_eq!(id.domain, *domain);
            assert_eq!(m.script(&id).unwrap().id.domain, *domain);
        }
        assert_eq!(m.script_count(), 8);
    }

    #[test]
    fn dispatch_runs_handlers_isolated() {
        let mut m = ScriptManager::new().unwrap();
        let good = m
            .load_script(
                ScriptDomain::Npc,
                code_source(
                    "gut",
                    "return { hail = function(c) andora.emit('hail_ok', { n = c.npc_id }) end }",
                ),
            )
            .unwrap();
        let bad = m
            .load_script(
                ScriptDomain::Npc,
                code_source(
                    "schlecht",
                    "return { hail = function() error('hail-fehler') end }",
                ),
            )
            .unwrap();
        m.register_event_handler(ScriptDomain::Npc, "hail", good.clone());
        m.register_event_handler(ScriptDomain::Npc, "hail", bad.clone());

        let event = ScriptEvent::new(
            "hail",
            ScriptContext {
                npc_id: Some("npc-415".into()),
                ..Default::default()
            },
        );
        let results = m.dispatch_event(ScriptDomain::Npc, &event);
        assert_eq!(results.len(), 2);
        // gut zuerst: ok; schlecht: isolierter Fehler
        let first = results[0].as_ref().unwrap();
        assert_eq!(first.script_id, good);
        assert_eq!(
            first.requests[0],
            RealmRequest::Emit {
                name: "hail_ok".into(),
                payload: serde_json::json!({ "n": "npc-415" }),
            }
        );
        assert!(matches!(results[1], Err(LuaError::RuntimeError { .. })));
    }

    #[test]
    fn dispatch_unknown_event_is_empty() {
        let mut m = ScriptManager::new().unwrap();
        let id = m
            .load_script(ScriptDomain::Npc, code_source("gut", minimal_returns("")))
            .unwrap();
        m.register_event_handler(ScriptDomain::Npc, "hail", id);
        let event = ScriptEvent::new("kein_event", ScriptContext::new());
        assert!(m.dispatch_event(ScriptDomain::Npc, &event).is_empty());
    }

    #[test]
    fn load_dir_collects_errors_without_abort() {
        let root = tmp_root("dir");
        fs::create_dir_all(root.join("quests")).unwrap();
        fs::create_dir_all(root.join("npcs")).unwrap();
        fs::write(root.join("quests/a.lua"), "return {}").unwrap();
        fs::write(root.join("quests/kaputt.lua"), "syntax ((( ").unwrap();
        fs::write(root.join("npcs/b.lua"), "return {}").unwrap();

        let mut m = ScriptManager::new().unwrap();
        let (ids, errors) = m.load_script_dir(&root);
        assert_eq!(ids.len(), 2);
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], LuaError::SyntaxError { .. }));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn load_dir_ignores_missing_folders() {
        let root = tmp_root("leer");
        fs::create_dir_all(&root).unwrap();
        let mut m = ScriptManager::new().unwrap();
        let (ids, errors) = m.load_script_dir(&root);
        assert!(ids.is_empty());
        assert!(errors.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn script_ids_iterator_lists_all() {
        let mut m = ScriptManager::new().unwrap();
        let id = m
            .load_script(ScriptDomain::Cutscene, code_source("intro", "return {}"))
            .unwrap();
        assert!(m.script_ids().any(|i| *i == id));
    }
}
