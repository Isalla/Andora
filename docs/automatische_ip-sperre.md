# Automatische temporäre IP-Sperre

Für normale Residential-/ISP-Adressen darf der Server anhand klar
definierter bestätigter Anti-Abuse-Signale automatisch eine zeitlich
begrenzte Schutzsperre setzen.

---

## Grundidee

- bei wiederholten bestätigten Seller-/RMT-Fällen über dieselbe
  Residential-IP
- automatische temporäre Sperre
- Dauer: 24 Stunden
- danach automatische Freigabe

---

## Schwellenwert

V1-bindende Regel:
- ausschließlich Residential-/ISP-IP
- 20 bestätigte RMT-/Seller-Accounts über dieselbe öffentliche IP
- automatische Sperre dieser einzelnen IP für 24 Stunden
- danach automatische Freigabe
- rohe Spielerreports zählen NICHT als bestätigte RMT-Fälle
- Spielerreports dürfen für sich allein niemals diese Sperre auslösen

Keine darüber hinausgehende Eskalationsregel erfinden.

Insbesondere keine automatische längere Wiederholungssperre,
Prefix-Sperre, ASN-Sperre oder Provider-Sperre hinzufügen.

---

## WICHTIG: Rohe Reports zählen nicht

Spielerreports selbst sind keine bestätigten Fälle.
Eine große Zahl unbestätigter Reports darf daher nicht unmittelbar
die 24h-IP-Sperre auslösen.

---

## Protokollierung

Jede automatische Netzwerksperre muss nachvollziehbar protokolliert sein:
- Ziel/IP
- Grund
- auslösende bestätigte Fälle
- Erstellungszeit
- Ablaufzeit
- automatische/manuelle Herkunft

---

## Aktive Regeln

Aktive Regeln sollen ohne Realm-Neustart wirksam werden.

---

## Account vs. Netzwerk

Account-Sperren und Netzwerk-Sperren logisch unterscheiden.

---

## Breitere Maßnahmen (nicht V1)

Breitere Maßnahmen wie:
- IPv6-/IPv4-Prefix-Sperren
- ASN-Sperren
- komplette Provider-Sperren

dürfen NICHT aufgrund eines einzelnen Falls automatisch entstehen.

Solche Maßnahmen sind besonders kollateralschadenanfällig und gehören
in eine gesonderte Security-Entscheidung.

Für spätere Security-Auswertung kann angezeigt werden, wie viele
bestätigte und nicht bestätigte/unauffällige Accounts durch eine
breitere Sperre betroffen wären.