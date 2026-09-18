# Anti-RMT V1

**Begriff:**
Andoras normale Spielwährung heißt Idia.
In neuer Dokumentation daher nicht von „Goldsellern“, sondern RMT,
Idia-Sellern bzw. Seller-Accounts sprechen.

**Arbeitsbereich:**
Anti-RMT V1 arbeitet ausschließlich auf:
- Accountebene
- Charakterebene

V1 soll NICHT bereits komplexe Gilden-, Handels- oder Transaktionsnetzwerke untersuchen.

---

## Grundprinzip

Chatnachricht
→ schneller normaler Filter
→ bei unbekannten/verdächtigen Mustern ggf. lokale KI
→ KI klassifiziert Inhalt
→ erkannte neue RMT-/Spam-Muster können dem normalen Filter als
   neue normalisierte Regeln/Signaturen zur Verfügung gestellt werden
→ zukünftige gleiche/ähnliche Fälle benötigen möglichst keine erneute
   KI-Prüfung.

---

## Normalisierung

Konzeptionell berücksichtigen:
- ungewöhnliche Leerzeichen
- Unicode-Tricks
- URL-Muster
- Schreibvarianten
- Geldbeträge/Echtgeldmuster
- typische RMT-Werbestrukturen

**Nicht nur exakte Nachrichtentexte speichern.**

---

## Neue Regeln

- persistent gespeichert werden können
- ohne Realm-Neustart aktiv werden können.

---

## WICHTIG: KI-Beschränkungen

Die KI darf nicht autonom dauerhafte Account-, IP-, Prefix- oder
Provider-Banns beschließen.

KI dient für:
- Erkennung
- Klassifizierung
- Vorsortierung
- Analyse
- Erzeugung geeigneter Filterkandidaten

Feste serverseitige Security-Regeln dürfen dagegen definierte temporäre
Schutzmaßnahmen automatisch auslösen.

---

## V1 Scope

Untersucht nicht:
- Gildenaktivitäten
- Handelsnetzwerke
- Transaktionsmuster zwischen mehreren Accounts

Fokus liegt auf:
- Einzelnen Accounts
- Einzelnen Charakteren
- Chat-Inhalten auf diesen Ebenen