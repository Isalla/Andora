# ANDORA – API SECURITY AUDIT, PHASE 1 (read-only)

**Stand:** Read-only-Analyse against current HEAD (`18c63d9 Implement Weather V1 and seasonal system`). No Code changed, no Commit/Push, no Migration executed. `references/` untouched (gitignored). Godot-Testclient not in scope.

**Method:** Only Befunde verified in the actual Quellcode with File, Function and Line-Range are documented. Existing protections are explicitly acknowledged. Design observations are marked as such and not presented as confirmed errors.

---

## 0. Executive Summary (priorisiert)

| # | Class | Finding | Location |
|---|---|---|---|
| H-1 | HOCH | TOTP replay race: replay-check is a pre-read, `SetLastTOTP` unconditional (TOCTOU) | `src/api/handlers.go:154`, `src/api/store.go:1192-1199` |
| H-2 | HOCH (FIXED) | Rate-Limit-Key embeds attacker-controlled Path → per-IP limit bypassable via unique paths, `rl.hits` grows within sweep window | `src/api/auth.go:115,182-220`, `src/api/server.go:153`, `src/login/server.go:59` |
| H-3 | HOCH | Realm health/internal-mail listener defaults to public bind (`0.0.0.0`); trust boundary is HMAC only | `src/realm-rs/src/config.rs:96-102`, `:694-696` |
| M-1 | MITTEL | HMAC signature compare in Go `authorize()` not constant-time (`hmacEqual` exists, unused there) | `src/api/auth.go:107`, `src/api/crypto.go:125-127` |
| M-2 | MITTEL | `security_events` unbounded: `ListSecurityEvents` without LIMIT, no retention (cf. `docs/Security.md` P-33) | `src/api/store.go:1830-1847` |
| M-3 | MITTEL | `MAIL_PAYLOAD_MAX=4096` defined but never enforced; notification payload forwarded unbounded | `src/api/mail.go:40`, `src/api/mail_handlers.go:189-209` |
| M-4 | MITTEL | `RATE_LIMIT_PER_MIN` never enforced (only burst; perMin only feeds Retry-After) | `src/api/auth.go:202-216` |
| M-5 | MITTEL | Service secrets: only non-empty check, no length/entropy floor | `src/api/config.go:202`, `src/realm-rs/src/config.rs:701-714` |
| M-6 | MITTEL | Login service forwards entire client JSON map to `/auth/verify` without field whitelist | `src/login/handlers.go:34-50` |
| M-7 | MITTEL | Session token in GET query string (`/realms?session_id=`) – proxy/access-log leak | `src/login/handlers.go:88` |
| M-8 | MITTEL | Unsalted SHA-256 e-mail lookup hash – dictionary-recoverable at DB compromise, linkage oracle | `src/api/crypto.go:27-43` |
| M-9 | MITTEL | Error oracles: `unknown service` vs `bad signature`; `account not found` 404 | `src/api/auth.go:82,87,108`, `src/api/endpoints.go:747,799,883,1179` |
| M-10 | MITTEL | `mysqlDSN` interpolates User/Password into DSN string | `src/api/store.go:316-319` |
| N-1 | NEDRIG | TOTP code compare `got == expected` not constant-time | `src/api/totp.go:90` |
| N-2 | NEDRIG | `RECOVERY_CALLBACK_HOST` configured but never used (dead config) | `src/api/config.go:72,233` |
| N-3 | NEDRIG | LEGACY world endpoints still routed (permission-gated, documented) | `src/api/server.go:185-186`, `src/api/endpoints.go:1250-1319` |
| N-4 | NEDRIG | `/status` open without service auth – aggregates only (documented) | `src/api/endpoints.go:630-638` |
| N-5 | NEDRIG | Parental PIN may be 4 digits (10⁴ space); Argon2-verified, service-gated | `src/api/parental_handlers.go:241`, `:116-121` |
| N-6 | NEDRIG | `clientIP` returns unvalidated hop string as rate-limit key (only via trusted proxy) | `src/api/iputil.go:93` |

No KRITISCH finding confirmed in the audited code.

---

## 1. Authentifizierung, Autorisierung, Berechtigungsgrenzen

### 1.1 Confirmed protections
- `authorize()` (`src/api/auth.go:79-122`): closed permission list (`auth.go:31-62`), per-service credentials, canonical payload `METHOD\nPATH\nRAW_QUERY\nTS\nSHA256(BODY)` (`auth.go:126-132`), ±90 s timestamp window (`auth.go:24`, `:98`), permission check before handler (`auth.go:111-114`).
- Every registered route (`src/api/server.go:166-219`) is served by a handler that calls `s.authorize(...)` with a specific permission constant – verified across `endpoints.go`, `handlers.go`, `twofactor.go`, `mail_handlers.go` (service endpoints), `parental_handlers.go`. Only `/health` and `/status` are open by design (aggregates only, `endpoints.go:67-79`).
- Realm verifies the Go-API mail forward with its own `MAIL_INBOUND_SERVICE_SECRET` and constant-time `hmac_verify` (`src/realm-rs/src/health.rs:152-179`, `:308-317`); empty service_id rejected (`health.rs:156`).
- Login- and Realm services hold no `AUTH_DB_*` credentials (`src/login/config.go`, `src/realm-rs/src/config.rs`); matches `docs/Auth_API_Architektur.md` §3/§9.
- Handoff `realm_id` binding enforced on the Realm side: `src/realm-rs/src/handlers.rs:92` `if !h.valid || h.realm_id != Some(realm_id)` – a handoff minted for another realm is rejected.
- Realm client signs with its own secret (`src/realm-rs/src/auth_api.rs:172-179`, `:196-198`).

### 1.2 Findings
- **M-1 (CONFIRMED):** `auth.go:107` compares the presented signature with `!=` (short-circuit). `hmacEqual` (`crypto.go:125-127`) exists but is not used in `authorize()`. The Realm side does it correctly (`health.rs:308-317`). Practical impact of timing attacks on MAC verification is limited, but constant-time compare is the standard requirement.
- **M-9 (CONFIRMED):** Distinct error strings `missing X-Andora-Service` / `unknown service` / `bad signature` (`auth.go:82,87,108`) allow unauthenticated callers to enumerate valid service IDs. `404 "account not found"` (`endpoints.go:747,799,883,1179`) is an account-existence oracle – only reachable by permission-carrying services (realm credentials do not carry `account.*` permissions per `docs/Auth_API_Architektur.md` §10).
- `/auth/verify` failure shapes are deliberately indistinguishable (`handlers.go:96-103` `fail()`), ban/password/missing-account – acknowledged as correct.

---

## 2. Go-API ↔ Realm communication, HMAC, Replay, Service-Identitäten

### 2.1 Confirmed protections
- Identical canonical payload convention on all three sides (Go `auth.go:126-132`; Rust client `auth_api.rs:172-179`; Rust verifier `health.rs:171-177`); cross-check vector with independent python3/hmac values in `auth_api.rs:336-354` (test `signature_matches_go_scheme`).
- Replay windows: 90 s (Go `auth.go:24`), 300 s (Realm inbound `health.rs:167`). Timestamp and body-hash are inside the HMAC payload.
- Handoff single-use: `ValidateHandoff` (`store.go:1029-1046`, `WHERE used_at IS NULL`) + conditional `UseHandoff` (`store.go:1050-1062`, `UPDATE ... WHERE used_at IS NULL` + `RowsAffected`→`ErrNoRows`); handler maps a losing race to `valid:false` (`endpoints.go:1220-1227`). Test `TestHandoffSingleUse` (`api_test.go:409`).
- Revocation poller (`src/realm-rs/src/session_watch.rs`): interval 10 s (`:32`), batch ≤250 (`:36`), four-way status parse without guessing (`auth_api.rs:116-125`, unknown → `Missing`), correlation via `index` (`auth_api.rs:264-284` – results placed by index, misordered responses cannot misattribute), stale-safety re-check under World-Lock incl. exact `session_id` equality before `close_conn` (`:146-158`), `Valid/Expired` never disconnect (AUTH-02a, `:127-132`), logs only counters (`:164-165`, `:271-281`).
- Auth-API unreachable → poller keeps connections: documented, intentional availability trade-off (`session_watch.rs:228-240`; `docs/Auth_API_Architektur.md` "Ausfallverhalten") – not counted as defect.

### 2.2 Findings
- **H-3 (CONFIRMED, design observation):** `bind_addrs("")` defaults to `0.0.0.0` (`config.rs:96-102`), `HEALTH_BIND_HOST` default leer (`config.rs:695-696`) → the Realm's health + `/internal/mail/*` listener binds to all interfaces unless the operator configures it. Protection relies solely on the HMAC. Recommendation: loopback default or explicit operator decision.
- **Observation:** `MAIL_FORWARD_URL` is operator config; `mail.go:156-188` builds `base+path` with fixed paths – no attacker-controlled SSRF vector found.
- **Observation (integration gap, fail-closed):** Go wrapper relays `/mail/send|quota|receive|notification`, but the Realm's internal handler routes only `list/read/claim` (`health.rs:182-190`, else 404) → those forwards are inert in V1. Not a security defect.

---

## 3. Mail V1: Send, Read, Claim, Escrow, Idempotenz, Ownership

### 3.1 Confirmed protections (Realm = sole authority)
- Go wrapper never touches `realm_state` (`mail.go:14-31`; no DB config there). Player endpoints authenticate via session possession; `account_id` is re-derived server-side and overwrites the body value (`mail_handlers.go:64,87,110` `req.ACCOUNT_ID = acc`); client-supplied `account_id`/`sender_account_id` never trusted.
- Ownership enforced twice: Go `requirePlayerSession` (`mail_handlers.go:225-246`) and Realm `mail_owner_verdict(body_account_id, owner_account_id)` against DB `characters.account_id` (`health.rs:211-233`, `mail.rs:240-242`); cross-account → 403 `not_owner`.
- Claim chain: deterministic commit id `mc{SHA256("mc:{char}:{mail}")[..40]}` (`mail.rs:180-189`), one pending claim per character (`mail.rs:605-613`), durable spool receipt (`mail.rs:554-561`), in-process receipt (`:563-580`), revision content-proof (`AlreadyApplied` only for identical payload, `:343-366`), conditional `DELETE` + `RowsAffected==0 → Conflict` rollback (`:428-431`), DB `PRIMARY KEY (commit_id)` + `UNIQUE (character_id, mail_id)` (`src/realm-rs/migrations/026_mail_v1.sql:73-74`), atomic escrow TX (mail row + `character_mail_items` + Idia credit, `mail.rs:254-324`, commit `:321`), double-refund idempotency via `refund_key` SELECT + `UNIQUE (refund_key)` (`mail.rs:268-279`, `026_mail_v1.sql:35`).
- Read/unread orthogonal to claim (`mail.rs:415-418`); list/read/claim queries scoped by `character_id` (`mail.rs:740-745,768-776,693-697`).

### 3.2 Findings
- **M-3 (CONFIRMED):** `MAIL_PAYLOAD_MAX = 4096` (`mail.go:40`) is defined but referenced nowhere; `handleMailNotification` (`mail_handlers.go:189-209`) validates `event_type` only – `PAYLOAD` is forwarded unbounded (up to the 64 KiB authorize limit).
- **Observation:** `readPlayerBody` allows 1 MiB (`mail_handlers.go:215`) for player endpoints vs 64 KiB for service endpoints – intentional (attachments), inconsistency worth noting.

---

## 4. Inputvalidierung, Rate-Limits, Missbrauch

### 4.1 Confirmed protections
- All SQL uses `?`-parameter binding; the only interpolated column name is whitelisted (`store.go:516-528`: `column != "username" && column != "email_lookup_hash"` → error). No attacker input reaches SQL text. CONFIRMED.
- Body limits: 64 KiB (`auth.go:21`, `authorize` `:102`), batch pre-check 32 KiB before auth (`endpoints.go:1075-1079`), 250-entry fail-closed 413 (`endpoints.go:1093-1099`), token format `isHexToken` 64 hex (`endpoints.go:1102-1105`, `store.go:1862-1868`) checked before hashing/DB.
- Username/email/password validation (`endpoints.go:18-57`), mail-send validation incl. recipient dedup and attachment type/size (`mail.go:81-137`).
- TrustedProxies fail-closed (`iputil.go:11-16,69-100`); X-Forwarded-For only honored if direct peer is trusted.
- TLS: MinVersion 1.2 (`server.go:232`), cert/key pair enforced (`server.go:81-83`), `ReadHeaderTimeout` 15 s (`server.go:23-27`, `:274`), no-cache headers (`server.go:299-308`).
- Realm anti-manipulation V1 (`security.rs`, acknowledged): frame-size gate before parse, C2S type whitelist (`:355-384`), session gate with violation counter → controlled Disconnect (`:317-339`), per-class 1 s sliding-window rate limits (`:256-271`), server-authoritative attribute spend allowlist (`:388-398`), server-side gold for auction buy (`:514-538`), log-injection defense `reject_field` (`:152-169`).

### 4.2 Findings
- **H-2 (CONFIRMED):** `ratePreAuth` key = `"ip:" + clientIP + ":" + r.URL.Path` (`server.go:153`), post-auth key = `"svc:" + cred.ID + ":" + r.URL.Path` (`auth.go:115`). `r.URL.Path` is attacker-controlled and unbounded: every unique path creates a new limiter entry that passes once; the sweep (`auth.go:186-199`) only removes keys older than 180 s. Consequences: (a) the per-IP burst limit is bypassable for unknown paths (they 404 at the mux, but the limiter never binds them), (b) `rl.hits` grows with the number of distinct paths per ~3 min → memory pressure. Same pattern in the login service (`src/login/server.go:59`).
- **M-4 (CONFIRMED):** `RateLimitPerMin` is never enforced: `allow()` (`auth.go:182-220`) rejects only at `len(ts) >= burst`; `perMin` only computes `Retry-After` (`:204-215`). The documented "burst + sliding one-minute window" (`auth.go:164-170`) is not implemented as such.
- **M-6 (CONFIRMED):** `handleLogin` (`src/login/handlers.go:34-50`) forwards the whole client map to `/auth/verify`; unknown/client fields reach the Auth-API request. Impact limited (Auth-API decodes into a typed struct, ignores unknown fields) – still a pass-through against the whitelist pattern used for `/parental` (`handlers.go:274-334`).
- **M-7 (CONFIRMED):** `GET /realms?session_id=...` (`src/login/handlers.go:88`) puts the session token into the query string (POST variant exists, `:89-101`). Query strings are commonly logged by upstream proxies.
- **M-10 (CONFIRMED):** `mysqlDSN` (`store.go:316-319`) embeds `User`/`Password` into the DSN string; passwords containing `:`, `@`, `%` or `(` can break or shift DSN parsing (operational risk).
- **Observation (documented, fail-safe):** `authorize()` uses `io.LimitReader` → oversized bodies are truncated, not rejected (`auth.go:102`; documented `endpoints.go:1031-1035`); handlers answer 400, no partial processing.
- **N-6 (CONFIRMED):** `clientIP` returns the unvalidated hop string as rate-limit key (`iputil.go:93`) – only reachable when the direct peer is a trusted proxy.

---

## 5. Database access, Transaction boundaries, Error handling

### 5.1 Confirmed protections
- Startup fail-closed: migrations must succeed or the service aborts (`server.go:77-95`, `store.go:356-436`); DSN carries connect/read/write timeouts 5 s (`store.go:317`); pool bounded (`store.go:329-331`).
- Atomic compounds in one transaction with full rollback on every error branch: `RecoverPassword` (`store.go:1107-1160`), 2FA setup/enable/disable/reset incl. CAS on the encrypted secret (`store.go:1329-1533`), `ChangePasswordRevokeAll` (`store.go:1533-1566`), `UseRecoveryCode` (`store.go:1770-1800`), `RevokeTrustedDevice` – per `docs/Auth_API_Architektur.md` §18 / P-34 (structure verified).
- Conditional idempotent markers: `UPDATE sessions SET revoked_at = NOW() WHERE ... AND revoked_at IS NULL` (`store.go:766,1304`), recovery-token consumption `WHERE used_at IS NULL` (`store.go:1139`).
- Batch session lookup: single `IN(...)` query, no N+1 (`store.go:699-755`), result correlation via hash-index (`:737-742`).
- Monitoring P-33: read-only aggregates, admission control + cancellation + bounded grace (`server.go:107-147`, `endpoints.go:174-272`), metadata errors classified, never raw driver text (`store.go:884-911`).
- Login compensation: `TouchLogin` failure revokes the just-created session (`handlers.go:211-214`).

### 5.2 Findings
- **H-1 (CONFIRMED):** TOTP replay rule is enforced by a pre-read (`handlers.go:154` `counter <= *acc.LastTOTPCounter`) plus an unconditional write `SetLastTOTP` (`store.go:1192-1199`, plain `UPDATE accounts SET last_totp_counter = ?`). Two concurrent `/auth/verify` requests presenting the same valid TOTP step both read the old counter, both pass, both create sessions. Sequential replay is rejected (tested, `twofactor_test.go:211`), concurrent replay is not. Fix direction: conditional `UPDATE ... WHERE last_totp_counter IS NULL OR last_totp_counter < ?` + `RowsAffected` check (mirrors the existing `UseHandoff` pattern).
- **M-2 (CONFIRMED):** `ListSecurityEvents` (`store.go:1830-1847`) has no `LIMIT`; `security_events` has no retention/cleanup (documented `docs/Security.md` P-33 `BESTÄTIGT`). Response and table grow unbounded per account.
- **Observation:** several handlers use `context.Background()` instead of `r.Context()` (`twofactor.go:117,146,178,212,254`, `endpoints.go:677,682,915,1140,1172,1211,1220`) – DB work is not cancelled on client disconnect; bounded by the 5 s DSN timeouts, impact limited. Worth aligning.
- **Observation:** `isDuplicateKey` matches error text (`store.go:441-447`) – brittle but fail-safe direction (worst case 500 instead of 409).

---

## 6. Secrets, Konfiguration, Logging, Information leaks

### 6.1 Confirmed protections
- `ENCRYPTION_KEY` required (`server.go:78-80`), 32-byte hex enforced at use (`crypto.go:52-56,78-82`); AES-256-GCM with fresh random 96-bit nonce per record (`crypto.go:65-74`); e-mail and TOTP secret encrypted at rest (`db.go:13-15`).
- Tokens (session/handoff/recovery/device) stored only as SHA-256 (`store.go:1858-1860`), generated from 32 CSPRNG bytes (`crypto.go:19-25`); raw token appears exactly once in a response, never in logs.
- Passwords: Argon2id, fresh random 16-byte salt per account (`endpoints.go:708-716`, `password.go:23-36`), self-describing parameters, constant-time verify (`password.go:72`); recovery codes and device tokens hashed (SHA-256), single-use conditional consumption (`store.go:300-302,1770-1800`).
- Access log: method + escaped/bounded path + status + duration only – no headers, no query, no body (`auth.go:222-277`, `:347-355`; login service `server.go:248`); status recording fixed so rejections are logged as rejections (`auth.go:278-345`).
- Security-event failure lines structured, free of token/secret/IP (`store.go:1212-1231`, `:1233-1241`; tested per P-34/P-35).
- Realm logging: counters only in poller (`session_watch.rs:164-165,271-281`), reject/takeover lines sanitized (`security.rs:152-185,214-232`), no session id/token/IP.
- `config.env.example` contains placeholders only; real `config.env` gitignored; secrets per service, no shared universal secret (`config.go:194-216`).

### 6.2 Findings
- **M-5 (CONFIRMED):** no minimum secret length anywhere: Go `config.go:202` (non-empty only), Realm `config.rs:701-714` (none), login `config.go` (non-empty only). A weak service secret is not detected at startup.
- **M-8 (CONFIRMED):** unsalted, single-round SHA-256 e-mail lookup hash (`crypto.go:27-43`, normalize = lower+trim): at DB compromise, common e-mail addresses are dictionary-recoverable; identical e-mails are linkable across accounts. The documented "non-reversible" property holds only against arbitrary pre-images, not against a wordlist attack.
- **N-2 (CONFIRMED):** `RECOVERY_CALLBACK_HOST` (`config.go:72,233`) is never read by any code – dead configuration.
- **N-4 (CONFIRMED):** `/status` is open and publishes session-count aggregates (`endpoints.go:630-638`) – documented as intentional (`endpoints.go:67-79`).
- **N-1 (CONFIRMED):** `evalTOTPCode` compares codes with `==` (`totp.go:90`); input restricted to exactly 6 digits (`totp.go:97-110`).

---

## 7. Tests and coverage gaps

### 7.1 Existing test base (counted in source)
- Go Auth-API: `api_test.go` 70 `func Test*`, `twofactor_test.go` 34, `parental_test.go` 20, `mail_test.go` 6, plus `verify_test.go`, `trusted_expiry_test.go`, `ipnet_test.go`, `fakestore_test.go`. Login service: `login_test.go` 17, `ipnet_test.go`.
- Realm: 633+ Rust tests (per `docs/Security.md` P-18 closure), incl. signature cross-check (`auth_api.rs:336`), poller stale/takeover tests (`session_watch.rs:672-721`), escrow/idempotency tests (`mail.rs`), anti-manipulation tests (`security.rs`).
- Covered by tests (acknowledged): bad signature / unknown service (`api_test.go:119,133`), permission denial (`:368`), handoff single-use + expiry (`:409,:437`), pre-auth rate limit (`:486`), session revoked/expired/missing classification (`:573-698`), sequential TOTP replay (`twofactor_test.go:211`), recovery-code replay (`:357`), oversize batch (`api_test.go:1005-1031`).

### 7.2 Coverage gaps (CONFIRMED absence in test files)
1. No test for concurrent TOTP replay (H-1 race) – only sequential replay tested.
2. No test that `RATE_LIMIT_PER_MIN` has any effect (M-4) – and no implementation.
3. No test for rate-limit key growth via unique paths (H-2).
4. No test for notification payload size (M-3).
5. No MariaDB integration test for the Go store (tests run on FakeStore; documented as limit in `docs/Auth_API_Architektur.md` P-34 closure and `docs/Security.md`).
6. No retention/cleanup test for `security_events` (M-2; feature not implemented).

---

## 8. Recommended corrections (NOT implemented – for follow-up work orders)

Priority order:
1. **H-1:** make TOTP consumption atomic: conditional `UPDATE ... WHERE last_totp_counter IS NULL OR last_totp_counter < ?` + `RowsAffected` check in `SetLastTOTP` (`store.go:1192-1199`); handler treats `ErrNoRows` as `two_factor_required` fail shape.
2. **H-2:** normalize the rate-limit key: use the registered route (mux key) or a fixed fallback key instead of the raw `r.URL.Path`; cap distinct keys.
3. **H-3:** bind the Realm health/internal-mail listener to loopback by default or require an explicit operator decision.
4. **M-1:** use `hmacEqual` (`crypto.go:125-127`) in `authorize()` (`auth.go:107`).
5. **M-4:** enforce the sliding one-minute window in `allow()` (`auth.go:182-220`) or document the burst-per-window semantics as normative.
6. **M-2:** add `LIMIT` to `ListSecurityEvents` and decide retention policy (cf. P-33).
7. **M-3:** enforce `MAIL_PAYLOAD_MAX` in `handleMailNotification`.
8. **M-5:** add secret length/entropy floor at config load (Go + Realm + login).
9. **M-6/M-7:** whitelist fields in login `/login` passthrough; prefer POST for session tokens.
10. **M-8:** consider keyed/blind hash (HMAC with service-local secret or per-account salt) for `email_lookup_hash` if the DB-compromise profile matters; decision needed first.
11. **M-10:** use the mysql driver's option map or escape DSN components.
12. **N-2:** remove or implement `RECOVERY_CALLBACK_HOST`.

---

## 9. Fachbuch references (verifiable titles via `references/README.md` index)

- **Mastering API Architecture** (James Gough, Daniel Bryant, Matthew Auburn; O'Reilly 2022) – index categories "Security API, Networking, Gameserver": Authentifizierung/Authorization, API-Gateways, Threat Modeling (index entry `masteringapiarchitecture`).
- **API Design Patterns** (J. J. Geewax; Manning 2022) – Auth-Muster Kap. 21/24/30 (index entry `apidesignpatterns`).
- **Web Application Security (2nd Ed.)** (Andrew Hoffman; O'Reilly 2024) – Injection/DoS/Business-Logic and Countermeasures Kap. 10-18, 20-37 (index entry `webapplicationsecurity2ndedition`).
- **Defensive Security Handbook (2nd Ed.)** (Amanda Berlin, Lee Brotherston, William F. Reyor III; O'Reilly 2024) – Authentifizierung Kap. 15, Logging/Monitoring Kap. 21-22 (index entry `defensivesecurityhandbook2ndedition`).
- **Zero Trust Networks (2nd Ed.)** (Razi Rais, Christina Morillo, Evan Gilman, Doug Barth; O'Reilly 2024) – Autorisierung Kap. 4, Geräte-/Identitäts-Trust Kap. 5-6 (index entry `zerotrustnetworks2ndedition`).

Page-level citations were not extracted from the EPUBs in this phase; references are limited to the verifiable index entries in `references/README.md`.

---

## 10. Fix-Log Phase 1

### H-1 – TOTP-Replay-Race (FIXED, offline-verified)
- **Root cause:** `handlers.go:154` pre-read of `last_totp_counter` + unconditional `SetLastTOTP` (`store.go:1192-1199`) → two concurrent `/auth/verify` requests with the same valid step both succeeded.
- **Fix:** `SetLastTOTP` (`store.go`) is now a conditional atomic UPDATE `WHERE id = ? AND (last_totp_counter IS NULL OR last_totp_counter < ?)` + `RowsAffected`→`sql.ErrNoRows` (`errWrapNoRows`). Handler (`handlers.go`) maps `sql.ErrNoRows` to the indistinguishable `two_factor_required` fail shape; other DB errors stay 500. FakeStore (`fakestore_test.go:503`) mirrors the condition.
- **Tests:** `TestTwoFactorTOTPConcurrentReplay` (20 rounds × 2 concurrent goroutines, same step, fresh account per round – exactly one winner; `-count=20` re-runs green), `TestSetLastTOTPAtomicStep` (NULL/equal/older/newer/reset semantics). Sequential replay tests still pass; full Go suite green, `gofmt -l` clean.
- **Limits:** verified on FakeStore only – no live-MariaDB proof (same boundary as P-34). No changes to H-2/H-3 or other findings.

### H-2 – Rate-Limit-Key umgehung via URL-Pfade (FIXED, offline-verified)
- **Root cause:** the pre-auth limiter keyed on the raw, attacker-controlled path: `ratePreAuth` key = `"ip:" + clientIP + ":" + r.URL.Path` (`src/api/server.go:153`, `src/login/server.go:59`). Every unique path minted a fresh limiter entry that passed once, so the per-IP burst limit was bypassable for unknown paths (they 404 at the mux but the limiter never bound them), and `rl.hits` grew with the number of distinct paths per ~3 min sweep window → memory pressure.
- **Fix (targeted, existing middleware/routing kept):** the pre-auth middleware now keys a request by its **matched registered route**, not the raw path. `handler()` builds a `routes` set through a `reg`-closure that registers on the mux and records the pattern in one place (no drift); `ratePreAuth(mux, routes)` maps the path via `rateLimitPathKey`: a registered route keeps its route as key segment, every path no route serves collapses into ONE fixed bucket `"*"`. The IP stays in the key (`"ip:" + clientIP + ":" + routeKey`), so the per-client bound and the TrustedProxies/`clientIP` trust rules are unchanged; burst/perMin and the sweep are untouched. The post-auth key (`src/api/auth.go:115`, `"svc:"+cred.ID+":"+path`) is inherently bounded (only registered routes reach a handler) and was left unchanged.
- **Tests:** `TestRateLimitPreAuthUnknownPathBypass` (same IP, burst+1 unique unknown paths → 429 and exactly 1 live limiter key, proving the per-IP bound is not bypassed and the key space does not grow with path count), `TestRateLimitPreAuthProtectedEndpoint` (repeated unauthenticated hits on a protected endpoint reach 429), `TestRateLimitPreAuthEndpointSeparation` (exhausting `/status` does not block `/health`), `TestRateLimitPreAuthDistinctIPs` (per-IP independence of the fixed bucket) — added in both `api_test.go` and `login_test.go`. Existing `TestRateLimitPreAuth` still passes; full Go suites green (`go test ./...`), `gofmt -l` clean.
- **Limits:** verified on the in-process limiter only (same shape as production, one auth service per process); no live-MariaDB proof (not needed here). No changes to H-1/H-3 or other findings.

## 11. Status of related known points

Already tracked in `docs/Security.md` and not re-raised as new: P-33 (session retention, `BESTÄTIGT`), P-34 (2FA atomicity, `ERLEDIGT`), P-35 (parental best-effort history writes), AUTH-02a/02b (session lifecycle/revocation), P-11/P-12/P-14/P-17/P-18/P-22/P-23/P-26/P-27 (Realm persistenz). This audit adds the new items H-1, H-2, H-3, M-1…M-10, N-1…N-6 above.
