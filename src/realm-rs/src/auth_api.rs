// auth_api — Signierter Auth-API-Client des Realm-Servers.
// Der Realm besitzt KEINE AUTH_DB-Zugangsdaten. Alle Auth-Funktionen
// (Handoff-/Session-Validierung, Elternkontrolle) laufen über die
// Auth-API des Go-Dienstes mit der EIGENEN Service-Credential.
// Signatur wie src/api/auth.go:
//   hex(HMAC-SHA256(secret, METHOD\nPATH\nRAW_QUERY\nTIMESTAMP\nSHA256(BODY)))
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::config::AuthApiConfig;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct HandoffValidation {
    pub valid: bool,
    #[serde(default)]
    pub account_id: Option<u32>,
    #[serde(default)]
    pub realm_id: Option<u32>,
}

/// Session-Prüfung: Teil der fail-closed Einstiegsprüfung (verify_entry)
/// — gilt nur, wenn die Session dem Handoff-Account gehört.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SessionValidation {
    pub valid: bool,
    #[serde(default)]
    pub account_id: Option<u32>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ParentalStatus {
    pub enabled: bool,
    #[serde(default)]
    pub remaining_seconds: i64,
    #[serde(default)]
    pub blocked: bool,
    #[serde(default)]
    pub buffer_until: Option<String>,
    #[serde(default)]
    pub force_logout: bool,
    #[serde(default = "yes")]
    pub chat_allowed: bool,
    #[serde(default = "yes")]
    pub voice_allowed: bool,
    #[serde(default)]
    pub warning: bool,
    #[serde(default)]
    pub extended_used_today: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PinVerify {
    pub valid: bool,
}

/// Antwortform der Auth-API (vollständig abgebildet, auch wenn einzelne
/// Felder erst künftig ausgewertet werden).
#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExtensionResult {
    pub granted: bool,
    #[serde(default)]
    pub remaining_seconds: i64,
    #[serde(default)]
    pub extended_used_today: bool,
}

#[derive(Debug)]
pub struct AuthApiError {
    pub status: Option<u16>,
    pub body: String,
}

impl std::fmt::Display for AuthApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(s) => write!(f, "authapi {s}: {}", self.body),
            None => write!(f, "authapi unreachable: {}", self.body),
        }
    }
}

impl AuthApiError {
    pub fn status(&self) -> Option<u16> {
        self.status
    }
}

#[derive(Debug, Clone)]
pub struct AuthApi {
    cfg: AuthApiConfig,
    client: reqwest::Client,
}

impl AuthApi {
    pub fn new(cfg: &AuthApiConfig) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("authapi http-client: {e}"))?;
        Ok(Self { cfg: cfg.clone(), client })
    }

    pub fn enabled(&self) -> bool {
        !self.cfg.url.is_empty()
    }

    fn sign(&self, method: &str, path: &str, ts: i64, body: &str) -> String {
        use sha2::Digest;
        let body_hash = hex::encode(Sha256::digest(body.as_bytes()));
        let payload = format!("{method}\n{path}\n\n{ts}\n{body_hash}");
        let mut mac = Hmac::<Sha256>::new_from_slice(self.cfg.secret.as_bytes())
            .expect("hmac key");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        data: serde_json::Value,
    ) -> Result<T, AuthApiError> {
        let body = serde_json::to_string(&data).unwrap_or_else(|_| "{}".into());
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let sig = self.sign("POST", path, ts, &body);
        let resp = self
            .client
            .post(format!("{}{}", self.cfg.url, path))
            .header("Content-Type", "application/json")
            .header("X-Andora-Service", &self.cfg.service_id)
            .header("X-Andora-Timestamp", ts.to_string())
            .header("X-Andora-Signature", sig)
            .body(body)
            .send()
            .await
            .map_err(|e| AuthApiError { status: None, body: e.to_string() })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if status != 200 && status != 201 {
            return Err(AuthApiError { status: Some(status), body: text });
        }
        serde_json::from_str(&text).map_err(|e| AuthApiError {
            status: Some(status),
            body: format!("invalid JSON from {path}: {e}"),
        })
    }

    /// Handoff validieren UND verbrauchen (einmalig; Zweitaufruf ungültig).
    pub async fn validate_handoff(&self, token: &str) -> Result<HandoffValidation, AuthApiError> {
        self.post("/handoff/validate", serde_json::json!({"handoff_token": token})).await
    }

    pub async fn validate_session(&self, session_id: &str) -> Result<SessionValidation, AuthApiError> {
        self.post("/session/validate", serde_json::json!({"session_id": session_id})).await
    }

    pub async fn parental_status(
        &self,
        account_id: u32,
        session_id: Option<&str>,
    ) -> Result<ParentalStatus, AuthApiError> {
        self.post(
            "/parental/status",
            serde_json::json!({"account_id": account_id, "session_id": session_id.unwrap_or("")}),
        )
        .await
    }

    pub async fn verify_pin(&self, account_id: u32, pin: &str) -> Result<PinVerify, AuthApiError> {
        self.post(
            "/parental/pin/verify",
            serde_json::json!({"account_id": account_id, "parent_pin": pin}),
        )
        .await
    }

    pub async fn use_extension(
        &self,
        account_id: u32,
        pin: &str,
    ) -> Result<ExtensionResult, AuthApiError> {
        self.post(
            "/parental/extension",
            serde_json::json!({"account_id": account_id, "parent_pin": pin}),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api() -> AuthApi {
        AuthApi {
            cfg: AuthApiConfig {
                url: "http://127.0.0.1:8080".into(),
                service_id: "realm-de1-service".into(),
                secret: "s3cret".into(),
            },
            client: reqwest::Client::new(),
        }
    }

    #[test]
    fn signature_matches_go_scheme() {
        // Unabhängig mit python3/hmac gerechnet (Schlüssel b"s3cret"):
        //   body_sha256({"session_id":"abc"})
        //     = a680f66cf072a541ac5fb1513c09b59a5ae4b4587226a3c436279577e0257d67
        //   HMAC(payload "POST\n/session/validate\n\n1753286400\n<body_sha256>")
        //     = f2c36706b9eeb7e73643c7f18de03eb4b7fe76982f7fc22134376b10cd377c75
        // Gleiches Schema wie src/api/auth.go (signPayload).
        let a = api();
        let sig = a.sign("POST", "/session/validate", 1753286400, r#"{"session_id":"abc"}"#);
        assert_eq!(
            sig,
            "f2c36706b9eeb7e73643c7f18de03eb4b7fe76982f7fc22134376b10cd377c75"
        );
    }
}
