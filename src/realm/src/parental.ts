// parental.ts — Serverseitige Elternkontrolle im Realm.
// Der Client entscheidet NIE: Alle Werte (Restzeit, Blockade, Puffer,
// Chat-/Voice-Rechte) kommen von der Auth-API; der Client stellt sie
// nur dar (Countdown-Extrapolation zwischen den Polls).
// Ablauf:
//   HELLO (mit session_id) -> Session validieren -> /parental/status.
//   Ist der Tag BLOCKED, wird der Einstieg verweigert (kein Puffer fuer
//   neue Logins). Laeuft eine bestehende Sitzung ins Budgetende, startet
//   die Auth-API den Puffer (15 Min So-Do, 30 Min Fr-Sa); nach Pufferende
//   trennt der Realm die Sitzung (PARENTAL_BLOCKED + Close).
//   Der Realm pollt den Status pro beaufsichtigtem Spieler ca. alle 10 s.
//   Temporaere Mechanismus-Freischaltungen (Chat/Voice) gelten nur fuer
//   die aktuelle Sitzung (Realm-Speicher) und verfallen mit deren Ende.
import WebSocket from 'ws';
import { S2C } from './protocol';
import { players } from './world';
import {
  authApiEnabled, fetchParentalStatus, verifyParentPin, useParentalExtension,
  AuthApiError, type ParentalApiStatus
} from './authapi';
import type { NetMsg, Player } from './types';

/** Realm-seitiger Elternkontroll-State eines Spielers (Sitzungs-Speicher). */
export interface ParentalPlayerState {
  accountId: number;
  enabled: boolean;
  remainingSeconds: number;
  blocked: boolean;
  bufferUntil: string | null;
  forceLogout: boolean;
  chatAllowed: boolean;
  voiceAllowed: boolean;
  warning: boolean;
  extendedUsedToday: boolean;
  /** Temporaere Sitzungs-Freischaltungen nach PIN (verfallen am Disconnect). */
  tempChat: boolean;
  tempVoice: boolean;
}

const states = new Map<string, ParentalPlayerState>();
const POLL_MS = 10000;
let timer: NodeJS.Timeout | null = null;

export function parentalStateOf(playerId: string): ParentalPlayerState | undefined {
  return states.get(playerId);
}

function applyApiStatus(st: ParentalPlayerState, s: ParentalApiStatus): void {
  st.enabled = s.enabled;
  st.remainingSeconds = s.remaining_seconds ?? 0;
  st.blocked = s.blocked ?? false;
  st.bufferUntil = s.buffer_until ?? null;
  st.forceLogout = s.force_logout ?? false;
  st.chatAllowed = s.chat_allowed ?? true;
  st.voiceAllowed = s.voice_allowed ?? true;
  st.warning = s.warning ?? false;
  st.extendedUsedToday = s.extended_used_today ?? false;
}

function sendStatus(pl: Player, st: ParentalPlayerState): void {
  if (pl.ws.readyState !== WebSocket.OPEN) return;
  pl.ws.send(JSON.stringify({
    seq: 0, type: S2C.PARENTAL_STATUS,
    data: {
      remaining_seconds: st.remainingSeconds,
      blocked: st.blocked,
      buffer_until: st.bufferUntil,
      warning: st.warning,
      chat_allowed: st.chatAllowed || st.tempChat,
      voice_allowed: st.voiceAllowed || st.tempVoice,
      extended_used_today: st.extendedUsedToday
    }
  }));
}

function sendBlocked(pl: Player, reason: string): void {
  if (pl.ws.readyState !== WebSocket.OPEN) return;
  pl.ws.send(JSON.stringify({
    seq: 0, type: S2C.PARENTAL_BLOCKED, data: { reason }
  }));
}

function sendResult(pl: Player, seq: number, data: unknown): void {
  if (pl.ws.readyState !== WebSocket.OPEN) return;
  pl.ws.send(JSON.stringify({ seq, type: S2C.PARENTAL_RESULT, data }));
}

/** Elternkontrolle fuer einen frisch per HELLO registrierten Spieler
 *  anhaengen. Ergebnis ok=false -> Einstieg verweigern (Verbindung
 *  schliessen). Auth-API-Fehler beim Login sind fail-closed. */
export async function attachParental(
  pl: Player, accountId: number, sessionId: string
): Promise<{ ok: true } | { ok: false; reason: string }> {
  pl.accountId = accountId;
  pl.sessionId = sessionId;
  if (!authApiEnabled() || accountId <= 0) return { ok: true };
  let s: ParentalApiStatus;
  try {
    s = await fetchParentalStatus(accountId, sessionId || null);
  } catch (e) {
    console.error('parental status at hello:', (e as Error).message);
    return { ok: false, reason: 'status_unavailable' };
  }
  const st: ParentalPlayerState = {
    accountId, enabled: false, remainingSeconds: 0, blocked: false,
    bufferUntil: null, forceLogout: false, chatAllowed: true, voiceAllowed: true,
    warning: false, extendedUsedToday: false, tempChat: false, tempVoice: false
  };
  applyApiStatus(st, s);
  states.set(pl.id, st);
  if (!st.enabled) return { ok: true };
  sendStatus(pl, st);
  if (st.blocked || st.forceLogout) {
    sendBlocked(pl, 'blocked');
    return { ok: false, reason: 'blocked' };
  }
  return { ok: true };
}

/** Sitzungs-State aufraeumen (Disconnect; temporaere Freischaltungen
 *  verfallen mit der Sitzung). */
export function detachParental(playerId: string): void {
  states.delete(playerId);
}

/** Chat-Gate fuer handlers/chat.ts (temporaere Freischaltung inklusive). */
export function chatAllowedFor(pl: Player): boolean {
  const st = states.get(pl.id);
  if (!st || !st.enabled) return true;
  return st.chatAllowed || st.tempChat;
}

/** Voice-Gate (fuer das spaetere Voice-System; Flag schon heute da). */
export function voiceAllowedFor(pl: Player): boolean {
  const st = states.get(pl.id);
  if (!st || !st.enabled) return true;
  return st.voiceAllowed || st.tempVoice;
}

/** C2S.PARENTAL: {action:'extend'|'unlock_chat'|'unlock_voice', pin}.
 *  Antwort via S2C.PARENTAL_RESULT. */
export async function handleParentalMessage(pl: Player, msg: NetMsg): Promise<void> {
  const st = states.get(pl.id);
  if (!authApiEnabled() || !st || !st.enabled) {
    sendResult(pl, msg.seq, { ok: false, reason: 'not_supervised' });
    return;
  }
  const action = String(msg.data?.action ?? '');
  const pin = String(msg.data?.pin ?? '');
  if (!/^\d{4,16}$/.test(pin)) {
    sendResult(pl, msg.seq, { ok: false, reason: 'bad_pin' });
    return;
  }
  try {
    if (action === 'extend') {
      const r = await useParentalExtension(st.accountId, pin);
      st.remainingSeconds = r.remaining_seconds;
      st.extendedUsedToday = true;
      sendStatus(pl, st);
      sendResult(pl, msg.seq, {
        ok: true, unlocked: 'extend',
        remaining_seconds: r.remaining_seconds, extended_used_today: true
      });
      return;
    }
    if (action === 'unlock_chat' || action === 'unlock_voice') {
      const v = await verifyParentPin(st.accountId, pin);
      if (!v.valid) {
        sendResult(pl, msg.seq, { ok: false, reason: 'bad_pin' });
        return;
      }
      if (action === 'unlock_chat') st.tempChat = true;
      else st.tempVoice = true;
      sendStatus(pl, st);
      sendResult(pl, msg.seq, { ok: true, unlocked: action });
      return;
    }
    sendResult(pl, msg.seq, { ok: false, reason: 'unknown_action' });
  } catch (e) {
    if (e instanceof AuthApiError && e.status === 409) {
      sendResult(pl, msg.seq, { ok: false, reason: 'extension_used' });
      return;
    }
    if (e instanceof AuthApiError && e.status === 401) {
      sendResult(pl, msg.seq, { ok: false, reason: 'bad_pin' });
      return;
    }
    console.error('parental action:', (e as Error).message);
    sendResult(pl, msg.seq, { ok: false, reason: 'service_unavailable' });
  }
}

async function pollOnce(): Promise<void> {
  if (!authApiEnabled()) return;
  for (const pl of players.values() as IterableIterator<Player>) {
    const st = states.get((pl as Player).id);
    if (!st || !st.enabled) continue;
    const p = pl as Player;
    try {
      const s = await fetchParentalStatus(st.accountId, p.sessionId || null);
      applyApiStatus(st, s);
      // Spielzeit waehrend des Polls serverseitig verbucht (die
      // Auth-API akkumuliert die Sekunden seit dem letzten Poll).
      sendStatus(p, st);
      // Nur forceLogout trennt: blocked allein bedeutet waehrend einer
      // laufenden Sitzung lediglich "Puffer laeuft" (der Puffer gehoert
      // zur bestehenden Sitzung und laeuft erst aus).
      if (st.forceLogout) {
        sendBlocked(p, 'buffer_expired');
        p.ws.close();
      }
    } catch (e) {
      // Netz-/API-Fehler waehrend der Sitzung: letzten bekannten State
      // behalten (kein Kick bei Schluckauf), nur loggen.
      console.error('parental poll:', (e as Error).message);
    }
  }
}

/** 10-s-Poller starten (aus main.ts, einmalig). */
export function startParentalPoller(): void {
  if (timer || !authApiEnabled()) return;
  timer = setInterval(() => {
    pollOnce().catch((e) => console.error('parental poll:', e));
  }, POLL_MS);
  timer.unref?.();
}

/** Poller stoppen (Shutdown). */
export function stopParentalPoller(): void {
  if (timer) clearInterval(timer);
  timer = null;
}
