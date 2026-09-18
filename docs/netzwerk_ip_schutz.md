# Netzwerk-/IP-Schutz

Für Security-/Anti-Abuse-Zwecke sollen IPv4 und IPv6 berücksichtigt werden.

---

## Keine MAC-Adressen

- Internetserver sieht die echte Client-MAC ohnehin nicht
- clientseitig übermittelte MAC wäre manipulierbar
- unnötiges Fingerprinting vermeiden

---

## Netzwerkdaten-Zuordnung

IP
→ Prefix/Netz
→ ASN
→ Organisation/Provider
→ Netzwerktyp
→ GeoIP

---

## Netzwerktypen

Mögliche Netzwerktypen:
- Residential/ISP
- Hosting/VPS/Datacenter
- VPN/Proxy
- unbekannt

---

## GeoIP

GeoIP bezeichnet ausschließlich die geographische Zuordnung der
verwendeten IP zum Zeitpunkt des Vorfalls.

**Es ist KEIN Beweis für den tatsächlichen Aufenthaltsort oder die
Nationalität des Spielers.**

---

## Provider-/ASN-Daten

Provider-/ASN-Daten sind Signale, keine alleinigen Beweise für RMT.