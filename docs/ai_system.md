# Andora – AI System

## Status

**Architektur / Planung**

Dieses Dokument beschreibt die zentrale KI-Architektur von Andora.

Spezialisierte Systeme wie NPC-KI, Dynamic Scene System oder AI-Crafting besitzen eigene Dokumentationen und bauen auf diesen gemeinsamen Regeln auf.

---

# 1. Grundprinzip

KI ist in Andora eine zusätzliche Interpretations-, Dialog- und Erzählebene.

Sie ist **nicht die Autorität über die Spielwelt**.

Grundregel:

> **Der Server bestimmt, was wahr ist und was passieren darf. Die KI interpretiert, formuliert und reagiert innerhalb dieses Rahmens.**

Die Spielwelt muss vollständig weiterlaufen können, wenn das KI-System nicht verfügbar ist.

---

# 2. Architektur

Grundlegender Kommunikationsweg:

```text
Godot Client
      ↓
Andora Gameserver
      ↓
AI Service
      ↓
KI-Provider
(lokal: Ollama)
      ↓
AI Service
      ↓
Andora Gameserver
      ↓
Godot Client
```

Der Client kommuniziert niemals direkt mit einem KI-Provider.

Der Gameserver kontrolliert:

* welche KI-Anfragen erlaubt sind
* welche Informationen an die KI gehen
* welche KI-Antworten verwendet werden
* welche Aktionen daraus entstehen dürfen

---

# 3. Aufgabenverteilung

## Realm-Server (Rust)

Der Realm-Server (Rust) ist für die Spielmechanik und Autorität verantwortlich.

Dazu gehören:

* Weltzustand
* Spielerzustand
* NPC-Zustand
* Positionen
* Combat
* Inventory
* Items
* Crafting
* Quests
* Beziehungen
* Knowledge
* Events
* Reisen
* Gruppen
* Instanzen
* Szenen
* Validierung
* Berechtigungen

Die KI darf diese Systeme nicht direkt umgehen.

---

## Lua

Lua definiert Gameplay-Inhalte und KI-spezifische Content-Regeln.

Dazu können gehören:

* NPC-Persönlichkeiten
* NPC-Prompt-Fragmente
* Dialogregeln
* Questdefinitionen
* Itemdefinitionen
* Rezepte
* Eventdefinitionen
* Scene Scripts
* feste Dialoge
* Narrative Regeln
* erlaubte AI-Kontexte

Lua führt kein beliebiges SQL aus und verändert Weltzustände ausschließlich über kontrollierte Server-APIs.

---

## MariaDB

MariaDB speichert persistente Zustände.

Beispiele:

* Charaktere
* Inventare
* Item-Instanzen
* Questfortschritt
* Beziehungen
* NPC-Zustände
* NPC-Wissen
* Reisen
* wichtige Ereignisse
* Story Flags
* World States
* Hochzeiten
* Statistiken

Die KI erhält keinen direkten Datenbankzugriff.

---

## KI-Provider

Der KI-Provider (im lokalen Standardbetrieb: Ollama) verarbeitet ausschließlich vom AI-Service vorbereitete Aufgaben.

Mögliche Aufgaben:

* NPC-Dialog
* Interpretation natürlicher Sprache
* persönliche Dialogvarianten
* Narrative Texte
* Scene-Dialog
* Crafting-Wünsche interpretieren
* situationsabhängige Reaktionen

Der KI-Provider ist niemals die Quelle des tatsächlichen Weltzustands.

## Providerunabhängigkeit

Andoras Runtime-KI ist langfristig nicht fest an Ollama oder einen einzelnen KI-Anbieter gekoppelt.

Der bestehende lokale Betrieb über Ollama bleibt ein unterstützter und zunächst bevorzugter Weg (Standard-/Basislösung). Andora muss weiterhin vollständig mit lokal betriebener KI arbeiten können; eine spätere Unterstützung externer Provider darf keine zwingende Cloud-Abhängigkeit erzeugen.

Die Architektur ermöglicht es, später auch externe KI-Provider über deren API anzubinden, ohne dafür Realm oder Gameplay-Systeme grundlegend umbauen zu müssen. Mögliche Beispiele sind lokale Modelle über Ollama, eine OpenAI- / ChatGPT-kompatible API, Anthropic sowie weitere zukünftige KI-Anbieter. Diese Nennungen beschreiben ausschließlich Erweiterungsmöglichkeiten und stellen keine Verpflichtung dar, diese jetzt zu implementieren oder dauerhaft zu unterstützen. Es wird hier noch keine konkrete Provider-API oder Implementierung festgelegt.

Der Realm muss nicht wissen, welcher konkrete KI-Provider einen Auftrag verarbeitet. Die bestehende Verantwortungsgrenze bleibt grundsätzlich: Realm → Coordinator/AI-Service → KI-Provider. Der Coordinator bildet die zentrale kontrollierte Schnittstelle zwischen Realm und Runtime-KI. Provider-spezifische Kommunikation (insbesondere API-Endpunkte, Authentifizierung, Modellnamen, Request-/Response-Formate, Timeouts, Rate-Limits und Fehlerbehandlung) gehört hinter eine klar abgegrenzte Provider-Schicht des KI-Systems und wird nicht in Realm-Logik oder Clients verteilt.

Zukünftig soll eine konfigurierbare Provider-Auswahl möglich sein; eine spätere Auswahl unterschiedlicher Provider oder Modelle abhängig von Jobtyp, Realm oder anderen kontrollierten Kriterien darf architektonisch möglich bleiben. Welche Routingregeln tatsächlich verwendet werden, wird später entschieden. Es werden jetzt keine automatische Provider-Auswahl, Fallback-Kette oder Kostenlogik festgelegt.

API-Keys, Tokens und andere Provider-Zugangsdaten sind Service-Secrets. Sie dürfen insbesondere nicht an Clients übertragen werden, nicht Bestandteil von Realm-Jobs sein, nicht in Git eingecheckt werden, nicht in normalen Logs erscheinen und nicht unnötig in Realm-/Gameplay-Datenbanken gespeichert werden. Die konkrete Secret-Verwaltung wird bei der späteren Implementierung festgelegt.

Ein externer KI-Provider erhält dadurch keinerlei direkte Autorität über Realm, Datenbanken oder Clients. Alle übrigen zentralen KI-Regeln dieses Dokuments (Realm-Autorität, Queue, Validierung, Rate-Limits, Fehler- und Fallback-Regeln, NPC-Wissensregeln) bleiben providerunabhängig bestehen.

---

# 4. Zentrale AI-Regel

> **Die KI erzählt mit den Fakten der Welt – sie bestimmt die Fakten der Welt nicht.**

Beispiel:

```text
Server:
Borin befindet sich in Ardan.
Borin kennt Spieler.
Beziehung = 72.
Borin weiß, dass die Brücke zerstört wurde.

        ↓

AI Context

        ↓

KI:
formuliert Borins Reaktion
```

Die KI darf nicht eigenständig behaupten, dass Borin gestern einen Drachen besiegt hat, wenn diese Information nicht Teil seines Wissens oder des erlaubten Szenenkontexts ist.

---

# 5. NPCs dürfen trotzdem lügen

Die Regel gegen erfundene Weltfakten bedeutet nicht, dass jeder NPC immer die Wahrheit sagen muss.

Ein NPC darf:

* lügen
* Informationen verschweigen
* manipulieren
* übertreiben
* täuschen
* eine Antwort verweigern

wenn:

* seine Persönlichkeit dies erlaubt
* er über das notwendige Wissen verfügt
* die Situation dazu passt

Beispiel:

```text
Server-Wahrheit:
In der Ruine wartet kein Schatz.
Dort befinden sich Banditen.

NPC-Wissen:
NPC kennt beide Fakten.

NPC-Persönlichkeit:
betrügerisch

        ↓

NPC:
"In der alten Ruine liegt ein wertvoller Schatz."
```

Die KI hat dabei keine Weltinformation erfunden.

Der NPC hat bewusst über eine bekannte Wahrheit gelogen.

---

# 6. AI Context

Der KI-Provider bekommt nicht automatisch den gesamten Weltzustand oder die vollständige Datenbank.

Der Server erstellt für jede Anfrage einen begrenzten, relevanten Kontext.

Beispiel:

```text
AI Request
├── request_type
├── player_context
├── npc_context
├── world_context
├── knowledge_context
├── relationship_context
├── scene_context
├── allowed_actions
└── locale
```

Nur benötigte Daten werden übergeben.

---

# 7. Narrative Context

Rohdaten können vor der KI-Anfrage durch den Server in erzählerisch sinnvolle Informationen übersetzt werden.

Beispiel:

```text
playtime_hours = 1240
quests_completed = 387

        ↓

Narrative Context

long_time_adventurer = true
experienced_adventurer = true
helped_many_people = true
```

Lua kann NPC-spezifisch festlegen, wie solche Informationen interpretiert werden dürfen.

Ein Zeremonienmeister kann dieselben Daten anders verwenden als ein Schmied, Wirt oder Gildenmeister.

---

# 8. KI-Systeme

Andora verwendet nicht eine einzige KI-Aufgabe für alles.

Das AI-System wird in spezialisierte Bereiche getrennt.

## NPC Dialogue AI

Verantwortlich für:

* Gespräche
* Persönlichkeit
* Reaktionen
* Wissen
* Beziehungen
* situationsabhängige Antworten
* bewusstes Lügen oder Verschweigen

Details:

`Ki-NPC.md`

---

## Command AI

Ein kleines, schnelles Modell kann Sprache bzw. natürliche Befehle in strukturierte Spielbefehle übersetzen.

Beispiel:

```text
"Alle zurück und beschützt mich!"

        ↓

Command AI

        ↓

{
    intent: "RETREAT_AND_PROTECT",
    scope: "ALL_COMPANIONS"
}
```

Die KI führt diesen Befehl nicht aus.

Der Server prüft anschließend:

* Eigentümer
* Begleiter
* Zustand
* Fähigkeiten
* Ziel
* Reichweite
* Cooldowns
* Ressourcen
* Berechtigungen

Erst danach wird eine erlaubte Aktion ausgeführt.

---

## Narrative AI

Verantwortlich für:

* dynamische Scene-Dialoge
* persönliche Story-Reaktionen
* World-Event-Dialoge
* Zeremonien
* besondere narrative Momente

Die Scene Engine bestimmt Ablauf und Weltzustand.

Die Narrative AI improvisiert ausschließlich freigegebene Dialogpassagen.

Details:

`AI-Cutscene.md`

---

## AI Request Parser

Natürliche Wünsche können in strukturierte Anforderungen übersetzt werden.

Beispiel Crafting:

```text
Spieler:
"Ich möchte ein Schwert aus Mithril mit zwei Sockeln
und einem Rubin."

        ↓

AI Request Parser

        ↓

{
    type: "sword",
    material: "mithril",
    sockets: 2,
    gem: "ruby"
}
```

Danach übernimmt der normale CraftingService.

Die KI bestimmt niemals:

* Kosten
* benötigte Materialien
* erlaubte Sockel
* Stats
* Qualität
* Craftingzeit

Diese Werte bestimmt der Server.

---

# 9. Strukturierte AI-Ausgaben

Wo eine KI-Antwort eine Spielaktion beeinflussen kann, sollte sie möglichst strukturiert erfolgen.

Beispiel:

```json
{
  "intent": "HEAL_OWNER",
  "target": "player_123",
  "confidence": 0.94
}
```

Der Server behandelt diese Ausgabe als:

> **Vorschlag**

nicht als Befehl.

Erst die Servervalidierung entscheidet über die tatsächliche Aktion.

---

# 10. i18n

Das bestehende Andora-i18n-System ist die einzige Quelle für die Sprache des Spielers.

Es gibt keine separate KI-Spracheinstellung.

```text
Godot i18n
      ↓
Spieler-Locale
      ↓
Gameserver
      ↓
AI Service
      ↓
KI-Provider
```

Der AI-Service gibt die gewünschte Ausgabesprache zentral vor.

Beispiel:

```text
locale = de
```

Die Lua-Prompts dürfen intern beispielsweise auf Englisch geschrieben sein.

Die Antwort an den Spieler erfolgt trotzdem auf Deutsch.

> **Die aktuell gewählte i18n-Sprache bestimmt auch die Sprache KI-generierter Spielinhalte.**

---

# 11. Asynchrone Verarbeitung

KI-Anfragen dürfen niemals den normalen Gameserver-Tick blockieren.

Der Andora-World-Tick läuft mit:

```text
10 Hz
100 ms
```

KI-Anfragen werden davon getrennt verarbeitet.

```text
Game Event
     ↓
AI Request erzeugen
     ↓
asynchrone Verarbeitung

World Tick ───────────────────────→ läuft weiter

     ↓
AI Response
     ↓
Server validiert
     ↓
Ergebnis verwenden
```

Der Server wartet niemals innerhalb des World-Ticks synchron auf den KI-Provider.

---

# 12. Prioritäten

Nicht jede KI-Anfrage ist gleich wichtig.

Das AI-System sollte Anfragen kategorisieren können.

Beispiel:

```text
HIGH
→ wichtiger Scene-Dialog
→ direkte Spieler-NPC-Interaktion

NORMAL
→ NPC-Reaktion
→ Crafting-Interpretation

LOW
→ Ambient NPC Conversation
→ optionale Hintergrundreaktion
```

Bei hoher Auslastung können unwichtige KI-Aufgaben verzögert oder verworfen werden.

Gameplay darf dadurch nicht blockiert werden.

---

# 13. AI Budget

Die Anzahl gleichzeitig laufender KI-Anfragen muss begrenzt werden.

Besonders wichtig ist dies später bei:

* Tavernen
* Städten
* World Events
* vielen Spielern
* NPC-zu-NPC-Gesprächen

Beispiel:

```text
AI Request Queue
        ↓
Priority
        ↓
Concurrency Limit
         ↓
KI-Provider (lokal: Ollama)
```

Ein Raum mit 30 NPCs darf nicht automatisch 30 parallele LLM-Anfragen erzeugen.

---

# 14. Cache-System

Das alte Konzept eines globalen 10-Minuten-Response-Caches wird nicht übernommen.

Dynamische Antworten hängen häufig von aktuellem Kontext ab.

Beispiel:

```text
Spieler:
"Wo ist Borin?"
```

Eine Antwort von vor fünf Minuten kann bereits falsch sein.

Deshalb werden persönliche oder weltabhängige KI-Antworten grundsätzlich nicht blind wiederverwendet.

---

# 15. Was gecacht werden darf

Geeignete Cache-Kandidaten sind beispielsweise:

* statische Prompt-Bausteine
* vorbereitete System-Prompts
* Lua-NPC-Definitionen
* Itemdefinitionen
* Questdefinitionen
* statische Narrative Regeln
* unveränderliche Referenzinformationen

Nicht allgemein wiederverwenden:

* NPC-Dialogantworten
* persönliche Scene-Dialoge
* aktuelle Weltinformationen
* Beziehungsreaktionen
* Knowledge-basierte Antworten
* aktuelle Service-Verfügbarkeit

Falls später Response-Caching benötigt wird, muss der vollständige relevante Kontext Bestandteil der Cache-Entscheidung sein.

---

# 16. AI-Ausfall

Der KI-Provider ist kein kritischer Bestandteil des World-Ticks.

Bei einem Ausfall:

```text
KI-Provider offline
        ↓
World Tick läuft weiter
       ↓
Combat läuft weiter
       ↓
NPC-Bewegung läuft weiter
       ↓
Quests funktionieren
       ↓
Inventory funktioniert
       ↓
Crafting-Grundsystem funktioniert
```

KI-abhängige Funktionen verwenden:

* Fallbacktexte
* Lua-Regeln
* deterministische Antworten
* temporäre Nichtverfügbarkeit

Ein Provider-Ausfall darf niemals den Gameserver zum Stillstand bringen.

---

# 17. NPC-Wissen

Die KI bekommt für einen NPC nur Informationen, die dieser NPC tatsächlich besitzen darf.

> **NPCs dürfen nur auf Informationen reagieren, die sie tatsächlich erhalten haben.**

Information kann beispielsweise entstehen durch:

* eigene Beobachtung
* Spieler erzählt etwas
* anderer NPC erzählt etwas
* Messenger übermittelt Nachricht
* World Event wird beobachtet
* offizielle regionale Information
* erlaubte berufliche Informationen

Der AI-Service darf einem NPC keine globale Allwissenheit geben.

---

# 18. Weltwissen und NPC-Wissen

Es muss zwischen Server-Wahrheit und NPC-Wissen unterschieden werden.

```text
WORLD TRUTH
"Die Brücke wurde zerstört."

NPC A
→ hat es gesehen
→ weiß es

NPC B
→ befindet sich weit entfernt
→ weiß es nicht

NPC C
→ bekam Nachricht von NPC A
→ weiß es
```

Alle drei NPCs können deshalb auf dieselbe Spielerfrage unterschiedlich reagieren.

---

# 19. AI und Gameplay-Aktionen

Eine KI darf niemals direkt:

* Items erzeugen
* Gold verändern
* XP vergeben
* Spieler teleportieren
* NPCs teleportieren
* Quests abschließen
* Schaden verursachen
* Spieler heilen
* Beziehungen verändern
* World States ändern
* DB-Einträge direkt verändern

Stattdessen kann die KI eine erlaubte Aktion vorschlagen.

Beispiel:

```text
AI
→ HEAL_OWNER

        ↓

Server

Darf NPC heilen?
Hat NPC Fähigkeit?
Genug Mana?
Cooldown bereit?
Ziel gültig?
Reichweite gültig?

        ↓

JA
→ HealService führt Aktion aus
```

---

# 20. Lua und AI

Lua darf Prompt-Fragmente und AI-Regeln definieren.

Beispiel:

```lua
ai = {
    personality = "friendly_blacksmith",

    prompt = [[
        Speak like an experienced blacksmith.
        Keep answers concise.
        Use only provided world knowledge.
    ]]
}
```

Globale Sicherheits- und Autoritätsregeln werden jedoch zentral vom AI-Service ergänzt.

Eine fehlerhafte Lua-Datei darf die zentrale Regel:

> Server ist die Quelle der Wahrheit.

nicht aufheben.

## Ruleset-spezifische KI-Schicht

Ein Ruleset darf zusätzlich eine übergeordnete Prompt-/Verhaltensschicht für dynamisch generierte KI-NPC-Kommunikation bereitstellen.

Dabei gilt:

* Die eigentliche NPC-Persönlichkeit bleibt unabhängig vom Ruleset erhalten.
* Rolle, Wissen, Beziehungen und individuelle Eigenschaften eines NPC werden nicht durch das Ruleset ersetzt.
* Das Ruleset ergänzt lediglich übergeordnete Verhaltens-, Kommunikations- und Tonalitätsregeln.
* Es werden keine separaten vollständigen NPC-Prompt-Sammlungen pro Ruleset gepflegt; es gibt genau eine Schicht pro Ruleset (`normal`, später ggf. `roleplay`, `hardcore`).
* Der Realm übergibt sein Ruleset mit jeder KI-Anfrage, damit die zugehörige Schicht angewendet werden kann.

Beispiele:

* `normal`: normale für Andora vorgesehene NPC-Kommunikation.
* `roleplay`: NPCs können konsequenter in ihrer Weltrolle sprechen, moderne oder spielmechanische Ausdrucksweisen vermeiden und stärker immersiv auf den Spieler reagieren.
* `hardcore`: dynamische NPC-Kommunikation kann einen raueren, direkteren oder teilweise feindseligeren Grundton erhalten und die gefährlichere Atmosphäre des Realms widerspiegeln.

Ein grundsätzlich freundlicher NPC muss dadurch nicht feindselig werden. Seine individuelle Persönlichkeit hat weiterhin Bestand; das Ruleset beeinflusst nur den übergeordneten Ton und das dynamische Verhalten.

Klare inhaltliche Grenze: Ruleset-spezifische Prompt-/Tonalitätsschichten dürfen ausschließlich dynamisch von der KI generierte Kommunikation und Reaktionen beeinflussen. Fest definierte Inhalte (Questtexte, fest geschriebene Questdialoge, Storytexte, Lore, Bücher und Briefe, Cutscene-Dialoge und andere redaktionell festgelegte Texte) bleiben davon unberührt und sind auf allen Rulesets identisch.

---

# 21. AI und Dynamic Scene System

Die Scene Engine kontrolliert:

* Teilnehmer
* Positionen
* NPC Scene Locks
* Bewegungen
* Animationen
* Szenenphasen
* Bedingungen
* Konsequenzen

Die KI kontrolliert:

* ausdrücklich freigegebene Dialogpassagen
* persönliche Formulierungen
* situationsabhängige Reaktionen

Damit können Szenen teilweise gescriptet und teilweise improvisiert sein.

---

# 22. AI und Crafting

KI kann natürliche Crafting-Wünsche verstehen.

Sie darf jedoch keine Crafting-Regeln bestimmen.

```text
Spielerwunsch
      ↓
AI Parser
      ↓
strukturierte Spezifikation
      ↓
CraftingService
      ↓
Validierung
      ↓
Kosten / Materialien / Ergebnis
```

Der Server bleibt vollständig autoritativ.

---

# 23. AI und Bosskämpfe

KI kann Dialoge und Reaktionen rund um Bosskämpfe erzeugen.

Taktische Informationen dürfen jedoch nur verwendet werden, wenn der entsprechende NPC diese Informationen tatsächlich kennt.

Ein NPC darf nicht durch das AI-System plötzlich Zugriff auf:

* versteckte Bossmechaniken
* interne Serverwerte
* unbekannte Fähigkeiten
* nicht beobachtete Ereignisse

erhalten.

---

# 24. Datenschutz innerhalb des Spiels

Nicht jeder gespeicherte Spielerwert sollte automatisch in einen KI-Prompt gelangen.

Der Context Builder übergibt nur Informationen, die:

1. für die aktuelle Anfrage relevant sind,
2. für den betreffenden NPC bzw. die Szene erlaubt sind,
3. narrativ sinnvoll verwendet werden können.

Beispielsweise kann eine Hochzeitszeremonie Spielzeit und gemeinsame Abenteuer berücksichtigen, während ein zufälliger Händler diese Informationen nicht automatisch erhält.

---

# 25. Fehlerbehandlung

Jede AI-Anfrage benötigt definierte Fehlerfälle.

Dazu gehören:

```text
TIMEOUT
MODEL_UNAVAILABLE
INVALID_RESPONSE
INVALID_JSON
CONTEXT_INVALID
ACTION_REJECTED
REQUEST_CANCELLED
```

Eine fehlerhafte KI-Antwort darf nicht ungeprüft in Gameplay umgesetzt werden.

## Fehlerhafte oder leere Anfragen in der Queue

Eine fehlerhafte, ungültige oder leere KI-Anfrage darf die Verarbeitung der AI-Queue niemals dauerhaft blockieren.

Wenn der AI-Service bzw. Coordinator feststellt, dass eine Anfrage nicht verarbeitet werden kann, muss der betroffene Job selbstständig als fehlgeschlagen behandelt und aus der aktiven Queue entfernt werden. Anschließend wird automatisch mit der nächsten Anfrage fortgefahren.

Dies gilt insbesondere bei:

* leeren Anfragen
* ungültigen oder unvollständigen Jobdaten
* nicht mehr auflösbaren Jobreferenzen
* beschädigten Jobdateien
* Anfragen, die auch nach den vorgesehenen Validierungs- oder Retry-Versuchen nicht verarbeitet werden können

```text
Job laden
    ↓
gültig und verarbeitbar?
├── JA   → normal verarbeiten
└── NEIN → Job als fehlgeschlagen behandeln
           ↓
           aus aktiver Queue entfernen
           ↓
           Fehlergrund protokollieren
           ↓
           nächsten Job verarbeiten
```

Falls eine Rückmeldung an den Gameserver noch möglich ist, erhält dieser einen passenden Fehlerstatus. Bestehende Recovery-Regeln bleiben davon unberührt.

Der AI-Service darf wegen eines einzelnen fehlerhaften oder leeren Jobs nicht pausieren und auf manuellen Eingriff warten.

> **Ein einzelner defekter KI-Job darf niemals die nachfolgenden KI-Anfragen blockieren.**

---

# 26. Logging und Monitoring

Das Monitoring-System sollte später grundlegende AI-Metriken anzeigen können.

Beispiele:

```text
AI Status
aktive Requests
wartende Requests
Requests pro Minute
durchschnittliche Antwortzeit
Timeouts
Fehler
Modell
Queue-Auslastung
```

Dabei sollten keine unnötigen vollständigen privaten Spielerunterhaltungen dauerhaft als Monitoringdaten gespeichert werden.

---

# 27. Erweiterbarkeit

Die Architektur soll weitere KI-Funktionen ermöglichen, ohne die Serverautorität aufzugeben.

Mögliche spätere Systeme:

* NPC-zu-NPC-Gespräche
* dynamische World-Event-Reaktionen
* Gilden-/Fraktionsreaktionen
* zusätzliche Command-Modelle
* narrative Ereignisse
* komplexere NPC-Planung

Neue KI-Systeme müssen dieselben zentralen Regeln einhalten.

---

# 28. Architekturübersicht

```text
                       ┌──────────────┐
                       │ Godot Client │
                       └──────┬───────┘
                              │
                              ▼
                    ┌──────────────────┐
                    │ Andora Server    │
                    │   (Rust)         │
                    └───────┬──────────┘
                            │
              ┌─────────────┼─────────────┐
              │             │             │
              ▼             ▼             ▼
          MariaDB          Lua        AI Context
                                        Builder
                                          │
                                          ▼
                                      AI Service
                                           │
                                           ▼
                                      KI-Provider
                                  (lokal: Ollama)
                                           │
                                           ▼
                                    AI Response
                                          │
                                          ▼
                                Server Validation
                                          │
                                          ▼
                                   Game Systems
```

---

# 29. Zentrale Architekturregeln

> **Serverzustand ist Wahrheit.**

> **KI interpretiert – sie autorisiert nicht.**

> **Die KI erhält nur den Kontext, den sie für ihre aktuelle Aufgabe benötigt.**

> **NPCs dürfen nur auf Wissen reagieren, das sie tatsächlich besitzen.**

> **KI-Anfragen dürfen den World-Tick niemals blockieren.**

> **Fehlerhafte oder leere KI-Jobs dürfen die AI-Queue niemals blockieren; sie werden nach den Fehler- und Recovery-Regeln selbstständig behandelt und die Verarbeitung wird mit dem nächsten Job fortgesetzt.**

> **Das Spiel muss auch ohne verfügbaren KI-Provider funktionieren.**

> **Andora bleibt vollständig mit lokal betriebener KI (Standard-/Basislösung: Ollama) lauffähig; externe Provider dürfen keine zwingende Cloud-Abhängigkeit erzeugen.**

> **Spielmechanik liegt im Realm-Server (Rust), Inhalte und Prompt-Fragmente liegen in Lua, Persistenz liegt in MariaDB und die KI übernimmt Sprache, Interpretation und kontrollierte Improvisation.**

> **Die i18n-Sprache des Spielers bestimmt auch die Sprache der KI-Ausgabe.**
