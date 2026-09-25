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

---

## Connection-Takeover und IP-Korrelation

- Die alte und die neue Quell-IP eines authentifizierten Connection-Takeovers dürfen als Sicherheitssignal protokolliert und miteinander verglichen werden.
- Eine IP, ein Prefix, ein ASN, ein Provider oder eine GeoIP-Zuordnung sind Indizien, keine Beweise.
- Gemeinsame IP-Adressen können durch Familien, Firmen, Mobilfunk und Carrier-NAT entstehen.
- Wechselnde IP-Adressen können regulär auftreten.
- Dieselbe IP-Adresse bei ungewöhnlich vielen verschiedenen Accounts darf einen Prüf-/Alarmhinweis erzeugen.
- Ein Connection-Takeover oder eine IP-Korrelation allein erzeugt keinen automatischen Bann.
- Die spezielle bestehende RMT-Regel aus `automatische_ip-sperre.md` wird dadurch weder verändert noch erweitert.
- Bei Reverse-Proxy-Betrieb dürfen weitergereichte Client-IPs nur von ausdrücklich vertrauenswürdigen Proxy-Systemen übernommen werden. Ungeprüfte Client-Header gelten nicht als Quell-IP.
