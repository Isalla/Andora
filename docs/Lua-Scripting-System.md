# Andora – Lua-Scripting-System

## Status

**Konzept / Architekturspezifikation.**

Dieses Dokument ist die autoritative Spezifikation für die spätere Implementierung der serverseitigen Lua-Content-Scripting-Schicht von Andora.

Es wird **kein Code** beschrieben, der bereits existiert. Es wird **keine technische Festlegung** getroffen, die erst in der Implementierungsphase entschieden werden muss (siehe Abschnitt 22).

Lua ist in Andora die **Content- und Orchestrierungsschicht**. Lua ersetzt **keines** der bestehenden Systeme und wird **nicht** zur Autorität über Spielregeln.

---

## 1. Grundprinzip

> **Was eine Spielregel ist, gehört in die autoritativen Systeme. Was konkreter Content und dessen Ablauf ist, kann über Lua gesteuert werden.**

Der Realm bleibt für alle verbindlichen Spielregeln die Autorität, unter anderem für:

* Combat
* Charakterzustand
* Progression / EXP / Level
* Items
* Inventory
* Loot
* Queststatus und Persistenz
* NPC-/World-State
* Berechtigungen und Validierung

Lua beschreibt und orchestriert Content und fordert Aktionen über eine **kontrollierte API** an. Lua berechnet Spielergebnisse nicht eigenmächtig und schreibt nicht direkt den autoritativen Zustand.

**Beispiel Questbelohnung:**

```text
Lua fordert eine Questbelohnung an (z. B. EXP, Items, Gold).
        ↓
Rust validiert die Anforderung.
        ↓
Das bestehende Progression-/Item-/Inventory-System entscheidet
und führt die Änderung aus.
        ↓
Zustandsänderung / Event
```

Lua berechnet also nicht eigenmächtig Charakter-EXP und schreibt nicht direkt den Charakterzustand. Die bestehende Rust-Progression prüft und führt aus.

**Lua erhält keinen direkten Datenbankzugriff.**

---

## 2. Warum Lua

* Content kann ohne Änderungen am Realm-Kern erstellt werden.
* Quests, NPC-Verhalten, Weltinteraktionen und Cutscene-Abläufe können flexibel geändert werden.
* Kleine Content-Korrekturen erfordern nicht, dass Gameplay-Kernlogik neu geschrieben wird.
* Lua verbindet bestehende Systeme, ersetzt sie aber nicht.
* Die Architektur soll langfristig auch KI-gestützte Content-Erstellung ermöglichen, ohne dass eine Content-KI beliebigen Rust-Realm-Code verändern muss.

**Beispiel Cutscene:**

Timing, Reihenfolge, NPC-Bewegung, Dialog oder Kameraführung können später im Lua-Content angepasst werden, während die technische Cutscene-Engine unverändert bleibt.

Die technische Cutscene-Engine bleibt bei den zuständigen Engine-/Realm-/Client-Systemen. Lua beschreibt den konkreten Ablauf. Dies ermöglicht insbesondere, Cutscenes später leicht anzupassen, wenn Timing, Bewegung, Dialog, Kamera oder Reihenfolge beim Testen nicht gefallen.

Verwandt: `cutscene_system.md` und `ai_cutscene_system.md` definieren den Szenen- und Scene-Lock-Rahmen; dieses Dokument referenziert sie und verändert **keine** dort definierten Regeln.

---

## 3. Script-Domänen

Die folgenden Script-Arten sind vorgesehen.

### 3.1 Quest Scripts

Quest-Scripts steuern konkrete Questabläufe:

* Questannahme / Ablehnung
* sequenzielle Schritte
* parallele Schritte
* gemischte Abläufe
* dynamisches Aktivieren weiterer Schritte
* Completion-Callbacks
* Reaktion auf Questereignisse
* Abschluss / Belohnungsanforderung

**Queststatus, Validierung und Persistenz bleiben Rust-Aufgabe** (siehe `Quest-System.md` und Abschnitt 6).

### 3.2 NPC Scripts

NPC-Scripts steuern:

* Spawn
* Respawn
* Spielerinteraktion
* Hail / Ansprechen
* Dialogabläufe
* Reaktionen auf Queststatus
* NPC-spezifisches Verhalten

NPC-Zustand, Position, Wissen und Weltzugehörigkeit bleiben autoritativ in Rust bzw. den dafür zuständigen Systemen (siehe `Ki-NPC.md`).

### 3.3 Item Scripts

Item-Scripts steuern:

* Examine / Untersuchen
* Benutzen
* kontextabhängige Interaktionen
* Questbezug
* Reaktionen auf Weltzustand oder Nähe zu Weltobjekten

**Item- und Inventory-Zustand bleiben autoritativ in Rust** (siehe `item_properties.md`, `inventory_system.md`).

### 3.4 Ability Scripts

Ability-Scripts beschreiben konkrete Fähigkeiten und Effekte.

**Wichtige Klassenstruktur:**

Gemeinsame Fähigkeiten einer Grundklasse liegen auf Ebene der Grundklasse.

```text
abilities/
  fighter/
    Taunt.lua
    Kick.lua
    warrior/
      ...
    paladin/
      ...
```

Alle Kämpfer-Unterklassen erhalten die gemeinsamen Kämpferfähigkeiten wie Taunt und Kick. Krieger und Paladin erhalten zusätzlich ihre jeweiligen spezialisierten Fähigkeiten.

Dasselbe Prinzip soll später für die anderen Klassenfamilien möglich sein (Magier, Priester, Kundschafter; siehe `Klassensystem.md`).

> **Wichtig:** Die Ordnerstruktur dient der Organisation und darf NICHT alleinige autoritative Quelle für Klassenberechtigungen sein. Die tatsächliche Freigabe einer Fähigkeit muss durch autoritative Ability-/Klassendaten bzw. den Realm validiert werden.

**Combat-Regeln selbst bleiben in Rust.** Lua darf kontrollierte Combat-Aktionen anfordern, aber das bestehende Combat-System nicht umgehen (siehe `Kampfsystem.md`, `Ability-System.md`).

### 3.5 Zone Scripts

Zone-Scripts dienen zonenweiten Content-Ereignissen und Reaktionen auf Welt-/Gebietsereignisse.

### 3.6 Region Scripts

Region-Scripts steuern räumlich begrenzte Bereiche:

* Enter
* Leave
* Tick
* Umwelteffekte

Beispiele:

* Lava
* Giftnebel
* Kälte
* heilende Bereiche
* besondere Dungeonflächen

**Trennung von Geometrie und Verhalten:**

```text
Daten definieren WO die Region liegt.
Lua definiert WAS als Content dort geschieht.
```

Autoritative Auswirkungen wie Schaden werden über Rust-Systeme ausgeführt (`Kampfsystem.md`).

### 3.7 Interaction Scripts

Interaction-Scripts steuern spezielle Weltaktionen, zum Beispiel:

* Objekt untersuchen
* Gegenstand aufnehmen
* Zelt anzünden
* Totem zerstören
* spezielle Weltobjekte benutzen
* kontextabhängige Aktionen

### 3.8 Cutscene Scripts

Cutscene-Scripts orchestrieren konkrete Cutscene-Abläufe:

* Cutscene starten
* Kameraabläufe anfordern
* NPC-Bewegungen
* Dialoge
* Animationen
* zeitliche Abläufe
* Spawn-/Encounter-Anforderungen
* Übergänge zurück zum Spieler
* Questfortschritt nach oder während einer Cutscene

Die technische Cutscene-Engine bleibt bei den zuständigen Engine-/Realm-/Client-Systemen (siehe `cutscene_system.md`, `ai_cutscene_system.md`). Lua beschreibt den konkreten Ablauf.

---

## 4. Eventmodell

Das Lua-System ist **ereignisbasiert** aufgebaut. Lua-Scripte reagieren auf kontrollierte Events.

Die folgenden Namen sind **ausschließlich konzeptionelle Beispiele** und keine endgültigen API-Namen:

```text
player_enter_area
player_leave_area
npc_spawn
npc_respawn
npc_hail
npc_killed
item_examined
item_used
quest_accepted
quest_declined
quest_step_completed
quest_completed
ability_cast
player_login
player_logout
region_enter
region_leave
region_tick
cutscene_started
cutscene_finished
```

> Die endgültige technische API wird erst in einer späteren Implementierungsphase festgelegt. Diese Liste beschreibt lediglich, an welche Art von Ereignissen Lua angebunden werden kann.

Die bestehenden Systemdokumente verwenden für dieselben konzeptionellen Ereignisse teils andere konventionelle Schreibweisen (z. B. `AreaEntered`, `QuestCompleted`, `QuestProgressChanged` in `Quest-System.md` und `cutscene_system.md`). Beide Schreibweisen sind konzeptionell und stellen keine endgültige API dar.

---

## 5. Quest-Integration und Ablaufmodelle

Lua orchestriert konkrete Questabläufe. Der Realm besitzt und persistiert den autoritativen Questzustand.

### Sequenziell

```text
Step 1 -> Step 2 -> Step 3 -> Abschluss
```

### Parallel

```text
          -> Ziel A ->
Start     -> Ziel B -> Abschluss
          -> Ziel C ->
```

### Gemischt

```text
             -> Ziel B ->
Step A       -> Ziel C -> Step E -> Abschluss
             -> Ziel D ->
```

Lua definiert bzw. orchestriert solche Abläufe, einschließlich des dynamischen Aktivierens weiterer Schritte.

**Quest V1 selbst wird separat spezifiziert und implementiert** (siehe `Quest-System.md`, `quests_stories.md`). Dieses Dokument hält die benötigte Flexibilität für die Lua-Seite fest, definiert jedoch keine Quest-V1-Mechanik.

---

## 6. Sicherheits- und Autoritätsgrenzen

Dieser Abschnitt ist zentral für das Gesamtsystem.

### 6.1 Lua darf NICHT

* direkt auf MariaDB zugreifen
* beliebiges SQL ausführen
* beliebige Dateien lesen/schreiben
* Betriebssystembefehle ausführen
* den Realm-Prozess kontrollieren
* Charakterwerte direkt außerhalb kontrollierter APIs verändern
* EXP-Regeln umgehen
* Inventory-Regeln umgehen
* Loot-Regeln umgehen
* Combat-Regeln umgehen
* Klassenberechtigungen umgehen
* beliebigen Netzwerkverkehr öffnen
* Rust-Speicher direkt manipulieren

Lua bekommt ausschließlich **explizit freigegebene Realm-APIs**.

### 6.2 Prinzip

```text
Lua fordert Aktion an
        ↓
Rust validiert
        ↓
bestehendes autoritatives System
        ↓
Zustandsänderung / Event
```

### 6.3 Beispiel Quest-Step

Lua fordert „Quest Step abschließen" an.

Rust prüft mindestens:

* Quest existiert
* Charakter besitzt Quest
* Quest ist aktiv
* Step existiert
* Step darf in diesem Zustand abgeschlossen werden
* Fortschritt wird autoritativ persistiert
* notwendige Folgeevents werden erzeugt

---

## 7. Fehlerisolation (Architekturziel)

**Ein fehlerhaftes Lua-Script darf den Realm nicht zum Absturz bringen.**

Später vorzusehen:

* Scriptfehler werden geloggt
* kontrollierter Abbruch des betroffenen Script-Aufrufs
* keine unkontrollierte Weitergabe von Lua-Fehlern in den Realm
* Schutz vor Endlosschleifen / übermäßig langer Ausführung
* begrenzte Ressourcen pro Script-Aufruf
* nachvollziehbare Script-/Datei-/Event-Angabe in Fehlermeldungen

> Die konkreten technischen Limits (z. B. Instruction Limits, Memory Limits) werden erst bei der Implementierung festgelegt und dürfen in diesem Dokument nicht erfunden werden.

---

## 8. Grundarchitektur: Realm → Event Queue → Dispatcher → Worker-Pool → Lua-VM → Request Queue → Realm

Die Abschnitte 8 bis 19 beschreiben die **Runtime- und Worker-Architektur** der Lua-Content-Scripting-Schicht. Kern ist eine **Kaskade**, die Lua vollständig aus dem kritischen Realm-Pfad herauslöst.

```text
Realm
  -> Event Queue          (bounded, klassifiziert)
  -> Dispatcher           (Status-/Routing-Regeln)
  -> Lua Worker           (dedizierter Context, 1:1)
  -> Lua VM / ScriptManager
  -> Request Queue        (kontrollierte Rückmeldung)
  -> Realm                (validiert und führt AUTORITATIV aus)
```

Verbindliche Architekturregeln:

* Der Realm wartet **niemals synchron** auf Lua. Lua-Aufrufe laufen asynchron über die Event Queue.
* Lua erhält **keine mutablen Welt-/Realm-Referenzen** und **keine direkten VM-Speicherzugriffe** auf den autoritativen Zustand.
* Lua ist und bleibt **Content- und Orchestrierungsschicht**; Rust bleibt für Spielregeln und Spielzustand autoritativ.
* Spielrelevanter, persistenter oder spielregelrelevanter Zustand darf **niemals ausschließlich** in einer Lua-VM liegen. Er gehört in Rust und/oder die Realm-Datenbank.
* Über die **Request Queue** fordert Lua Aktionen an; die Validierung und Ausführung übernimmt der autoritative Realm (siehe Abschnitt 6).
* Fehler aus der Kaskade (Worker, VM, Script, Queue, Dispatcher) dürfen den Realm nicht destabilisieren (siehe Abschnitt 7).

> Konkrete technische Werte dieser Architektur (Thread-Anzahl, Limits, Queue-Größen) werden bewusst nicht erfunden und sind in Abschnitt 22 als offen markiert.

---

## 9. Worker-Pool-Kapazität

Der Worker-Pool besitzt konzeptionell **drei Kernwerte**:

| Parameter | Default | Bedeutung |
|---|---|---|
| `lua_worker_base_count` | `10` | Basis-Worker, die dauerhaft vorgehalten werden |
| `lua_worker_normal_reserved` | `4` | davon **geschützte** normale World-/Regular-Worker |
| `lua_worker_max_count` | `15` | Obergrenze inkl. temporärer Worker (Default, **kein** hartes Architekturlimit) |

Verbindliche Beziehung:

```text
lua_worker_normal_reserved <= lua_worker_base_count <= lua_worker_max_count
```

Semantik im Detail:

* **Basis-Worker (10):** dauerhaft vorgehaltener Kern des Pools.
* **Normal Reserved (4):** von diesen Basis-Workern sind 4 für **normale Weltlast reserviert**. Sie werden **niemals** für Raids, Hot-Zones oder andere Speziallast „gestohlen", auch dann nicht, wenn sie gerade unbeschäftigt sind. Diese geschützte Kapazität gibt der Normal-Welt eine garantierte Mindest-QoS.
* **Rest der Basis-Worker (6):** stehen der Speziallast zur Verfügung.
* **Temporäre Worker bis `max_count` (15):** dürfen bei zusätzlicher Speziallast temporär erzeugt werden.
* `lua_worker_max_count` ist **kein hartes Architekturlimit**, sondern ein **konfigurierbarer Default**. Stärkere Server dürfen höhere Werte konfigurieren (z. B. 20, 30 oder mehr). Der Default 15 ist die voreingestellte Konfigurarbeitsgrenze, keine Architekturgrenze.
* `lua_worker_normal_reserved` ist ebenfalls konfigurierbar.

> Bewusst nicht festgelegt: die konkrete Anzahl an Threads pro Worker sowie alle technischen Ressourcenlimits (siehe Abschnitt 22).

---

## 10. Dedicated Worker und Context-Isolation

Ein **Dedicated Worker** bedient während seiner Zuweisung **genau einen** Dedicated Context (1:1-Bindung).

```text
worker_id      = 8
context_type   = raid
context_id     = 4711
```

Regeln:

* Ein Dedicated Worker bearbeitet ausschließlich Events/Requests seines gebundenen **`context_id`**.
* Ein zweiter Context (z. B. ein zweiter Raid) wird niemals parallel auf demselben Dedicated Worker ausgeführt.
* Falsch geroutete Events für einen anderen `context_id` werden **abgelehnt und geloggt**.
* Es existiert **kein VM-lokaler Zustand**, der zwischen zwei unterschiedlichen Dedicated Contexts überleben darf.
* Die Context-Isolation gilt explizit auch zwischen Raids untereinander: Fehler in einem Raid dürfen einen anderen Raid nicht über eine gemeinsame VM beschädigen.

### Technische Mindestregel: Context-Mismatch vor der Zustellung

Bei einem Dedicated Worker muss die **Context-Zuordnung vor der Zustellung von Events/Requests in dessen Worker-Inbox** geprüft werden.

Stimmt der Context des Events/Requests **nicht** mit dem gebundenen Dedicated Context des Workers überein, gilt:

* das Event/der Request wird **nicht** in die Worker-Inbox eingereiht,
* es wird **nicht** von Lua ausgeführt,
* es wird eindeutig als **Context-Mismatch** abgelehnt,
* der aufrufende technische Pfad erhält einen eindeutigen `ContextMismatch`-Fehler/Outcome.

Zusätzlich erfolgt eine **minimale Rust-seitige Protokollierung** des Mismatch. Mindestens protokollierbar sind:

* `worker_id`,
* erwarteter Context,
* empfangener Context.

Dies ist normales technisches Logging. **Nicht** Bestandteil dieser Regel: Telemetrie-System, Incident-Auswertung, automatische Eskalation, ein Worker-Reset aufgrund eines einzelnen Mismatch, Schuldzuweisung und Failover (Telemetrie-Integration in Stufe 5; Watchdog/Failover in Abschnitten 16 und 18).

Ziel der 1:1-Ensembles:

> **Ein problematischer Raid darf nicht den nächsten Raid über dieselbe Lua-VM beschädigen.**

---

## 11. Worker-Lebenszyklus

Jeder Worker durchläuft konzeptionell einen Lebenszyklus. Die Übergänge werden serverseitig (Rust) kontrolliert.

```text
IDLE --> RESERVED --> ACTIVE --> DRAINING --> [DESTROYING | RESETTING]
  ^          |            |          |
  |          +------------+----------+
```

* **IDLE** – Worker ist im Pool verfügbar, kein Context gebunden.
* **RESERVED** – Worker ist einem Dedicated Context (z. B. Raid) **exklusiv** zugeordnet; die Context-Bindung besteht bereits. Der Worker befindet sich noch in der **Vorbereitung** und verarbeitet noch **keine** Events dieses Contexts.
* **ACTIVE** – Worker verarbeitet **ausschließlich** Arbeit seines gebundenen Dedicated Contexts.
* **DRAINING** – technische **Abwicklungsphase** des Workers beim Beenden der Lua-Verarbeitung eines Contexts durch Rust. Ab DRAINING werden **keine neuen Lua-Gameplay-Events** für diesen Context mehr angenommen; bereits akzeptierte, noch gültige Lua-Arbeit wird zu Ende verarbeitet. Nach Ablauf eines **Drain-Timeout** wird der Context endgültig geschlossen.
* **RESETTING** – VM-/Runtime-Zustand wird vollständig verworfen (frische VM).
* **DESTROYING** – Worker wird nach Ende eines temporären/Notfall-Einsatzes entfernt.

### Übergänge RESERVED → ACTIVE → DRAINING

* **IDLE → RESERVED:** Der Worker wird einem Dedicated Context **exklusiv** zugeordnet; die Context-Bindung besteht ab diesem Zeitpunkt. In RESERVED befindet sich der Worker noch in der Vorbereitung und verarbeitet noch **keine** Events des Contexts.
* **RESERVED → ACTIVE:** erfolgt **erst**, wenn
  * die Initialisierung/der Reset **vollständig abgeschlossen** ist,
  * eine **frische Lua-VM** für diesen Context bereitsteht und
  * der Worker für die Verarbeitung dieses Contexts **bereit** ist.
* **ACTIVE:** Der Worker verarbeitet ausschließlich Arbeit seines gebundenen Dedicated Contexts.
* **ACTIVE → DRAINING:** erfolgt, wenn Rust die Lua-Verarbeitung des Contexts **beendet**. Ab DRAINING werden **keine neuen Lua-Gameplay-Events** für diesen Context mehr angenommen.

### Abschluss des normalen Drains

Der normale erfolgreiche Drain ist abgeschlossen, sobald **keine bereits akzeptierte Lua-Arbeit dieses Contexts mehr aussteht**.

Danach gilt:

* Basis-/wiederverwendbarer Worker: `DRAINING → RESETTING → IDLE`.
* Temporärer Worker: `DRAINING → DESTROYING`.

Für **RESETTING** gilt weiterhin:

* Der alte VM-/Script-Zustand wird **vollständig verworfen**.
* Es darf **kein Context-Zustand** die Wiederverwendung überleben.
* Vor einer späteren neuen Dedicated-Bindung wird eine **frische VM** verwendet.

> `DRAINING` ist **nicht** die 10-minütige Raid-Postclear-Phase: DRAINING ist ausschließlich die technische Abwicklungsphase des Lua-Workers (siehe Abschnitt 14). Diese Festlegung regelt nur den normalen erfolgreichen Drain. Ausdrücklich **offen** bleiben: das Verhalten beim Drain-Timeout, der erzwungene Abbruch mit verbliebener Arbeit sowie die Behandlung verbliebener Reliable Events bei einem erzwungenen Drain (siehe Abschnitt 12); Watchdog-Eskalation und Failover folgen in Stufe 4 (Abschnitte 16 und 18).

Regeln zum Lebenszyklus:

* Nach dem Ende eines Dedicated Contexts wird der **alte VM-/Runtime-Zustand vollständig verworfen**.
* Vor der Zuweisung an den nächsten Context wird eine **frische VM** erzeugt. Es wird kein VM-021-Zustand zwischen Contexts wiederverwendet.
* Temporäre Worker werden nach Beendigung ihres Dedicated Contexts **zerstört**.
* Basis-Spezial-Worker werden nach der Beendigung ihres Contexts **komplett zurückgesetzt** (frische VM), bevor sie wieder in `IDLE` übergehen.

> Die konkrete Dauer des Drain-Timeout bleibt offen (siehe Abschnitt 21 / 22).

---

## 12. Event Queue und QoS-Klassen

Events zwischen Realm und Lua laufen über **bounded Queues** (keine unbegrenzten Queues).

Konzeptionelle Event-Klassen:

| Klasse | Semantik |
|---|---|
| `Reliable` | darf nicht verloren gehen |
| `Coalescable` | darf mit gleichartigen Events zusammengefasst werden (z. B. Tick-Events) |
| `Droppable` | darf unter Last verworfen werden (z. B. veraltete Positions-/Tick-Updates) |

Regeln:

* **Die Klassenzuordnung wird vom Rust-Eventproduzenten vorgenommen**, nicht von Lua. Lua kann nicht selbst bestimmen, ob ein Event zuverlässig ist.
* Die konkrete Semantik des Event-Typs (was „zuverlässig" konkret bedeutet) ergibt sich aus der eventuell genaueren Definition in `Eventmodell` benachbarter Systeme bzw. wird in der Implementierung entschieden (siehe Abschnitt 21/22).
* Queue-Tiefe und Queue-Latenz sind **monitoringpflichtig** (siehe Abschnitt 19 und `Telemetrie_und_Analyse.md`).
* Es gibt **keine persistente Disk-Queue** als Default für die Lua-Orchestrierung. Temporäre Queuedaten verbleiben im Arbeitsspeicher; Realm-Zustand bleibt in Rust/MariaDB.

> Exaktes Verhalten bei einer vollständig gefüllten `Reliable`-Queue eines **normalen** Workers: siehe hierzu die verbindliche Overflow-Regel unten.

### Verhalten bei vollständig gefüllter Queue (Overflow)

Für **normale World-/Regular-Worker** ist Folgendes verbindlich beschlossen:

* Ist die Queue eines normalen Workers **ausgelastet/voll**, verteilt der **Dispatcher/Scheduler** weitere dafür geeignete Arbeit nach Möglichkeit auf einen **anderen zulässigen, freien bzw. verfügbaren Worker des normalen Worker-Pools**.
* Die vorhandenen **Idle-/freien Worker** dienen damit auch dazu, **Lastspitzen bzw. Überlauf einzelner normaler Worker abzufangen**.
* **Reliable Events werden dabei nicht wie Droppable Events verworfen.**
* Der Realm wartet weiterhin **niemals synchron** auf Lua.
* Die bestehende **Worker-Pool-Konfiguration und die Reservierungsregeln bleiben bestehen** (siehe Abschnitt 9); die vier dauerhaft für die normale Welt reservierten Worker bleiben für die normale Welt reserviert.
* Diese Regel erfindet **keine neue Worker-Klasse** und **keinen neuen Lifecycle-Zustand** (siehe Abschnitt 11).

> Die konkreten technischen Umsetzungsdetails (Queue-Größen, Auswahl- und Verteilungsalgorithmus des Dispatchers) bleiben offen (siehe Abschnitt 22).

**Grenze: Dedicated Context.** Die Overflow-Regel gilt **nicht** für Dedicated Workers:

* Ein Dedicated Worker bedient weiterhin **genau einen** Dedicated Context (1:1, siehe Abschnitt 10).
* Events/Requests eines Dedicated Context werden **nicht** zur Entlastung auf einen Worker eines anderen Dedicated Context verteilt.
* Die bestehende **Context-Mismatch-Regel bleibt unverändert**: falsch geroutete Events für einen anderen `context_id` werden abgelehnt und geloggt (technische Mindestregel zur Prüfung vor der Zustellung: siehe Abschnitt 10).

> **Sonderfall offen:** Das Verhalten bei einer vollständig ausgelasteten `Reliable`-Queue eines Dedicated Context bzw. Dedicated Workers ist noch **nicht entschieden** und bleibt ausdrücklich **offen**. Bewusst **nicht** festgelegt werden in diesem Dokument: Events verwerfen, den Realm blockieren, einen zweiten Worker auf denselben Dedicated Context verteilen, Emergency Worker starten, den Context zurücksetzen oder einen Raid abbrechen. Derartige Entscheidungen benötigen eine **separate Architekturentscheidung**.

---

## 13. Raid-Worker und Raid-Queue

Raid-Instanzen können als **Dedicated Contexts** einem einzelnen Dedicated Worker zugeordnet werden.

```text
raid_instance_id <-> genau ein Dedicated Worker (1:1)
```

Regeln:

* Eine Raid-Instanz erhält genau **einen** Dedicated Worker (Kid).
* Der Worker ist über die `raid_instance_id` fest an diese Instanz gebunden.
* **Kein „Stehlen" von Normal-Workern:** Wird `lua_worker_max_count` erreicht, so wird **keine neue Raid-Instanz gestartet** und es wird auch **kein** geschützter Normal-Arbeiter genommen. Die Gruppe wartet stattdessen in der **Raid-Queue**.
* Die Position in der Raid-Queue ist für Spieler **sichtbar** (z. B. „Raid-Queue: 2").
* Die konkrete Wartezeit wird **nicht** vorgetäuscht: Für Positionen hinter 1 wird **keine erfundene Startzeit** angezeigt. Lediglich für Position 1 kann, wenn absehbar ein Worker frei wird, eine **voraussichtliche** Wartezeit angezeigt werden.

Sobald ein Worker verfügbar ist, wird er **beim Erreichen der Queue-Position 1 bereits fest an die neue Raid-Instanz gebunden** (s. Abschnitt 15 zur Eintrittsphase).

---

## 14. Raid-Postclear (Nachlaufphase)

Nach dem Sieg über den letzten Boss-Encounter beginnt die **Postclear-Phase** der Raid-Instanz.

Regeln:

* Der **Postclear-Timer beträgt als Maximum 10 Minuten** (Default, konfigurierbar). Er ist eine **Höchst-/Nachlaufzeit**, keine Mindestdauer.
* Während der Postclear-Phase dürfen Spieler in der Instanz bleiben: Loot einsammeln, Truhen öffnen, Gruppe formen, kurz kommunizieren.
* **Befindet sich währenddessen bereits eine neue Gruppe in der Raid-Queue**, wird die Wartezeit durch einen früheren Abschluss verkürzt.

### Lua-Worker während der Postclear-Phase

Die Raidinstanz benötigt während der Postclear-Phase **nicht weiterhin** ihren Dedicated Lua Worker.

* Nach erfolgreichem Lua-`DRAINING` kann der Dedicated Worker **zurückgesetzt** bzw. bei einem temporären Worker **zerstört** werden (siehe Abschnitt 11).
* Die verbleibende Postclear-Instanz wird **autoritativ durch Rust/Realm** verwaltet.
* Die Postclear-Phase darf **keinen Lua Worker nur deshalb 10 Minuten lang reservieren**.

Während der Postclear-Phase:

* **erlaubt** bleiben insbesondere: Loot-Verteilung und notwendige Loot-Interaktionen, Bewegung innerhalb der verbleibenden Instanz, Verlassen der Instanz sowie normale nicht-kampfbezogene Kommunikation/Interaktionen, soweit sie nicht aus anderen Regeln ausgeschlossen sind;
* **deaktiviert** sind: Combat, Angriffe sowie gameplaywirksame Kampffähigkeiten bzw. Fähigkeiten, die eine aktive Encounter-/Lua-Verarbeitung voraussetzen.

> `DRAINING` ist **nicht** die 10-minütige Raid-Postclear-Phase: DRAINING ist ausschließlich die technische Abwicklungsphase des Lua-Workers (siehe Abschnitt 11). Beim Wechsel `ACTIVE → DRAINING` werden keine neuen Lua-Gameplay-Events für diesen Context mehr angenommen; bereits akzeptierte, noch gültige Lua-Arbeit wird abgearbeitet.

**Vorzeitiger Abschluss:** Verlassen **alle** Spieler die Instanz vor Ablauf des Postclear-Timers, wird die Instanz sofort geschlossen:
* Worker wird gedrained/zerstört bzw. zurückgesetzt,
* der nächste wartende Raid kann früher starten.

**Timer-Ablauf ohne vorheriges Verlassen:** Nach Ablauf des Timers werden verbliebene Spieler sicher aus der Instanz (an einen sicheren Ausgangspunkt) geleitet, die Instanz wird geschlossen und der Worker freigegeben/zerstört.

> Dieses Dokument legt den **Instanz-/Worker-Lebenszyklus** des Postclear fest, nicht Raidspielregeln oder Bossspezifika (siehe Raid-/Boss-System; `Boss-System.md` Abschnitt 5).

---

## 15. Raid-Ready-/Eintrittsphase

Sobald ein Dedicated Worker für die Queue-Position 1 verfügbar wird, ist er **bereits fest an die neue Raid-Instanz gebunden** und die **Eintrittsphase (Ready-Phase)** beginnt. (Diese feste Bindung entspricht dem Zustand `RESERVED`; die Verarbeitung beginnt erst mit `ACTIVE`, siehe Abschnitt 11.)

Regeln:

* Die Ready-Phase dauert **5 Minuten** (Default, entschieden).
* Der Spieler sieht eine **UI-Anzeige** mit dem Status „Raid ist bereit" und einem **Countdown**.
* Der Spieler kann über einen Button **„Raid jetzt beitreten"** sofort eintreten.
* Klickt der Spieler **nicht**, wird er nach Ablauf der 5 Minuten **automatisch in die Raid-Instanz teleportiert**.

Begründung:
* Spieler werden nicht abrupt aus ihrer aktuellen Situation gerissen.
* Ein Raid startet spätestens nach Ablauf der Ready-Phase – es gibt **keine unbegrenzte Wartezeit** auf säumige Spieler.

> 5 Minuten sind als **Design-/Architekturwert entschieden**. Es werden hier bewusst **keine zusätzlichen** Timeout-/Kick-/Bestrafungsregeln für Spieler erfunden (siehe auch Abschnitt 21/22, offene Punkte zu Fairness und Disconnect).

---

## 16. Watchdog (Rust-seitige Schutzschicht)

Jeder Worker erhält eine **Rust-seitige Watchdog-/Überwachungskomponente**. Kein Lua-Script überwacht Lua selbst.

Der Watchdog überwacht pro Worker konzeptionell:

* Laufzeit eines einzelnen Lua-Aufrufs
* Instruktionsverbrauch
* VM-Speicher
* Queue-Tiefe / Queue-Latenz
* aktuell ausgeführtes Script (+ Worker-/Context-ID)
* Fehlerhäufigkeit
* Uptime / Context-Dauer

> Konkrete Limits (z. B. Instruktionsobergrenze, Memory-Limits, Laufzeit in ms) werden **erst in der Implementierung** festgelegt und dürfen in diesem Dokument nicht erfunden werden (siehe Abschnitt 7 und Abschnitt 22).

**Eskalationsleiter des Watchdogs:**

1. **Einzelner Fehler:** nur der betroffene Lua-Aufruf wird abgebrochen; Fehler und Kontext werden geloggt; der Worker läuft weiter. **Nicht jeder Scriptfehler tötet sofort den Worker.**
2. **Wiederholte Fehler / nicht vertrauenswürdige VM:** VM wird zurückgesetzt oder der Worker hochgestuft (siehe Abschnitt 17).
3. **Dedicated Worker dauerhaft instabil:** Worker wird als `FAILED` markiert, **Failover** wird eingeleitet (siehe Abschnitt 18).

---

## 17. Kombinationsfehler und Diagnosekontext

Fehler können **nicht** nur in einem einzelnen Script entstehen. Häufig sind mehrere Faktoren beteiligt:

* mehrere Lua-Scripts
* Event-Reihenfolge
* Spieleraktionen
* Rust-Systeme
* Client-Eingaben
* Routing/Queue
* seltene Zustandskombinationen („Race"-artige Abläufe durch Parallelität)

**Diagnosekontext (pro Worker/Context):**

* kleiner **Ringpuffer / kurze Diagnosehistorie** der letzten relevanten Ereignisse (Scriptaufrufe, Events, Requests, Validierungsfehler, Zustandsübergänge)
* beim Incident wird dieser Kontext **dauerhaft ins Incident-/Recovery-Log** übernommen (siehe `Telemetrie_und_Analyse.md`)
* Später kann eine KI statt einer sofortigen Ursachensuche Korrelationen und Auffälligkeiten **markieren** (siehe `Telemetrie_und_Analyse.md` Abschnitt 3)

**Wichtig:** Es wird **kein automatischer „Schuldiger"** bestimmt und **nichts automatisch entschädigt** (siehe `Telemetrie_und_Analyse.md` Abschnitt 2/3).

---

## 18. Failover

Ziel: Bei einem schweren Worker-Ausfall soll ein laufender Dedicated Context (z. B. Raid) auf einen **Ersatzworker** übergehen, ohne die Spieler zu verlieren.

**Wichtigste Regel:**

> Beim Failover bleibt die Raid-Instanz dieselbe. **Geändert wird nur die Worker-Bindung**, nicht die Instanz.

```text
raid_4711  -> worker_8    (vorher)
raid_4711  -> worker_16   (nach Failover)
```

* Die Spieler bleiben logisch in derselben Raidinstanz. Es entsteht **keine neue Instanz**.
* Der neue Worker erhält eine **frische Lua-VM** (der alte VM-Zustand ist unbrauchbar). Der Context wird aus dem autoritativen Realm-/DB-Zustand **neu aufgebaut** – nicht aus dem beschädigten VM-Speicher.
* Das ist nur deshalb möglich, weil der maßgebliche Zustand **nicht in Lua liegt** (siehe Abschnitt 8).

**Failover darf über `max_count` hinausgehen:**

* Ein laufender Dedicated Context hat **Recovery-Priorität**.
* Ist `lua_worker_max_count` bereits erreicht, darf ein **Notfall-Worker über `max_count`** kurzzeitig genutzt werden.
* **Einschränkung:** Solche Notfall-Worker dienen **ausschließlich** der Recovery/Failover bereits laufender Contexts. Sie starten **niemals** neue Raids, neue Speziallast oder normale Weltlast.
* Es gilt **maximal 3 Failover-Versuche** (Default, konfigurierbar: `lua_failover_max_attempts = 3`), **pro laufendem Dedicated Context**, nicht global pro Server.

**Wenn alle 3 Failover-Versuche fehlschlagen:**

* Der Context wird als `FAILED` markiert.
* Es werden **keine weiteren Lua-Events** an den Context gesendet.
* Encounter/Kampf wird serverseitig **kontrolliert** beendet.
* Spieler werden informiert und sicher zur Instanzgrenze (Raid-Ausgang) teleportiert.
* Die Instanz wird geschlossen/abgeräumt, der Worker entfernt.
* Das **Incident-/Recovery-Log** wird abgeschlossen (siehe `Telemetrie_und_Analyse.md`).

> So bleibt kein Spieler dauerhaft in einer defekten Instanz gefangen, ohne auf einen GM warten zu müssen.

---

## 19. Monitoring des Worker-Pools

Das Ziel des Monitorings ist ausdrücklich **nicht** nur „läuft ja / läuft nicht", sondern die Frage **warum** eine Last besteht.

**Übersicht des gesamten Pools:**

* Basis-Worker (gesamt), belegte/freie Normal-Arbeiter, aktive/idle/Notfall-Worker
* `max_count`-Auslastung (aktiv vs. konfiguriert)
* Anzahl laufender Dedicated Contexts
* Raid-Queue-Länge (Wartepositionen)
* Zuordnungs-/Failover-Fehler
* Ressourcen-Spitzen (Peak)

**Pro Worker anzeigbar:**

* `worker_id`, Status, Typ (Normal / Spezial / Raid-Dedicated)
* `context_id` und `raid_instance_id` (bei Raid)
* Auslastung/Last
* Queue-Tiefe/-Latenz
* Ereignisse pro Sekunde
* Script-Laufzeiten
* Fehler
* Uptime / Context-Dauer
* Failover-Status

**Detailansicht (anklickbar) bis auf Domänen-/Script-Ebene:**

```text
Worker 8 – Raid 4711  [ACTIVE]
  Last gesamt:        97 %
  Abilities:          51 %
  NPC/Boss:           34 %
  Regionen:           10 %
  Cutscenes:           5 %
  Script-Spitzenreiter: shadow_wave.lua, summon_minions.lua
```

So kann ein Admin erkennen, **welches Script welche Last verursacht**, nicht nur, dass ein Worker stark belastet ist.

Weitere Details zur Monitoring-Infrastruktur und zur allgemeinen Telemetrie/Incident-Analyse: `Telemetrie_und_Analyse.md` und `monitoring_web_panel.md`.

---

## 20. Script-Organisation

Konzeptionelle Zielstruktur (noch keine unveränderliche technische API):

```text
scripts/
  quests/
  npcs/
  items/
  abilities/
    fighter/
      warrior/
      paladin/
    mage/
    priest/
    scout/
  zones/
  regions/
  interactions/
  cutscenes/
  shared/
```

Die Struktur beschreibt die geplante Content-Organisation. Gemeinsam nutzbare Lua-Helfer können unter `shared/` liegen, sofern dies später sicher umgesetzt wird.

---

## 21. Content-Änderungen und Reload (Ziel)

Ziel ist, dass Lua-Content leichter austauschbar und testbar ist als fest kompilierte Realm-Kernlogik.

> Dieses Dokument behauptet **nicht**, dass echtes Live-Hot-Reload bereits technisch beschlossen oder implementiert ist.

Festgehalten als Ziel:

* möglichst geringe Hürde für Content-Änderungen
* Lua-Scripte sollen unabhängig von Änderungen am Rust-Quellcode bearbeitet werden können
* Reload-/Cache-/Versionsstrategie wird in der späteren technischen Spezifikation entschieden
* laufende Quest-/Cutscene-Zustände müssen bei einem späteren Reload-Konzept berücksichtigt werden

---

## 22. Abgrenzung: Jetzt festgelegt / Erst Implementierung

Dieses Dokument unterscheidet ausdrücklich zwischen Architekturziel und späterer Implementierung.

### 22.1 Jetzt festgelegt

* Lua ist serverseitige Content-Scripting-Schicht
* Rust bleibt autoritativ
* Script-Domänen (Abschnitt 3)
* Sicherheitsprinzip (Abschnitt 6)
* Eventprinzip (Abschnitt 4)
* Klassen-/Ability-Hierarchie (Abschnitt 3.4)
* Cutscene-Orchestrierung (Abschnitt 3.8)
* Quest-Integration (Abschnitte 3.1, 5)
* Regions-/World-Integration (Abschnitte 3.5, 3.6)
* Fehlerisolation als Architekturziel (Abschnitt 7)
* Script-Organisation als konzeptionelle Zielstruktur (Abschnitt 20)

### 22.2 Noch NICHT festlegen bzw. erfinden

Folgende Entscheidungen erfolgen in der späteren Implementierungsphase und werden in diesem Dokument bewusst **nicht** getroffen:

* konkrete Lua-Runtime / Library für Rust
* endgültige API-Funktionsnamen
* Instruction Limits
* Memory Limits
* Threading-Modell
* Hot-Reload-Implementierung
* genaue Cache-Strategie
* konkrete Performance-Limits
* endgültiges Dateiformat für Script-Metadaten
* konkrete Netzwerkprotokolländerungen

> Hinweis: Im Repository liegt unter `deps/lua/` eine referenz-vendordierte Lua-5.5.1-Quelle (unverändert, **nicht** eingebaut; siehe `deps/README.md` und `docs/README.md`). Das ist eine reine Referenzquelle und **keine** Festlegung auf eine konkrete Rust-Anbindungsbibliothek oder Integrationsart. Die endgültige Runtime-Entscheidung bleibt der Implementierungsphase vorbehalten.

---

## 23. Beziehung zu bestehenden Systemen

| System | Dokument | Bezug zum Lua-Scripting-System |
|---|---|---|
| Quest-System | `Quest-System.md`, `quests_stories.md` | Lua beschreibt Questdefinitionen und orchestriert Abläufe; Realm prüft und persistiert Questzustand |
| Cutscene / Scene | `cutscene_system.md`, `ai_cutscene_system.md` | Lua orchestriert konkrete Cutscene-Abläufe; Scene Engine / Realm bleiben technisch zuständig |
| Kampf | `Kampfsystem.md` | Content-/Lua-definierter Gegner-Grundzustand; Combat-Regeln bleiben in Rust |
| Ability | `Ability-System.md` | Content-/Lua-definierte Fähigkeiten, Qualitätswerte, Meisterschaftsvarianten; Realm führt aus |
| Klassen | `Klassensystem.md` | Fähigkeitshierarchie je Grund-/Unterklasse; Klassenberechtigungen werden durch autoritative Daten validiert |
| Inventory / Items | `inventory_system.md`, `item_properties.md` | Lua steuert Item-/Interaction-Content; Item- und Inventory-Zustand bleibt autoritativ in Rust |
| NPC | `Ki-NPC.md` | NPC-Scripts steuern Content-Verhalten; NPC-Zustand, Wissen, Beziehungen bleiben autoritativ |
| Architektur | `architecture.md`, `project_overview.md` | Lua als Content-/Scripting-Schicht innerhalb der serverautoritativen Architektur |
| Status | `Projekt-Status.md` | Lua-Scripting ist Konzept/Architekturziel; es existiert kein implementierter Lua-Content-Layer |