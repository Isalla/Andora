# Coordinator – KI-Queue und Provider-Schnittstelle

> **Umsetzungsstand (2026-09):** Implementiert als eigenständiger Go-Dienst
> in `src/coordinator` (Endpunkte, Betrieb und Konfiguration:
> `src/coordinator/README.md`, `deploy/systemd/andora-coordinator.service`,
> `deploy/conf/coordinator.conf`). Das Realm (Rust) ist noch nicht an den
> Coordinator angeschlossen; die Fallback-Regeln der Abschnitte 13/24/25
> gelten dann.

## 1. Zweck

Der Coordinator ist die zentrale Schnittstelle zwischen den Andora-Realmservern und den KI-Providern.

Der bestehende lokale Betrieb über Ollama bleibt ein unterstützter und zunächst bevorzugter Weg (Standard-/Basislösung); die Architektur ist jedoch providerunabhängig (siehe §3.1).

Seine Aufgabe ist ausschließlich die kontrollierte Verarbeitung von KI-Anfragen.

Der Coordinator ist kein Gameserver, kein Loginserver, keine Account-API und keine zentrale Spieldatenbank.

Grundprinzip:

**Realm → Coordinator → KI-Provider → Coordinator → Realm**

Der Realm muss dabei nicht wissen, welcher konkrete KI-Provider einen Auftrag verarbeitet (aktuell lokal: Ollama; Details zur Providerunabhängigkeit in §3.1).

Die Realmserver bleiben vollständig für ihre eigenen Spiel-, Charakter-, Item- und Auftragsdaten verantwortlich.

---

## 2. Sicherheitsgrenze

Der Coordinator besitzt keine Datenbankrechte.

Er erhält insbesondere keinen direkten Zugriff auf:

* Auth-Datenbank
* Accountdaten
* Character-Daten (in der jeweiligen Realm-Datenbank)
* Realm-Datenbanken
* Item-Daten
* Crafting-Daten

Er benötigt deshalb auch keine Zugangsdaten zu diesen Datenbanken.

Seine lokale Persistenz besteht ausschließlich aus seinen Queue-/Job-Dateien, technischen Logs und dem persistenten Erinnerungsspeicher für NPC-Beziehungen und -Wissen (Details: `Ki-NPC.md`, Abschnitt 0).

Der Coordinator darf keine Spielzustände direkt verändern.

**Der KI-Provider erzeugt Vorschläge bzw. Antworten. Der Coordinator prüft und vermittelt diese. Der Realm entscheidet über alle spielmechanischen Auswirkungen.**

---

## 3. Zentrale Provider-Schnittstelle

Realmserver greifen nicht direkt auf KI-Provider zu.

Alle KI-Anfragen werden an den Coordinator gesendet.

Dadurch existiert eine zentrale Stelle für:

* Queue-Verwaltung
* Priorisierung
* Lastbegrenzung
* Rate-Limits
* Input-Prüfung
* Output-Prüfung
* Korrekturversuche
* Timeouts
* Provider-Verfügbarkeit
* Recovery

Der KI-Provider muss dadurch nicht gleichzeitig unabhängige Verbindungen von allen Realmservern verwalten.

### 3.1 Providerunabhängigkeit und Provider-Schicht

Andoras Runtime-KI ist langfristig nicht fest an Ollama oder einen einzelnen KI-Anbieter gekoppelt.

Der bestehende lokale Betrieb über Ollama bleibt ein unterstützter und zunächst bevorzugter Weg. Ollama bleibt als lokale Standard-/Basislösung vorgesehen. Andora muss weiterhin vollständig mit lokal betriebener KI arbeiten können; eine spätere Unterstützung externer Provider darf keine zwingende Cloud-Abhängigkeit erzeugen.

Die Architektur ermöglicht es, später auch externe KI-Provider über deren API anzubinden, ohne dafür Realm oder Gameplay-Systeme grundlegend umbauen zu müssen.

Mögliche Beispiele sind:

* lokale Modelle über Ollama
* OpenAI- / ChatGPT-kompatible API
* Anthropic
* weitere zukünftige KI-Anbieter

Die Nennung konkreter Anbieter beschreibt ausschließlich Erweiterungsmöglichkeiten und stellt keine Verpflichtung dar, diese jetzt zu implementieren oder dauerhaft zu unterstützen. Es wird hier noch keine konkrete Provider-API oder Implementierung festgelegt.

Grundprinzip:

Der Realm muss nicht wissen, welcher konkrete KI-Provider einen Auftrag verarbeitet.

Die bestehende Verantwortungsgrenze bleibt grundsätzlich:

Realm → Coordinator → KI-Provider

Der Coordinator bildet die zentrale kontrollierte Schnittstelle zwischen Realm und Runtime-KI.

Provider-spezifische Kommunikation gehört hinter eine klar abgegrenzte Provider-Schicht des KI-Systems und wird nicht in Realm-Logik oder Clients verteilt. Innerhalb der Provider-/Coordinator-Schicht gekapselt werden insbesondere provider-spezifische Unterschiede wie:

* API-Endpunkte
* Authentifizierung
* Modellnamen
* Request-/Response-Formate
* Timeouts
* Rate-Limits
* Fehlerbehandlung

Provider-Auswahl:

Die Architektur soll zukünftig eine konfigurierbare Provider-Auswahl ermöglichen. Eine spätere Auswahl unterschiedlicher Provider oder Modelle abhängig von Jobtyp, Realm oder anderen kontrollierten Kriterien darf architektonisch möglich bleiben.

Welche Routingregeln tatsächlich verwendet werden, wird später entschieden. Es werden jetzt keine automatische Provider-Auswahl, Fallback-Kette oder Kostenlogik festgelegt.

Bestehende Regeln bleiben erhalten:

Ein externer KI-Provider erhält dadurch keinerlei direkte Autorität über Realm, Datenbanken oder Clients. Bestehende Regeln zu Queue, Validierung, Jobzuständen, Rate-Limits, Spam-/Inhaltsfiltern und Realm-Autorität bleiben bestehen, soweit sie nicht technisch ausschließlich an Ollama gekoppelt formuliert sind. Die übrigen Abschnitte dieses Dokuments gelten providerunabhängig; wo dort noch „Ollama“ als konkreter Provider genannt ist, ist damit der aktuell angeschlossene lokale Standard-Provider gemeint.

---

## 4. Keine Realm-Queues

Realmserver besitzen keine vorgelagerten KI-Warteschlangen.

Wenn die Coordinator-Queue voll ist, antwortet der Coordinator beispielsweise mit:

`QUEUE_FULL`

Der betreffende KI-Auftrag ist damit für diesen Versuch beendet.

Der Realm darf die Anfrage nicht automatisch speichern und später erneut senden.

Dadurch wird verhindert, dass nach einer Überlast plötzlich mehrere Realmserver gleichzeitig aufgestaute Anfragen an den Coordinator senden.

Der Realm verwendet stattdessen eine lokale, fest definierte Reaktion.

Beispiel bei einem Schmied:

„Heute habe ich keine Zeit mehr. Komm später wieder.“

Diese Fallback-Antwort benötigt keine neue KI-Anfrage.

---

## 5. Priorisierung

Der Coordinator priorisiert KI-Aufträge zentral.

Vorgesehene Grundreihenfolge:

1. Raid-/Boss-KI
2. direkte Spieleranfragen
3. aktive NPC-Interaktion mit Spielern
4. Quest-/Event-NPCs
5. Hintergrundverhalten von NPCs
6. Welt-/Atmosphäre-Aktionen

Die konkrete Gewichtung soll konfigurierbar sein und nicht unnötig im Programmcode fest verdrahtet werden.

Beispiel:

* `raid_boss = 100`
* `player_request = 90`
* `npc_player_active = 80`
* `quest_event = 70`
* `background_npc = 30`
* `world_atmosphere = 10`

Innerhalb derselben Priorität wird grundsätzlich nach Eingangsreihenfolge gearbeitet.

Eine komplexere dynamische Priorisierung soll erst eingeführt werden, wenn reale Lasttests zeigen, dass sie notwendig ist.

---

## 6. Spieleranfragen und Spam-Schutz

Freie Spieleranfragen an KI-NPCs erhalten einen Cooldown.

Startwert:

**maximal eine freie KI-Anfrage pro Spieler innerhalb von 5 Sekunden**

Der Cooldown gilt ausschließlich für echte KI-Anfragen.

Normale Spielmechaniken wie:

* Handeln
* Reparieren
* Quest annehmen
* Inventaraktionen
* Kaufen/Verkaufen
* normale Crafting-Mechaniken

laufen vollständig über normalen Realm-Spielcode und dürfen nicht unnötig an den KI-Provider gesendet werden.

---

## 7. Begrenzung der Texteingabe

Freie Texteingaben von Spielern werden zunächst auf maximal:

**500 Zeichen**

begrenzt.

Die Prüfung soll nicht ausschließlich dem Client vertraut werden.

Die Grenze soll mindestens erneut auf Realm-Seite und beim Coordinator geprüft werden.

Zu lange Eingaben werden nicht an den KI-Provider weitergegeben.

Der Coordinator kann beispielsweise antworten:

`INPUT_TOO_LONG`

Die 500-Zeichen-Grenze betrifft die freie Texteingabe des Spielers, nicht den gesamten internen KI-Kontext.

---

## 8. Kontextbudget

Für den KI-Provider ist zunächst ein Kontext von ungefähr:

**8k Tokens**

vorgesehen (provider-spezifischer Konfigurationswert der Provider-Schicht; aktuell bemessen am lokalen Ollama-Standardbetrieb).

Der Coordinator muss darauf achten, dass genügend Platz für die eigentliche Antwort verbleibt.

Der Gesamtkontext kann beispielsweise enthalten:

* zentrale KI-Regeln
* NPC-Kontext
* relevante Spielsituation
* notwendige Weltinformationen
* Spielertext
* Platz für die Antwort

Unnötige Informationen dürfen nicht in jede Anfrage aufgenommen werden.

---

## 9. Input-Prüfung

Bevor eine Spieleranfrage an den KI-Provider geschickt wird, prüft der Coordinator den Inhalt gegen zentrale Regeln.

Dadurch wird verhindert, dass jeder einzelne Realm sämtliche Inhaltsregeln selbst vollständig implementieren muss.

Die Prüfung kann beispielsweise erkennen:

* nicht erlaubte Inhalte
* für Andora unzulässige Gegenstände
* offensichtlich ungültige Anforderungen
* Manipulationsversuche
* ungültiges Format
* überschrittene Eingabelimits

Der Coordinator entscheidet dabei nicht über Realm-Spielzustände.

### 9.1 Mehrsprachige globale Sperrwortfilter

Die zentralen Wortregeln liegen sprachdateibasiert unter
`COORDINATOR_FILTER_DIR` (Standard `filters`) — eine Datei je Sprache und
Richtung:

```
filters/en.input.txt    englische Masterliste   (Input; von uns gepflegt)
filters/en.output.txt   englische Masterliste   (Output)
filters/de.input.txt    deutsche Ergänzungen    (Input)
filters/de.output.txt   deutsche Ergänzungen    (Output)
```

Regeln:

1. Die englische Datei ist die maßgebliche **Masterliste** und die
   Ausgangsbasis für spätere Übersetzungen. Weitere Sprachdateien können
   daraus übersetzt und zusätzlich sprachspezifisch ergänzt werden.
2. Beim Start lädt der Coordinator **alle vorhandenen Sprachdateien
   gemeinsam in den Speicher**. Es gibt keine Konfiguration für
   „aktive Sprachen" — jede vorhandene Datei zählt.
3. Die Filterung läuft immer gegen **alle geladenen Sprachen** und ist
   **nicht von der Client-/Spielersprache abhängig**. Ein Wechsel der
   Client-Sprache kann Filter daher nicht umgehen.
4. **Input und Output sind getrennt:** `*.input.txt` greift auf die
   Spielereingabe (§9), `*.output.txt` auf die Provider-Antwort (§10).
5. `INPUT_DENY_WORDS`/`OUTPUT_DENY_WORDS` bleiben als **zusätzliche
   Betreiber-Einträge** erhalten und verschmelzen mit den Dateiregeln.
6. Format: eine Regel je Zeile (niedrig geschrieben), `#`-Zeilen sind
   Kommentare. Einzelwörter werden Wort-genau gematcht (kein Teilstring),
   Phrasen (mit Leerzeichen) nur als ganze Wortfolge; Matching ist
   case-insensitiv und Unicode-fähig (Umlaute/Akzente).
7. Fehlende oder leere Filterdateien sind erlaubt — die jeweilige Seite
   bleibt dann leer bzw. nur durch die Betreiber-Einträge belegt.

**Neue Sprache ergänzen** (keine Codeänderung):

1. Aus der englischen Masterliste übersetzen.
2. `filters/<lang>.input.txt` und `filters/<lang>.output.txt` anlegen
   und ggf. sprachspezifische Begriffe ergänzen.
3. Coordinator starten — die Datei wird automatisch geladen und gilt ab
   dann für alle Jobs.

---

## 10. Output-Prüfung

Auch die Provider-Antwort wird vor der Weitergabe an den Realm geprüft.

Dabei können sowohl Inhaltsregeln als auch definierte Welt-/Plausibilitätsregeln geprüft werden.

Beispiel:

Der KI-Provider schlägt für einen Schmied ein 20 Meter langes Schwert vor.

Der Coordinator erkennt, dass die vorgeschlagenen Eigenschaften außerhalb der erlaubten Andora-Regeln liegen.

Die Antwort wird nicht unmittelbar verworfen.

Stattdessen erhält der KI-Provider einen Korrekturauftrag und soll eine regelkonforme Alternative erzeugen.

Grundprinzip:

**Nicht sofort ablehnen, sondern innerhalb eines begrenzten Rahmens korrigieren lassen.**

---

## 11. Begrenzte Korrekturschleife

Coordinator und KI-Provider dürfen niemals unbegrenzt in einer Korrekturschleife hängen.

Startwert:

**maximal 5 Korrektur-/Wiederholungsversuche pro Job**

Nach Erreichen des Limits wird der Job beendet.

Der Realm erhält lediglich einen technischen Fehlerstatus, beispielsweise:

`VALIDATION_FAILED`

Der Realm verwendet anschließend eine fest definierte lokale Fallback-Reaktion.

Der Fallback darf niemals selbst eine neue KI-Anfrage erzeugen.

---

## 12. Fehlende oder leere Provider-Antwort

Nach jeder Provider-Anfrage muss geprüft werden, ob tatsächlich eine verwertbare Antwort vorhanden ist.

Als ungültig gelten insbesondere:

* keine Antwort
* leere Antwort
* ausschließlich Whitespace
* unvollständige strukturierte Antwort
* nicht parsebares erwartetes Format

Dies kann beispielsweise auftreten, wenn eine Anfrage das verfügbare Kontextlimit überschreitet oder der KI-Provider die Generierung abbricht.

Solche Fälle zählen als fehlgeschlagener Versuch.

Nach Erreichen des maximalen Retry-Limits wird der Job beendet und der Realm informiert.

---

## 13. Fallback-Grundsatz

KI-Fehler dürfen keine neue KI-Anfrage erzeugen.

Dies gilt insbesondere bei:

* `QUEUE_FULL`
* `AI_UNAVAILABLE`
* `TIMEOUT`
* `VALIDATION_FAILED`
* `CONTEXT_TOO_LARGE`
* `INPUT_TOO_LONG`
* fehlender/leer gebliebener Provider-Antwort

Der Coordinator meldet nur den technischen Zustand an den Realm.

Der Realm besitzt für solche Fälle fest definierte lokale Reaktionen.

Dadurch funktionieren grundlegende NPC-Reaktionen auch bei vollständig ausgefallenem KI-System.

---

## 14. Persistente dateibasierte Queue

Die Coordinator-Queue darf nicht ausschließlich im RAM existieren.

Jeder angenommene KI-Auftrag wird als eigene Datei gespeichert.

Es soll bewusst keine einzelne große Datei geben, welche sämtliche Auftragsinhalte enthält.

Grund:

Ein beschädigter Schreibvorgang oder problematischer Auftrag soll niemals die gesamte Queue gefährden.

Beispiel:

`queue/jobs/<timestamp>_<type>-<id>.json`

Mögliche Beispiele:

`20260902T142015.347_craft-4711.json`

`20260902T142017.201_raidboss-realm03-encounter8842.json`

Die genaue eindeutige ID hängt vom Jobtyp ab.

Mögliche Referenzen:

* Crafting → Craft-/Job-ID des Realms
* Raidboss → Encounter-ID
* NPC-Interaktion → Interaction-ID
* Event → Event-ID

Der Timestamp ermöglicht zusätzlich eine nachvollziehbare Eingangsreihenfolge.

---

## 15. queue.json

Neben den einzelnen Job-Dateien existiert eine separate:

`queue.json`

Diese enthält im Wesentlichen die aktuelle Reihenfolge der abzuarbeitenden Job-Dateien.

Die eigentlichen Auftragsdaten befinden sich nicht ausschließlich in `queue.json`.

Dadurch kann die Queue bei Verlust dieser Datei aus den vorhandenen Job-Dateien rekonstruiert werden.

---

## 16. Sichere Dateischreibvorgänge

Bestehende gültige Job- oder Queue-Dateien dürfen bei einer Aktualisierung nicht direkt überschrieben werden.

Neue Versionen werden zunächst vollständig als temporäre Datei geschrieben.

Grundprinzip:

1. bestehende gültige Datei bleibt bestehen
2. neue Version vollständig temporär schreiben
3. Schreibvorgang abschließen/synchronisieren
4. Inhalt bei Bedarf validieren
5. atomar durch die neue Version ersetzen

Bei einem Absturz während des Schreibens bleibt dadurch möglichst die vorherige gültige Version erhalten.

Für sämtliche temporären Dateien gelten die allgemeinen Andora-Projektregeln
(`docs/Temporäre_Dateien.md`).

Kurzlebige Zwischendaten dürfen systemweites `/tmp`-bzw. `/tmp/opencode`
nutzen; temporäre Dateien eines atomaren Schreibvorgangs in der Queue
gehören jedoch dorthin, wo der atomare Vorgang stattfindet
(`COORDINATOR_DATA_DIR/queue/jobs/.tmp-*`) — ein Umweg über ein fremdes
Dateisystem würde die rename-/Persistenzgarantie brechen. Dauerhafte
Projektdateien gehören ausschließlich in den Projektbereich (`.tmp/` bzw.
den jeweiligen Datenordner), niemals in systemweites `/tmp`.

---

## 17. Recovery bei fehlender queue.json

Fehlt `queue.json` beim Start, darf der Coordinator nicht einfach mit leerer Queue starten.

Er durchsucht die vorhandenen Job-Dateien und rekonstruiert daraus eine neue Queue.

Dabei können Timestamp und eindeutige Job-ID aus den Dateinamen zur Rekonstruktion verwendet werden.

Anschließend wird eine neue `queue.json` sicher erzeugt.

---

## 18. Fehlende Job-Dateien

Wenn `queue.json` auf eine Job-Datei verweist, die nicht mehr vorhanden ist, darf dies die restliche Queue nicht blockieren.

Der Coordinator:

1. erkennt die fehlende Datei
2. überspringt diesen Queue-Eintrag
3. protokolliert den Fehler
4. informiert den zuständigen Realm über das Fehlschlagen der betreffenden Job-ID
5. arbeitet mit dem nächsten Queue-Eintrag weiter

Eine einzelne fehlende oder beschädigte Job-Datei darf niemals die gesamte Queue blockieren.

Problematische Dateien können für Diagnosezwecke separat in Quarantäne verschoben werden, statt sie zwingend sofort endgültig zu löschen.

---

## 19. Graceful Shutdown

Bei einem normalen Shutdown beendet der Coordinator nicht sofort sämtliche Arbeit.

Ablauf:

1. keine neuen KI-Jobs mehr annehmen
2. aktuell laufenden Auftrag kontrolliert fertig bearbeiten
3. Ergebnis bzw. endgültigen Zustand sauber behandeln
4. `queue.json` sicher aktualisieren
5. Dateien synchronisieren
6. Coordinator beenden

Noch nicht begonnene Aufträge bleiben als Job-Dateien erhalten.

Sie müssen beim Shutdown nicht unnötig umgeschrieben werden.

Beim nächsten Start liest der Coordinator `queue.json` und setzt die Verarbeitung fort.

Ein hartes Timeout soll verhindern, dass ein hängender Provider-Aufruf den Shutdown unbegrenzt blockiert.

---

## 20. Realm-Verantwortung für Crafting

Crafting-Daten gehören ausschließlich zum jeweiligen Realm.

Der Coordinator besitzt keine Crafting-Datenbank und greift nicht auf Realm-Datenbanken zu.

Beim Erstellen eines KI-gestützten Crafting-Auftrags erzeugt der Realm zunächst einen eigenen eindeutigen Job-/Craft-Datensatz.

Diese Realm-ID wird anschließend als Referenz an den Coordinator übergeben.

---

## 21. Item-Zuordnung bei Crafting-Aufträgen

Wenn ein Charakter einem NPC Gegenstände oder Materialien für einen Crafting-Auftrag übergibt, werden die eindeutigen Item-Instanz-IDs unmittelbar mit der Job-ID in der Realm-Datenbank verknüpft.

Die Recovery darf nicht ausschließlich davon abhängen, ob die Items korrekt mit einem Status wie `RESERVED` markiert wurden.

Grund:

Nach einem fehlerhaften Update könnte beispielsweise die Reservierungslogik ausfallen.

Die gespeicherte Beziehung

**Job-ID → Character-ID → konkrete Item-IDs**

bleibt deshalb die maßgebliche Recovery-Information.

Auch gezahltes Gold bzw. andere rückerstattbare Kosten müssen dem Job eindeutig zugeordnet werden.

---

## 22. Character-ID

Jeder entsprechende Realm-Job speichert die eindeutige Character-ID des Auftraggebers.

Damit kann der Realm jederzeit bestimmen:

* wem der Auftrag gehört
* welche Items abgegeben wurden
* welches Gold bezahlt wurde
* wem eine Rückerstattung zusteht

Diese Daten bleiben ausschließlich im Realm.

Der Coordinator benötigt dafür keine Character-Datenbank, denn die Charakter- und Jobdaten liegen in der Realm-Datenbank (`realm_state_<realm>`), auf die er keinen Zugriff besitzt.

---

## 23. Rückerstattung über das Mail-System

Scheitert ein Crafting-Auftrag endgültig, kann der Realm die gespeicherten Item-IDs und Kosten anhand der Job-ID rekonstruieren.

Die Items werden nicht einfach direkt in das Inventar zurückgelegt.

Die Rückgabe erfolgt über das Realm-Mail-System an die gespeicherte Character-ID.

Dadurch können Gegenstände auch zurückgegeben werden, wenn:

* der Charakter offline ist
* das Inventar voll ist
* der Charakter sich an einem anderen Ort befindet

Gold kann ebenfalls entsprechend zurückerstattet werden.

Die Rückerstattung muss gegen doppelte Ausführung geschützt sein.

Ein Job darf seine Items und Kosten nur einmal zurückerstatten.

---

## 24. Fehlende Coordinator-Jobs und Realm-Recovery

Wenn der Coordinator feststellt, dass eine erwartete Job-Datei fehlt, sendet er trotzdem eine Fehlermeldung mit der bekannten Job-ID an den Realm.

Der Realm entscheidet anschließend selbstständig, ob diese Job-ID dort noch existiert und ob Recovery erforderlich ist.

Der Coordinator führt niemals selbst Realm-Recovery durch.

---

## 25. Zusätzliche Realm-Konsistenzprüfung

Realmserver können unabhängig vom Coordinator nach ungewöhnlich lange offenen eigenen Jobs suchen.

Vorgesehener Startwert:

**7 Tage**

Ein alter Job wird dadurch nicht automatisch beendet.

Stattdessen fragt der Realm den Coordinator:

„Existiert Job `<ID>` noch?“

Antwort:

### Job existiert

Der Realm wartet weiter.

### Job existiert nicht

Der Realm kann den eigenen Auftrag kontrolliert beenden und die gespeicherten Items sowie Kosten zurückerstatten.

Dadurch wird verhindert, dass verwaiste Jobs dauerhaft in Realm-Datenbanken verbleiben.

Gleichzeitig darf ein langsamer, aber noch vorhandener Coordinator-Job nicht versehentlich vom Realm beendet werden.

---

## 26. Keine Coordinator-Abfragen in Realm-Datenbanken

Der Coordinator darf zur Prüfung eines Jobs niemals selbst auf Realm-Datenbanken zugreifen.

Die Kommunikation erfolgt ausschließlich über definierte Nachrichten zwischen Realm und Coordinator.

Der Realm kann den Coordinator nach einer Job-ID fragen.

Der Coordinator beantwortet ausschließlich, ob dieser Job innerhalb seines eigenen Zuständigkeitsbereichs noch vorhanden ist.

Der Realm interpretiert diese Antwort anhand seines eigenen Datenbestands.

---

## 27. Support und Nachvollziehbarkeit

Die eindeutigen Job-IDs sollen später auch für das Support-/Ticketsystem verwendet werden können.

Ein Realm kann anhand seiner eigenen Daten zu einem Charakter beispielsweise anzeigen:

* offene Jobs
* abgeschlossene Jobs
* fehlgeschlagene Jobs
* Jobinhalt
* beteiligter NPC
* abgegebene Item-IDs
* gezahltes Gold
* Ergebnis
* Fehlergrund
* Rückerstattungsstatus

Dadurch muss der Support nicht manuell verschiedene Logs und Tabellen durchsuchen.

Der Coordinator liefert nur die Informationen aus seinem eigenen Zuständigkeitsbereich.

---

## 28. Datenschutz und Job-Dateien

Coordinator-Job-Dateien sollen keine sensiblen Accountdaten enthalten.

Insbesondere nicht speichern:

* Passwörter
* Passwort-Hashes
* E-Mail-Adressen
* Auth-Tokens
* Session-Geheimnisse
* Datenbankzugangsdaten

Dadurch ist für die normalen Coordinator-Job-Dateien zunächst keine zusätzliche Inhaltsverschlüsselung vorgesehen.

Freie Spielertexte können allerdings vom Spieler selbst eingegebene persönliche Informationen enthalten.

Solche Inhalte sollen deshalb nicht unnötig langfristig gespeichert werden.

Provider-Zugangsdaten sind Service-Secrets:

API-Keys, Tokens und andere Provider-Zugangsdaten dürfen insbesondere nicht:

* an Clients übertragen werden
* Bestandteil von Realm-Jobs sein
* in Git eingecheckt werden
* in normalen Logs erscheinen
* unnötig in Realm-/Gameplay-Datenbanken gespeichert werden

Die konkrete Secret-Verwaltung wird bei der späteren Implementierung festgelegt.

---

## 28.1 Persistenter Erinnerungsspeicher

Der Coordinator speichert NPC-Erinnerungen und Beziehungen persistent in einem separaten dateibasierten Speicherbereich.

Dieser Speicher ist getrennt von der Queue.

Gründe:

* Erinnerungen dürfen nicht verloren gehen
* Ein Coordinator-Neustart darf Erinnerungen nicht löschen
* Der Coordinator besitzt weiterhin keinen Datenbankzugriff
* Stabile interne IDs verhindern, dass Namensänderungen Erinnerungen zerstören

Enthalten sind unter anderem:

* persönliche Erinnerungen pro Character-ID und NPC
* Beziehungsstatus zwischen Charakteren und NPCs
* Shared Knowledge pro Character-ID

Der Erinnerungsspeicher enthält keine Accountdaten, keine Passwörter, keine Items und keine Goldstände.

Er ist kein Ersatz für Realm-Datenbanken und enthält keine Spielzustände, die der Realm autoritativ verwaltet.

---

## 29. Zentrale Architekturregel

Die Verantwortlichkeiten bleiben strikt getrennt:

**Realmserver**

* Spielzustand
* Charaktere
* Items
* Gold
* Crafting-Aufträge
* Recovery
* Mail-System
* endgültige Spielentscheidungen

**Coordinator**

* keine Datenbankrechte
* KI-Job-Dateien
* zentrale KI-Queue
* Priorisierung
* Spam-/Lastschutz
* Input-Prüfung
* Provider-Kommunikation (Provider-Schicht, aktuell: Ollama)
* Output-Prüfung
* begrenzte Korrekturschleifen
* Fehler-/Statusmeldung an den Realm
* persistenter Erinnerungsspeicher (dateibasiert, kein DB-Zugriff)

**KI-Provider (aktuell lokale Standard-/Basislösung: Ollama; später auch externe Provider möglich, siehe §3.1)**

* erzeugt dynamische KI-Antworten und Vorschläge
* besitzt keine Autorität über den Spielzustand

Grundsatz:

**Der Coordinator verarbeitet KI. Der Realm verwaltet das Spiel.**
