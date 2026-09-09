# Aufgabe: Dynamisches NPC-, Informations- und Beziehungssystem für Andora

Analysiere zuerst die bestehende Serverarchitektur und vorhandenen NPC-, Spieler-, Raid-, Welt-, Event- und Datenbank-Komponenten.

Ändere noch nichts, bevor du verstanden hast, welche Systeme bereits vorhanden sind und wiederverwendet werden können.

## Ziel

Baue die Grundlage für ein dynamisches NPC-System, bei dem NPCs einmalige Personen in der gemeinsamen Spielwelt sind.

NPCs sollen:

* individuelle Beziehungen zu Spielern besitzen
* nur an einem Ort gleichzeitig existieren
* Informationen nur kennen, wenn sie diese tatsächlich erhalten haben
* Informationen untereinander weitergeben können
* auf Ereignisse reagieren können
* sichtbar durch die Welt reisen können
* unter bestimmten Bedingungen Spielern oder Gruppen helfen können
* nach einem Ereignis wieder zu ihrem normalen Aufenthaltsort zurückkehren

Die Runtime-KI (lokal aktuell: Ollama) soll später Persönlichkeit, Dialoge und begrenzte Entscheidungen ermöglichen.

Die Wahrheit über die Spielwelt bleibt jedoch immer beim Gameserver.

Für Kampfverhalten von NPCs/Monstern gelten ergänzend die verbindlichen Gegnergrundregeln in `Kampfsystem.md` (Abschnitte 18–21): Content-/Lua-definierter Grundzustand (`attackable`/`aggressive` getrennt), kontextgebundene Überschreibungen, Home-Zone/Verfolgung, Evade/Return als Combat-Reset sowie Respawn als Contentwerte. Dieses Dokument regelt dazu komplementär Wissen, Beziehungen und Reiseverhalten, nicht die Kampfregeln selbst.

---

# 0. Erinnerungs- und Beziehungskonzept

## Grundprinzip

NPC-Erinnerungen werden beim Coordinator persistent gespeichert, nicht nur im RAM oder LLM-Kontext.

Ein Server- oder Coordinator-Neustart darf Erinnerungen nicht verlieren.

Die Speicherung erfolgt dateibasiert, da der Coordinator weiterhin keinen DB-Zugriff erhält.

Erinnerungsdaten liegen ausdrücklich nicht im bestehenden Queue-Ordner, sondern in einem separaten persistenten Speicherbereich.

## Stabile interne IDs

Dateien und Speicherpfade verwenden stabile interne IDs, keine Spieler- oder Charakternamen als Dateinamen oder primäre Identität.

Charaktere werden über Character-IDs identifiziert.

NPCs innerhalb persönlicher Erinnerungen über NPC-IDs.

Namensänderungen dürfen bestehende Erinnerungen und Beziehungen nicht zerstören.

## Persönliche Erinnerungen

Pro Character-ID existiert eine persistente persönliche Erinnerungsstruktur.

Darin können alle NPCs aufgeführt werden, mit denen dieser Charakter tatsächlich Kontakt hatte.

Pro NPC können unter anderem individueller Beziehungsstatus und relevante gemeinsame Erinnerungen gespeichert werden.

Jeder Charakter besitzt zu jedem NPC seinen eigenen unabhängigen Beziehungsstatus.

Beziehungen verschiedener Charaktere werden nicht zusammengelegt.

Erinnerungen dürfen Verknüpfungen zwischen bekannten Personen enthalten, zum Beispiel „Charakter A sagt, Charakter B sei sein Vater".

Solche Aussagen müssen als Aussage bzw. Erinnerung behandelt werden und dürfen nicht automatisch zu objektiver Weltwahrheit werden.

## Beziehungen zwischen Personen

Ein NPC darf seine persönlichen Erinnerungen an eine erwähnte Person abrufen.

Beispiel: Ein Spieler erwähnt seinen Vater Bandalor. Kennt der Schmied Bandalor, dürfen dessen persönliche Erinnerungen und sein Beziehungsstatus die Antwort beeinflussen.

Die Beziehung zum Sohn bleibt trotzdem vollständig unabhängig.

Ein NPC kann einem bekannten Spieler später von Begegnungen mit dessen verknüpften Angehörigen oder Bekannten erzählen.

## Shared Knowledge

Shared Knowledge wird getrennt von persönlichen NPC-Erinnerungen gespeichert, ebenfalls persistent und nach Character-ID organisiert.

Es enthält nur allgemein erzählbares Wissen oder Ruf über einen Charakter, keine erfundenen persönlichen Begegnungen.

Beispiele: lange in einer Region unterwegs, als tapfer bekannt, bedeutende Taten, Beteiligung an bekannten Ereignissen, bekannter Handwerker, seit langer Zeit nicht mehr gesehen.

Nicht jede Aktivität eines Spielers wird automatisch Shared Knowledge.

NPCs, die einen Charakter nie persönlich getroffen haben, können dadurch trotzdem von ihm gehört haben.

Sie müssen sprachlich zwischen persönlicher Bekanntschaft und gehörtem bzw. allgemeinem Wissen unterscheiden.

## Verbreitung von Shared Knowledge

Allgemeines Wissen ist nicht automatisch jedem NPC bekannt.

Seine plausible Verbreitung kann unter anderem von Fraktion, Region sowie Bedeutung oder Bekanntheit des Charakters abhängen.

Ein lokal bekannter Charakter kann einer anderen Fraktion völlig unbekannt sein; eine weithin bekannte Persönlichkeit kann auch dort als Name oder Geschichte bekannt sein.

Die genauen Regeln und Formeln dafür bleiben vorerst offen.

## Langfristige Weltgeschichte

Erinnerungen können auch nach langer Inaktivität eines Charakters erhalten bleiben.

Dadurch können ehemalige Spieler später Teil der erzählten Geschichte eines Realms werden, ohne dass Entwickler dafür feste Lore-Dialoge schreiben müssen.

Persönliche NPC-Erinnerungen und Shared Knowledge müssen dabei klar unterscheidbar bleiben.

## KI-Kontext

Das LLM muss nicht sämtliche Erinnerungen permanent im Kontext halten.

Der Coordinator ermittelt für eine konkrete Unterhaltung nur die relevanten Erinnerungen und stellt diese der KI bereit.

Persistenter Speicher ist die Grundlage; RAM darf später lediglich als Cache oder Index dienen.

Ein Verlust des RAM-Caches darf keinen Verlust der Erinnerungen verursachen.

## Realm-Grenze

Der Coordinator erhält weiterhin keinerlei direkten DB-Zugriff.

Spielzustände oder Fakten, die für Erinnerungen benötigt werden, müssen ihm über die dafür vorgesehene Realm- oder Coordinator-Schnittstelle übermittelt werden.

Die KI darf unbekannte Weltfakten nicht eigenmächtig zu objektiver Wahrheit erklären.

---

# Sehr wichtige Architekturregel

## Keine große NPC-Datei bauen

Dieses System MUSS modular aufgebaut werden.

Nicht alle Funktionen in:

* `npc.js`
* `npcManager.js`
* `gameServer.js`
* oder einer anderen einzelnen großen Datei

unterbringen.

Teile Verantwortlichkeiten in mehrere klar strukturierte Module auf.

Beispielsweise sinnvoll:

```text
server/
└── npc/
    ├── npcManager
    ├── npcState
    ├── npcMovement
    ├── npcRelationships
    ├── npcKnowledge
    ├── npcEvents
    ├── npcDecisionService
    ├── npcDialogueService
    └── npcRepository
```

Zusätzlich können je nach bestehender Architektur eigene Module entstehen für:

```text
events/
travel/
crime/
raids/
world/
```

Die konkreten Dateinamen dürfen an die bestehende Projektstruktur angepasst werden.

Entscheidend ist:

**Eine Datei = eine klar abgegrenzte Verantwortung.**

Wenn eine Datei beginnt, mehrere unabhängige Systeme zu enthalten oder unnötig groß zu werden, muss sie weiter aufgeteilt werden.

Keine künstliche Aufteilung in viele winzige Dateien, aber auch keine monolithischen Manager mit hunderten unterschiedlicher Verantwortlichkeiten.

---

# 1. NPCs sind einmalige Personen

Ein benannter NPC existiert serverweit nur einmal.

Beispiel:

```text
Borin
```

kann nicht gleichzeitig:

* in seiner Schmiede
* in Raid A
* in Raid B

existieren.

Jeder NPC benötigt deshalb einen eindeutigen Zustand.

Beispiel:

```text
IDLE
WORKING
TRAVELLING
RESERVED
AT_EVENT
AT_RAID
RETURNING
UNAVAILABLE
DEAD
```

Die Zustände sollen sinnvoll an die bestehende Architektur angepasst werden.

Der Server muss verhindern, dass zwei Systeme denselben NPC gleichzeitig reservieren.

Reservierungen müssen zuverlässig und möglichst atomar erfolgen.

---

# 2. Beziehungssystem

NPCs besitzen individuelle Beziehungen zu Spielern.

Beispiel:

```text
NPC: Borin
Player: 5821
Relationship: 78
```

Jeder Charakter besitzt zu jedem NPC seinen eigenen unabhängigen Beziehungsstatus.

Beziehungen verschiedener Charaktere werden nicht zusammengelegt.

Beispiel: Hat Spieler A eine gute Beziehung zum Schmied Borin und Spieler B eine schlechte, wirkt sich dies unabhängig voneinander aus.

Die Beziehung darf später durch verschiedene Ereignisse beeinflusst werden, beispielsweise:

* häufige Einkäufe
* Reparaturen
* erfüllte Aufträge
* Gespräche
* Hilfe für den NPC
* Angriffe
* Diebstahl
* Verrat
* Ereignisse in der Welt

Implementiere zunächst eine erweiterbare Grundlage.

Keine komplexe Balance erfinden, wenn sie noch nicht definiert wurde.

---

# 3. Gruppen profitieren unterschiedlich

Ein NPC kann wegen seiner Beziehung zu EINEM Spieler zu einem Raid oder Ereignis reisen.

Das bedeutet aber nicht, dass automatisch alle anderen Spieler dieselben Vorteile erhalten.

Beispiel Borin:

Ein Spieler besitzt eine sehr gute Beziehung zu Borin.

Dadurch entscheidet Borin möglicherweise, dem Raid zu helfen.

Im Raid:

* Borin ist für alle Spieler sichtbar.
* Spieler mit guter Beziehung erhalten möglicherweise bessere Dienste.
* Spieler mit neutraler Beziehung erhalten normale oder eingeschränkte Dienste.
* Spieler mit sehr schlechter Beziehung können abgelehnt werden.

Die genaue Balance soll später konfigurierbar sein.

NPC-Beziehungen sollen Möglichkeiten schaffen, aber nicht zwangsläufig zu einer Pflicht für jede Gilde werden.

---

# 4. NPC-Wissen

NPCs dürfen NICHT automatisch alles wissen, was auf dem Server passiert.

Jeder NPC besitzt eigenes Wissen.

Beispiel:

```text
Knowledge:
  subject
  information
  source
  timestamp
  confidence
```

Ein NPC darf nur auf Informationen reagieren, die:

* selbst erlebt wurden
* von einem anderen NPC erzählt wurden
* von einem Spieler erzählt wurden
* über ein anderes zulässiges Weltereignis erhalten wurden

Die Runtime-KI darf keine Fakten über die Welt erfinden.

Wenn Borin nicht weiß, dass ein Spieler in einem Raid steckt, darf sein KI-Dialog diese Information nicht kennen.

NPCs unterscheiden zwischen persönlich erlangtem Wissen und allgemein gehörtem Wissen (Shared Knowledge).

Ein NPC, der einen Spieler nie persönlich getroffen hat, kann trotzdem von ihm gehört haben, zum Beispiel durch Gerüchte oder Nachrichten.

Shared Knowledge wird getrennt von persönlichen Erinnerungen gespeichert und muss sprachlich klar davon unterschieden werden.

---

# 5. Nachrichten müssen physisch übertragen werden können

Beispielszenario:

Ein Spieler mit hoher Beziehung zu Borin befindet sich in einem schwierigen Raid.

Ein anderer NPC erfährt davon.

Dieser NPC kann als Bote zu Borin laufen.

Ablauf:

```text
Raid-Ereignis
↓
NPC erhält Information
↓
NPC entscheidet / erhält Aufgabe, Borin zu informieren
↓
NPC reist zu Borin
↓
NPC erreicht Borin
↓
Information wird übertragen
↓
Borin besitzt nun dieses Wissen
↓
Borin kann darauf reagieren
```

Die Information darf NICHT bereits bei Borin gespeichert werden, bevor der Bote ihn erreicht.

---

# 6. Nachrichten können verloren gehen

Das Informationssystem muss echte Konsequenzen erlauben.

Beispiel:

Ein Spieler tötet den Boten auf dem Weg zu Borin.

Dann:

```text
Bote stirbt
↓
Nachricht erreicht Borin nicht
↓
Borin weiß nichts vom Raid
↓
Borin bleibt in seiner Schmiede
```

Das System darf Borin nicht später automatisch das Wissen geben.

---

# 7. Zeugen und Straftaten

Wenn ein Spieler einen NPC angreift oder tötet, kann daraus ein separates Weltereignis entstehen.

Beispiel:

```text
Event:
type: murder
victim: messenger
perpetrator: player
location
timestamp
```

NPCs oder Spieler in Sichtweite können Zeugen sein.

NPC-Zeugen können Wissen über das Ereignis erhalten.

Beispiel:

```text
Witness:
NPC: merchant_12
knowledge:
  Täter
  Opfer
  Ort
  Zeitpunkt
confidence: 0.85
```

Eine Stadtwache darf einen Spieler nicht automatisch verfolgen, wenn niemand von der Tat weiß.

Mögliche Kette:

```text
Spieler tötet Boten
↓
Händler sieht Tat
↓
Händler erhält Wissen
↓
Händler meldet Tat einer Wache
↓
Wache erhält Wissen
↓
Stadt kann auf Täter reagieren
```

Wenn eine Stadtwache die Tat selbst sieht, kann sie direkt reagieren.

Das Kriminalitätssystem zunächst nur als erweiterbare Grundlage implementieren.

Keine vollständige Justizsimulation bauen, wenn sie nicht notwendig ist.

---

# 8. NPC-Reisen durch die Welt

NPCs sollen nicht einfach am Ziel erscheinen.

Wenn Borin zu einem Raid reist:

```text
Schmiede
↓
verlässt Stadt
↓
reist über Weltkarte
↓
erreicht Raid-Eingang
↓
tritt dem Raid-Ereignis bei
```

Andere Spieler sollen Borin unterwegs sehen können.

Sie sollen ihn theoretisch auch ansprechen können.

---

# 9. Effiziente Reise-Simulation

NPCs müssen nicht dauerhaft vollständig simuliert werden, wenn kein Spieler in ihrer Umgebung ist.

Beispiel:

```text
NPC: Borin
state: TRAVELLING
route: city_to_raid_04
startedAt
expectedArrival
progress
```

Wenn kein Spieler in der Umgebung ist:

* Reise abstrakt berechnen
* keine permanente Pathfinding-Simulation notwendig

Betritt ein Spieler das Gebiet:

* aktuelle Position anhand der Route und Reisezeit bestimmen
* NPC dort materialisieren
* sichtbare Bewegung übernehmen

Dadurch soll das System auch mit vielen NPCs skalierbar bleiben.

---

# 10. Übergang in einen Raid

Erreicht Borin den Raid-Eingang:

```text
World NPC
↓
Raid Transition
↓
Raid NPC
```

Es handelt sich weiterhin um denselben NPC.

Kein Klon erzeugen.

Nach dem Raid:

```text
Raid
↓
verlässt Raid
↓
RETURNING
↓
reist zurück
↓
Schmiede
↓
WORKING / IDLE
```

Während Borin weg ist, ist seine Schmiede entsprechend nicht durch ihn besetzt.

---

# 11. Andere NPCs reagieren auf seine Abwesenheit

Andere NPCs sollen seinen Zustand kennen können, wenn es logisch ist.

Beispiel:

Ein Spieler kommt zur Schmiede.

Borin ist unterwegs.

Ein Lehrling könnte über die Runtime-KI sagen:

> Borin ist vor einer Weile Richtung Nordstraße aufgebrochen.

Wichtig:

Der Lehrling darf dies nur sagen, wenn er entsprechende Informationen besitzt.

---

# 12. Runtime-KI-Aufgabe (lokal aktuell: Ollama; providerunabhängig, siehe `Coordinator.md` §3.1)

Die Runtime-KI ist NICHT die Datenbank und NICHT die Wahrheit der Welt.

Die Runtime-KI darf verwendet werden für:

* natürlich formulierte Dialoge
* Persönlichkeit
* Reaktionen auf bekannte Informationen
* Auswahl zwischen serverseitig erlaubten Aktionen
* begrenzte soziale Entscheidungen

Der Gameserver entscheidet:

* NPC-Position
* NPC-Zustand
* Wissen
* Reputation
* Inventar
* Reisezeit
* Verfügbarkeit
* Raid-Zugehörigkeit
* Tod
* Straftaten
* erlaubte Aktionen

Die Runtime-KI bekommt ausschließlich relevante strukturierte Informationen.

Beispiel:

```text
NPC: Borin
Personality: direct, loyal, gruff
CurrentState: TRAVELLING
Destination: Ancient Ruins
KnownFacts:
- Stammkunde Arin befindet sich dort in einem Raid
- Bote Mira hat diese Information überbracht
RelationshipWithArin: 82

Player asks:
"Wo willst du hin?"
```

Die Runtime-KI darf daraus natürliche Sprache erzeugen.

Es darf aber keine neuen Weltfakten erzeugen.

---

# 13. Events statt harter Kopplung

Die Systeme möglichst über Events bzw. klar definierte Schnittstellen verbinden.

Beispiel:

```text
RaidNeedsHelp
MessengerDispatched
MessageDelivered
NPCStartedTravel
NPCArrived
NPCEnteredRaid
NPCLeftRaid
NPCWitnessedCrime
CrimeReported
NPCDied
```

Nicht:

```text
raidManager ruft direkt 14 Funktionen aus npcManager auf
```

Ziel ist eine wartbare Architektur, in der neue NPC-Verhaltensweisen später ergänzt werden können.

---

# 14. Persistenz

Prüfe, welche Zustände gespeichert werden müssen, damit ein Serverneustart keine inkonsistente Welt erzeugt.

Beispielsweise:

* NPC-Position bzw. Aufenthaltsort
* NPC-State
* Beziehungen
* langfristiges Wissen
* aktive Reisen
* aktive Reservierungen
* wichtige Ereignisse

Erinnerungen werden beim Coordinator in einem separaten persistenten Speicherbereich dateibasiert gespeichert, nicht in der Realm-Datenbank.

Der Coordinator besitzt weiterhin keinen direkten Datenbankzugriff.

Dateien verwenden stabile interne IDs, keine Spielernamen.

Beispielsweise:

* persönliche Erinnerungen: `memories/characters/<character_id>/npcs/<npc_id>.json`
* Shared Knowledge: `memories/characters/<character_id>/shared_knowledge.json`

Temporäre Werte müssen nicht zwangsläufig dauerhaft gespeichert werden.

---

# 15. Performance

Das MMO soll später viele Spieler und NPCs unterstützen.

Deshalb:

* keine permanenten KI-Provider-Aufrufe für jeden NPC
* keine KI-Berechnung pro Server-Tick
* keine vollständige Pathfinding-Simulation für unbeobachtete NPCs
* Ereignisse bevorzugen
* KI nur dann aufrufen, wenn eine Entscheidung oder ein Dialog wirklich benötigt wird
* Ergebnisse sinnvoll zwischenspeichern, wenn möglich

Die grundlegende Weltlogik muss auch funktionieren, wenn der KI-Provider zeitweise nicht erreichbar ist.

---

# 16. Fehlerfälle

Berücksichtige mindestens:

* Bote stirbt
* Borin stirbt oder ist nicht verfügbar
* Raid endet, bevor Borin ankommt
* Borin ist bereits für ein anderes Ereignis reserviert
* Server startet während einer NPC-Reise neu
* der KI-Provider antwortet nicht
* Zielgebiet ist nicht verfügbar
* Spieler verlässt den Raid
* mehrere Spieler versuchen gleichzeitig denselben NPC auszulösen

In diesen Fällen darf kein NPC dupliziert oder in einem ungültigen Zustand zurückgelassen werden.

---

# Vorgehen

Arbeite in dieser Reihenfolge:

1. Bestehende Architektur vollständig analysieren.
2. Vorhandene NPC-, Event-, Raid-, Welt- und Persistenzsysteme identifizieren.
3. Prüfen, was davon wiederverwendet werden kann.
4. Einen modularen Architekturplan erstellen.
5. Geplante Dateien und deren Verantwortlichkeiten auflisten.
6. Prüfen, ob einzelne geplante Dateien zu viele Verantwortlichkeiten enthalten.
7. Erst danach implementieren.
8. Kleine, nachvollziehbare Schritte verwenden.
9. Nach jedem größeren Schritt prüfen, ob bestehende Funktionen weiterhin funktionieren.
10. Tests für zentrale Zustandsübergänge erstellen.
11. Keine unnötigen Systeme implementieren, die für die aktuelle Grundlage noch nicht benötigt werden.
12. Am Ende dokumentieren:

* neue Dateien
* geänderte Dateien
* Datenmodell
* Events
* Zustandsmaschine
* offene Punkte
* mögliche spätere Erweiterungen

## Besonders wichtig

**Keine große Datei bauen, die das komplette NPC-System enthält.**

Vor der Implementierung ausdrücklich zeigen:

```text
Datei/Modul → Verantwortung
```

Wenn du während der Implementierung bemerkst, dass ein Modul mehrere unabhängige Verantwortlichkeiten übernimmt, refaktoriere es in getrennte Module.

Die Architektur soll langfristig erweiterbar sein, weil später weitere NPC-Berufe, Beziehungen, Reisen, Ereignisse, Informationsketten und KI-gesteuerte Verhaltensweisen hinzukommen werden.
