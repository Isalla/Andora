# Coordinator – KI-Queue und Ollama-Schnittstelle

## 1. Zweck

Der Coordinator ist die zentrale Schnittstelle zwischen den Andora-Realmservern und Ollama.

Seine Aufgabe ist ausschließlich die kontrollierte Verarbeitung von KI-Anfragen.

Der Coordinator ist kein Gameserver, kein Loginserver, keine Account-API und keine zentrale Spieldatenbank.

Grundprinzip:

**Realm → Coordinator → Ollama → Coordinator → Realm**

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

Seine lokale Persistenz besteht ausschließlich aus seinen Queue-/Job-Dateien und technischen Logs.

Der Coordinator darf keine Spielzustände direkt verändern.

**Ollama erzeugt Vorschläge bzw. Antworten. Der Coordinator prüft und vermittelt diese. Der Realm entscheidet über alle spielmechanischen Auswirkungen.**

---

## 3. Zentrale Ollama-Schnittstelle

Realmserver greifen nicht direkt auf Ollama zu.

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
* Ollama-Verfügbarkeit
* Recovery

Ollama muss dadurch nicht gleichzeitig unabhängige Verbindungen von allen Realmservern verwalten.

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

laufen vollständig über normalen Realm-Spielcode und dürfen nicht unnötig an Ollama gesendet werden.

---

## 7. Begrenzung der Texteingabe

Freie Texteingaben von Spielern werden zunächst auf maximal:

**500 Zeichen**

begrenzt.

Die Prüfung soll nicht ausschließlich dem Client vertraut werden.

Die Grenze soll mindestens erneut auf Realm-Seite und beim Coordinator geprüft werden.

Zu lange Eingaben werden nicht an Ollama weitergegeben.

Der Coordinator kann beispielsweise antworten:

`INPUT_TOO_LONG`

Die 500-Zeichen-Grenze betrifft die freie Texteingabe des Spielers, nicht den gesamten internen KI-Kontext.

---

## 8. Kontextbudget

Für Ollama ist zunächst ein Kontext von ungefähr:

**8k Tokens**

vorgesehen.

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

Bevor eine Spieleranfrage an Ollama geschickt wird, prüft der Coordinator den Inhalt gegen zentrale Regeln.

Dadurch wird verhindert, dass jeder einzelne Realm sämtliche Inhaltsregeln selbst vollständig implementieren muss.

Die Prüfung kann beispielsweise erkennen:

* nicht erlaubte Inhalte
* für Andora unzulässige Gegenstände
* offensichtlich ungültige Anforderungen
* Manipulationsversuche
* ungültiges Format
* überschrittene Eingabelimits

Der Coordinator entscheidet dabei nicht über Realm-Spielzustände.

---

## 10. Output-Prüfung

Auch Ollamas Antwort wird vor der Weitergabe an den Realm geprüft.

Dabei können sowohl Inhaltsregeln als auch definierte Welt-/Plausibilitätsregeln geprüft werden.

Beispiel:

Ollama schlägt für einen Schmied ein 20 Meter langes Schwert vor.

Der Coordinator erkennt, dass die vorgeschlagenen Eigenschaften außerhalb der erlaubten Andora-Regeln liegen.

Die Antwort wird nicht unmittelbar verworfen.

Stattdessen erhält Ollama einen Korrekturauftrag und soll eine regelkonforme Alternative erzeugen.

Grundprinzip:

**Nicht sofort ablehnen, sondern innerhalb eines begrenzten Rahmens korrigieren lassen.**

---

## 11. Begrenzte Korrekturschleife

Coordinator und Ollama dürfen niemals unbegrenzt in einer Korrekturschleife hängen.

Startwert:

**maximal 5 Korrektur-/Wiederholungsversuche pro Job**

Nach Erreichen des Limits wird der Job beendet.

Der Realm erhält lediglich einen technischen Fehlerstatus, beispielsweise:

`VALIDATION_FAILED`

Der Realm verwendet anschließend eine fest definierte lokale Fallback-Reaktion.

Der Fallback darf niemals selbst eine neue KI-Anfrage erzeugen.

---

## 12. Fehlende oder leere Ollama-Antwort

Nach jeder Ollama-Anfrage muss geprüft werden, ob tatsächlich eine verwertbare Antwort vorhanden ist.

Als ungültig gelten insbesondere:

* keine Antwort
* leere Antwort
* ausschließlich Whitespace
* unvollständige strukturierte Antwort
* nicht parsebares erwartetes Format

Dies kann beispielsweise auftreten, wenn eine Anfrage das verfügbare Kontextlimit überschreitet oder Ollama die Generierung abbricht.

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
* fehlender/leer gebliebener Ollama-Antwort

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

Für sämtliche temporären Dateien gelten die allgemeinen Andora-Projektregeln.

Es darf kein systemweites `/tmp` verwendet werden.

Temporäre Dateien gehören ausschließlich in den projektinternen `.tmp`-Bereich.

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

Ein hartes Timeout soll verhindern, dass ein hängender Ollama-Aufruf den Shutdown unbegrenzt blockiert.

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
* Ollama-Kommunikation
* Output-Prüfung
* begrenzte Korrekturschleifen
* Fehler-/Statusmeldung an den Realm

**Ollama**

* erzeugt dynamische KI-Antworten und Vorschläge
* besitzt keine Autorität über den Spielzustand

Grundsatz:

**Der Coordinator verarbeitet KI. Der Realm verwaltet das Spiel.**
