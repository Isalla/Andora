// error — Fehlertypen der Lua-Content-Schicht (docs/Lua-Scripting-System.md §7, §18).
//
// Grundprinzip Fehlerisolation: Ein Fehler in einem Script darf weder den
// Realm zum Absturz bringen noch die Verarbeitung anderer Scripts oder
// Threads beeinträchtigen. Alle Fehler werden als strukturierte LuaError-
// Werte an den Realmaufrufer zurückgegeben; ein Script kann sich niemals
// außerhalb seiner eigenen Ausführung „abstürzen“.
//
// Keine Verwendung von anyhow/thiserror (Realm-Konvention):
//	- LuaError ist der einzige hier verwendete Fehlertyp.
//	- Rachlanwender-Seite über matches!() bzw. variantenbezogene Muster.

use std::fmt;

use super::domain::ScriptDomain;

/// Hergestellte, strukturierte Fehler der Lua-Content-Schicht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LuaError {
    /// Angefragtes Script ist nicht geladen.
    ScriptNotFound {
        /// Betroffene Domäne.
        domain: ScriptDomain,
        /// Betroffener Script-Name (Dateipfad-Inneres oder Name).
        name: String,
    },
    /// Script konnte nicht geladen werden (ungültige Quelle, fehlender
    /// Ordner usw.).
    LoadError {
        /// Betroffene Domäne.
        domain: ScriptDomain,
        /// Betroffener Script-Name.
        name: String,
        /// Detailnachricht.
        message: String,
    },
    /// Syntaxfehler beim Kompilieren (Laden) des Scripts.
    SyntaxError {
        /// Betroffene Domäne.
        domain: ScriptDomain,
        /// Betroffener Script-Name.
        name: String,
        /// Detailnachricht des Lua-Compilers.
        message: String,
    },
    /// Laufzeitfehler während der Script-Ausführung (oder beim Aufruf eines
    /// Callbacks). Isoliert: nur dieses Script / dieser Aufruf schlägt fehl.
    RuntimeError {
        /// Betroffene Domäne.
        domain: ScriptDomain,
        /// Betroffener Script-Name.
        name: String,
        /// Detailnachricht.
        message: String,
    },
    /// Script ist geladen, hat aber nicht die erwartete Form
    /// (z. B. liefert kein Callback-Tabelle zurück).
    InvalidScript {
        /// Betroffene Domäne.
        domain: ScriptDomain,
        /// Betroffener Script-Name.
        name: String,
        /// Detailnachricht.
        message: String,
    },
    /// Der angeforderte Callback existiert nicht in der Callback-Tabelle.
    UnknownCallback {
        /// Betroffene Domäne.
        domain: ScriptDomain,
        /// Betroffener Script-Name.
        name: String,
        /// Gesuchte Callback-Benennung.
        callback: String,
    },
    /// Sandbox-Verstoß durch ein Script (host-API mit unzulässigen
    /// Inhalten, z. B. nicht-serialisierbare Werte im Payload).
    SandboxViolation {
        /// Detailnachricht.
        message: String,
    },
    /// Abgelehnter Host-API-Aufruf (Rust-Validierung, z. B. ungültiger
    /// Name, übermäßige Verschachtelungstiefe).
    HostError {
        /// Detailnachricht.
        message: String,
    },
    /// Interner Fehler der Lua-Content-Schicht (z. B. VM-/Table-Problem),
    /// nicht durch das Script verursacht.
    Internal {
        /// Detailnachricht.
        message: String,
    },
}

impl LuaError {
    /// Für Diagnose-/Logmeldungen: fester Präfix je Variante.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ScriptNotFound { .. } => "SCRIPT_NOT_FOUND",
            Self::LoadError { .. } => "LOAD_ERROR",
            Self::SyntaxError { .. } => "SYNTAX_ERROR",
            Self::RuntimeError { .. } => "RUNTIME_ERROR",
            Self::InvalidScript { .. } => "INVALID_SCRIPT",
            Self::UnknownCallback { .. } => "UNKNOWN_CALLBACK",
            Self::SandboxViolation { .. } => "SANDBOX_VIOLATION",
            Self::HostError { .. } => "HOST_ERROR",
            Self::Internal { .. } => "INTERNAL_ERROR",
        }
    }

    /// Kurztext, der zu den Bemerkungen (message) gehört.
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::ScriptNotFound { name, .. } => Some(name),
            Self::LoadError { message, .. }
            | Self::SyntaxError { message, .. }
            | Self::RuntimeError { message, .. }
            | Self::InvalidScript { message, .. }
            | Self::SandboxViolation { message }
            | Self::HostError { message }
            | Self::Internal { message } => Some(message),
            Self::UnknownCallback { callback, .. } => Some(callback),
        }
    }

    /// Komforthelfer für interne Fehler.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Komforthelfer für Host-API-Ablehnungen.
    pub fn host(message: impl Into<String>) -> Self {
        Self::HostError {
            message: message.into(),
        }
    }
}

impl fmt::Display for LuaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScriptNotFound { domain, name } => {
                write!(f, "Script nicht gefunden: {domain} '{name}'")
            }
            Self::LoadError {
                domain,
                name,
                message,
            }
            | Self::SyntaxError {
                domain,
                name,
                message,
            }
            | Self::RuntimeError {
                domain,
                name,
                message,
            }
            | Self::InvalidScript {
                domain,
                name,
                message,
            } => write!(f, "{} ({domain} '{name}'): {message}", self.code()),
            Self::UnknownCallback {
                domain,
                name,
                callback,
            } => write!(f, "Unbekannter Callback '{callback}' ({domain} '{name}')"),
            Self::SandboxViolation { message } => {
                write!(f, "SANDBOX_VIOLATION: {message}")
            }
            Self::HostError { message } => write!(f, "HOST_ERROR: {message}"),
            Self::Internal { message } => write!(f, "INTERNAL_ERROR: {message}"),
        }
    }
}

impl std::error::Error for LuaError {}

/// Ein vom Rust-Host (host.rs) abgelehnter Host-API-Aufruf.
///
/// Wird als [`mlua::Error::external`]-Ursache in den Lua-Stack gereicht und
/// von der Runtime über [`Error::downcast_ref`] wieder in einen
/// [`LuaError::HostError`] übersetzt — dadurch werden Host-Ablehnungen vom
/// Script klar von echten Lua-Laufzeitfehlern unterschieden.
#[derive(Debug)]
pub struct HostRejected(pub String);

impl fmt::Display for HostRejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "andora.host: {}", self.0)
    }
}

impl std::error::Error for HostRejected {}

/// Ein vom Sandbox-/Serialisierungs-Check abgelehnter Payload-Wert.
#[derive(Debug)]
pub struct SandboxRejected(pub String);

impl fmt::Display for SandboxRejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "andora.sandbox: {}", self.0)
    }
}

impl std::error::Error for SandboxRejected {}
