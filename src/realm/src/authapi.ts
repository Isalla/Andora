// authapi.ts — Signierter Auth-API-Client des Realm-/World-Servers.
// Der Realm besitzt KEINE AUTH_DB-Zugangsdaten und KEINEN direkten
// auth-DB-Zugriff. Alle Auth-Funktionen (Session-/Handoff-Validierung,
// Elternkontrolle) laufen ueber die Auth-API des separaten
// Auth-/API-Services (Go, src/api) mit eigener Service-Credential.
// Signatur wie in src/api/auth.go (authorize/signPayload):
//   X-Andora-Service, X-Andora-Timestamp, X-Andora-Signature =
//   hex(HMAC-SHA256(secret, METHOD\nPATH\nRAW_QUERY\nTIMESTAMP\nSHA256(BODY)))
// Ist AUTHAPI_URL leer, ist die Auth-Anbindung (inkl. Elternkontrolle)
// bewusst deaktiviert (Entwicklung/Testprototyp).
import crypto from 'crypto';
import http from 'http';
import https from 'https';
import { config } from './config';

/** Antwort der Auth-API als /parental/status (Auszug). */
export interface ParentalApiStatus {
  enabled: boolean;
  remaining_seconds: number;
  blocked: boolean;
  buffer_until?: string;
  force_logout?: boolean;
  chat_allowed: boolean;
  voice_allowed: boolean;
  week_minutes: number;
  warning_threshold_seconds: number;
  extended_used_today?: boolean;
  warning?: boolean;
}

export interface SessionApiValidation {
  valid: boolean;
  account_id?: number;
  expires_at?: string;
}

export interface PinApiVerify {
  valid: boolean;
}

export interface ExtensionApiResult {
  granted: boolean;
  remaining_seconds: number;
  extended_used_today: boolean;
}

/** HTTP-Fehler der Auth-API (Status + ggf. Body). */
export class AuthApiError extends Error {
  status: number;
  body: string;
  constructor(status: number, body: string) {
    super(`authapi ${status}: ${body}`);
    this.status = status;
    this.body = body;
  }
}

/** True, wenn die Auth-Anbindung konfiguriert ist. */
export function authApiEnabled(): boolean {
  return config.authApi.url !== '';
}

function sign(secret: string, method: string, path: string, ts: number, body: string): string {
  const bodyHash = crypto.createHash('sha256').update(body, 'utf8').digest('hex');
  const payload = `${method}\n${path}\n\n${ts}\n${bodyHash}`;
  return crypto.createHmac('sha256', secret).update(payload, 'utf8').digest('hex');
}

/** Signierter POST gegen die Auth-API, Antwort als JSON geparst. */
export function authApiPost<T>(path: string, data: unknown, timeoutMs = 5000): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const body = JSON.stringify(data ?? {});
    const url = new URL(config.authApi.url + path);
    const ts = Math.floor(Date.now() / 1000);
    const sig = sign(config.authApi.secret, 'POST', url.pathname, ts, body);
    const lib = url.protocol === 'https:' ? https : http;
    const req = lib.request(
      {
        hostname: url.hostname,
        port: url.port || (url.protocol === 'https:' ? 443 : 80),
        path: url.pathname,
        method: 'POST',
        timeout: timeoutMs,
        headers: {
          'Content-Type': 'application/json',
          'Content-Length': Buffer.byteLength(body),
          'X-Andora-Service': config.authApi.serviceId,
          'X-Andora-Timestamp': String(ts),
          'X-Andora-Signature': sig
        }
      },
      (res) => {
        const chunks: Buffer[] = [];
        res.on('data', (c) => chunks.push(Buffer.isBuffer(c) ? c : Buffer.from(c)));
        res.on('end', () => {
          const text = Buffer.concat(chunks).toString('utf8');
          if (res.statusCode !== 200 && res.statusCode !== 201) {
            reject(new AuthApiError(res.statusCode ?? 0, text));
            return;
          }
          try {
            resolve(JSON.parse(text) as T);
          } catch (e) {
            reject(new Error(`authapi: invalid JSON from ${path}: ${(e as Error).message}`));
          }
        });
      }
    );
    req.on('timeout', () => req.destroy(new Error(`authapi: timeout ${path}`)));
    req.on('error', (e) => reject(e));
    req.write(body);
    req.end();
  });
}

/** Session gegen die Auth-API pruefen (account.permissions: session.validate). */
export function validateSession(sessionId: string): Promise<SessionApiValidation> {
  return authApiPost<SessionApiValidation>('/session/validate', { session_id: sessionId });
}

/** Effektiven Elternkontroll-Status laden (parental.status). */
export function fetchParentalStatus(accountId: number, sessionId: string | null): Promise<ParentalApiStatus> {
  return authApiPost<ParentalApiStatus>('/parental/status', {
    account_id: accountId,
    session_id: sessionId ?? ''
  });
}

/** Eltern-PIN pruefen (parental.pin). */
export function verifyParentPin(accountId: number, pin: string): Promise<PinApiVerify> {
  return authApiPost<PinApiVerify>('/parental/pin/verify', {
    account_id: accountId,
    parent_pin: pin
  });
}

/** Einmalige +1h-Verlaengerung einloesen (parental.pin). */
export function useParentalExtension(accountId: number, pin: string): Promise<ExtensionApiResult> {
  return authApiPost<ExtensionApiResult>('/parental/extension', {
    account_id: accountId,
    parent_pin: pin
  });
}
