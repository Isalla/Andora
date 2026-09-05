// bind.ts — Bind-Host-Auswertung für die Listener des Realm-/World-Servers.
// Semantik identisch zu src/api/bind.go (Go) und src/realm-rs/src/config.rs:
//
//   "" | "auto"   alle Interfaces (bisheriges Verhalten)
//   "ipv4" | "4"  IPv4-Wildcard 0.0.0.0 (nur IPv4)
//   "ipv6" | "6"  IPv6-Wildcard [::] mit ipv6Only (nur IPv6)
//   "dual"|"both" explizit zwei Listener (0.0.0.0 + [::] ipv6Only),
//                 keine Abhängigkeit von IPv4-mapped Acceptance
//   IP-Literal    Family des Literals (IPv6 mit ipv6Only)
//   Hostname      ein Listener mit diesem Hostnamen (Auflösung durch Node)
import net from 'net';

export interface BindPlan {
  host?: string;
  ipv6Only?: boolean;
}

export function bindHosts(host: string): BindPlan[] {
  const h = (host || '').trim().toLowerCase();
  switch (h) {
    case '':
    case 'auto':
      return [{}];
    case 'ipv4':
    case '4':
      return [{ host: '0.0.0.0' }];
    case 'ipv6':
    case '6':
      return [{ host: '::', ipv6Only: true }];
    case 'dual':
    case 'both':
      return [{ host: '0.0.0.0' }, { host: '::', ipv6Only: true }];
    default:
      break;
  }
  const raw = h.replace(/^\[/, '').replace(/\]$/, '');
  if (/^[0-9a-f:]+$/i.test(raw) && net.isIP(raw) === 6) {
    return [{ host: raw, ipv6Only: true }];
  }
  return [{ host: h }];
}

// bracketUrlHost klammer einen ungeklammerten IPv6-Literal im Host-Anteil
// einer URL (Port-zuerst), kompatibel zu src/login/urlutil.go.
export function bracketUrlHost(raw: string): string {
  const schemeEnd = raw.indexOf('://');
  if (schemeEnd === -1) return raw;
  const rest = raw.slice(schemeEnd + 3);
  let authEnd = rest.length;
  const cut = rest.search(/[/?#]/);
  if (cut !== -1) authEnd = cut;
  const auth = rest.slice(0, authEnd);
  if (!auth || auth.startsWith('[') || !auth.includes(':')) return raw;
  const li = auth.lastIndexOf(':');
  if (li > 0) {
    const hostPart = auth.slice(0, li);
    const portPart = auth.slice(li + 1);
    if (portPart && /^\d+$/.test(portPart) && net.isIP(hostPart) === 6) {
      return `${raw.slice(0, schemeEnd + 3)}[${hostPart}]:${portPart}${rest.slice(authEnd)}`;
    }
  }
  if (net.isIP(auth) === 6) {
    return `${raw.slice(0, schemeEnd + 3)}[${auth}]${rest.slice(authEnd)}`;
  }
  return raw;
}