# Andora – Player-Persistenzstrategie (laufende Spielerzustände)

## 1. Status

**Player-Persistenz Stufe A (Dirty-State, periodischer Player-Save, Disconnect-/Shutdown-Flush) ist implementiert und abgeschlossen.**

**Die Spool-/Recovery-Architektur (Stufe B) ist implementiert und in der Datei dokumentiert.**

Diese Datei definiert die allgemeine Persistenzstrategie für laufende Spielerzustände des Realm-Servers (`src/realm-rs`).

Sie ergänzt die bestehenden, weiterhin autoritativen Regeln:

* `Datenbank_Architektur.md` – Datenbankaufteilung (`auth`, `realm_state_<realm>`), Realm-Isolation, Backups
* `Quest-System.md` – insbesondere Abschnitt 27.26 (atomarer/transaktionaler Questabschluss)
* `inventory_system.md` – Inventar-Persistenz (Transaktions-Vollwrite, Sicherheits-Puffer)
* `Deployment_Betriebsarchitektur.md` – insbesondere §7 Realm-Updates (Wartungsmodus → Shutdown → Backup → Update → Migration → Healthcheck → Freigabe)
* `Erfahrung_und_Progressionssystem.md` – EXP-Persistenz (derzeit bei Disconnect; Einzelspeicherpunkte dort §2)

Diese Datei führt für normale, laufende Spielerzustände eine Dirty-State-Strategie mit periodischem Flush ein. Sie ändert die bestehenden Sofort-Persistenzgarantien kritischer Vorgänge nicht (siehe Abschnitt 9).

---

## 2. Ziel

Eine allgemeine Persistenzstrategie für laufende Spielerzustände.

Grundprinzip:

* **RAM** ist während einer laufenden Realm-Session die autoritative Live-Repräsentation des Spielerzustands.
* Normale persistente Änderungen werden als **dirty** markiert.
* Dirty-Zustände werden **periodisch über eine lokale Persistence-Spool** (Stufe B, Abschnitt 21) **nach MariaDB** geschrieben.
* **Kritische/irreversible Transaktionen** werden weiterhin **sofort** persistiert.
* **Disconnect** führt zu einem finalen Player-Save.
* **Graceful Shutdown** führt zu einem finalen Save der verbundenen Spieler.

Die Strategie soll unnötige DB-Writes reduzieren und gleichzeitig den möglichen Fortschrittsverlust bei einem ungeplanten Realm-Crash begrenzen.

---

## 3. RAM als autoritative Live-Repräsentation

Für die Dauer einer laufenden Realm-Session gilt:

> Der Realm-RAM ist die autoritative Live-Repräsentation des Spielerzustands.

Ein Spieler besitzt während seiner Session einen kompletten, konsistenten persistenzen Abbildzustand im Realm-RAM (Spielposition, Progression, Idia, Inventar, Questzustand/-fortschritt). Das Spiellayer arbeitet ausschließlich gegen diese RAM-Repräsentation.

MariaDB ist kein „Live-Backup" des RAM: Nicht jede RAM-Änderung wird unmittelbar nach MariaDB geschrieben. Die DB hält den zuletzt erfolgreich persistierten Stand und wird über die in dieser Datei beschriebenen Flush-Wege aktualisiert.

---

## 4. Periodischer Player-Save (Intervall)

Der Realm erhält einen **konfigurierbaren periodischen Player-Save**.

**Startwert:** `900 Sekunden` (15 Minuten).

Passende Config-Bezeichnung (konsistent zur bestehenden Umgebungsvariablen-Konvention, vgl. `NPC_PERSIST_INTERVAL_MS` in `config.rs`):

```text
PLAYER_PERSIST_INTERVAL_MS=900000
```

Die konkrete Einbindung des Config-Keys in `config.rs` und die genaue Konfigstruktur sind Sache des Coding-Auftrags. Ist der Key nicht gesetzt, gilt der Default `900000` (900 Sekunden).

Der Wert ist ausdrücklich ein **Startwert**. Er darf später angepasst werden aufgrund von:

* Lasttests
* DB-Messwerten
* Spielerzahlen
* Live-Betrieb
* beobachtetem Crash-Verlust

Für V1 wird **keine adaptive Autosave-Logik** festgelegt (kein dynamisches, per-Spieler lernendes Intervall, keine Last-abhängige Anpassung).

---

## 5. Dirty-State

Normale Änderungen lösen **nicht automatisch sofort** einen DB-Write aus.

Stattdessen wird die betroffene persistente Spielerkomponente als **dirty** markiert.

Beispiele:

```text
Questfortschritt:   12/100 → 13/100   (Persistenz: Quest-Persistenzpfad, Abschnitt 8)
EXP-Zuwachs
Idia-Änderungen
normale Inventaränderungen
Positionsänderungen
```

Diese Änderungen existieren unmittelbar im autoritativen Realm-RAM.

Die Persistenz erfolgt beim nächsten passenden Flush (periodisch, Disconnect oder Graceful Shutdown).

---

## 6. Komponentenbezogenes Dirty-State

Die Architektur unterscheidet Dirty-State nach **persistenter Komponente**.

Konzeptionell:

```text
position dirty
progression dirty
idia dirty
inventory dirty
quest state/progress dirty
```

Nicht vorgeschrieben ist, dass dies zwingend fünf boolesche Felder sein müssen. Die konkrete Rust-Datenstruktur bleibt Implementierungsdetail (Abschnitt 20).

Ziel:

> Eine Änderung am Questfortschritt soll nicht unnötig einen vollständigen Inventar-Write erzwingen.

Die Komponente „inventory dirty" umfasst ausschließlich die persistenten
Inventarbestandteile (Grundinventar, Rucksäcke/Bag-Slots, Equipment). Der
temporäre Sicherheits-Puffer des Inventory-Systems (`inventory_system.md`
§10/§11) ist ausschließlich flüchtiger Runtime-State einer laufenden Session
und ausdrücklich **nicht** Teil der Dirty-/Persistenzpflicht: Er wird weder vom
periodischen noch vom finalen Player-Save geschrieben, unabhängig von dessen
Erfolg.

Die Persistenzwege der einzelnen Komponenten bleiben unverändert diejenigen aus der jeweils autoritativen Doku (z. B. Inventar als Transaktions-Vollwrite gemäß `inventory_system.md` §11).

Quest State / Quest Progress besitzen dagegen einen **eigenen direkten MariaDB-Persistenzweg** (Quest-Persistenzpfad, Abschnitt 8) und sind für Stufe B **nicht** Teil des normalen Player-Snapshot-/Spool-Systems (Abschnitt 23). Die bestehende Stufe-A-Dirty-Komponente für Quest wird nicht als verbindliche Stufe-B-Architektur übernommen (Abschnitt 8).

---

## 7. Periodischer Flush

Beim periodischen Save werden **nur tatsächlich dirty** gewordene persistente Spielerzustände geschrieben.

Ein Spieler, dessen persistenter Zustand sich seit dem letzten erfolgreichen Save nicht verändert hat, benötigt keinen unnötigen Player-State-Write.

Regeln:

* Nach **erfolgreicher** Persistierung darf der entsprechende Dirty-Zustand **zurückgesetzt** werden.
* Bei **fehlgeschlagener** Persistierung gilt der Zustand **nicht als sauber**: Er bleibt dirty und muss für einen späteren Retry verfügbar bleiben (Abschnitt 16).

In der Stufe-B-Architektur verläuft der periodische Persistenzlauf über die lokale Persistence-Spool (Abschnitt 21): Der erfasste PersistSnapshot wird zunächst sicher lokal geschrieben und von dort mit dem aktuellen Persistenzcode nach MariaDB übertragen.

Im Sinne dieses periodischen Pfads gilt eine Persistenz erst dann als dauerhaft gesichert, wenn der Snapshot sicher in der Spool liegt (Abschnitt 35). Der anschließende Transfer nach MariaDB ist davon fachlich entkoppelt (Abschnitt 38); die Erkennung von Races zwischen persistierenden Läufen erfolgt über die `persist_generation` (Abschnitt 39).

Für den Stufe-B-Spool-Pfad gilt die Granularität **pro Charakter**: Ist ein Charakter dirty, enthält sein Player-Eintrag im Batch einen **vollständigen Snapshot** aller persistenten Player-Komponenten des normalen Player-Persistence-Systems, nicht nur die unmittelbar geänderte Komponente (Abschnitt 23).

---

## 8. Questfortschritt

Quest State und Quest Progress gehören **nicht** zum normalen vollständigen Player-Snapshot des Stufe-B-Spool-Systems (Abschnitt 23). Quest-Persistence besitzt bewusst einen **eigenen direkten MariaDB-Persistenzweg** (Quest-Persistenzpfad). Der normale Player-Spool darf deshalb einen neueren Quest-Zustand in MariaDB **niemals** durch einen älteren Quest-Zustand aus einem Player-Snapshot überschreiben.

Quest-Persistence erfolgt **nicht bei jeder kleinsten Queständerung**.

Beispiel:

```text
Quest: Töte 100 Wölfe.
```

Wolf 1/100 → 2/100 → 3/100 usw. muss **nicht nach jedem einzelnen Kill** unmittelbar in MariaDB geschrieben werden.

Sondern:

```text
Kill bestätigt
→ Questfortschritt im autoritativen Realm-RAM erhöhen
→ im Realm-RAM halten bis zum nächsten relevanten Quest-Checkpoint
```

Normaler flüchtiger Quest-Fortschritt zwischen relevanten Quest-Checkpoints wird im Realm-RAM gehalten.

Relevante Quest-Checkpoints werden dagegen **direkt** über den Quest-Persistenzpfad persistiert. Dazu gehören insbesondere:

* Quest Acceptance (Abschnitt 10)
* relevante Quest-Gespräche mit NPCs
* Quest-Aktualisierungen, die durch einen NPC bzw. ein entsprechendes Quest-Ereignis ausgelöst werden
* Quest Completion (Abschnitt 9)

Die genaue Liste aller zukünftigen Quest-Checkpoint-Typen wird in dieser Datei bewusst nicht festgelegt.

Bei einem ungeplanten Prozess-/Host-Crash kann Quest-Fortschritt seit dem letzten erfolgreich persistierten Quest-Checkpoint verloren gehen. Das ist für normalen Zwischenfortschritt **bewusst akzeptiert**.

**Hinweis zur bestehenden Stufe-A-Implementierung:**

Im bestehenden Rust-Code sind Quest State/Quest Progress derzeit noch Teil des bisherigen Player-Persistence-Pfads (`PersistComponent::QuestState`, Quest-Daten im bisherigen `PersistSnapshot`, `write_quest_state` im zentralen Stufe-A-Persistence-Pfad). Diese vorhandene Implementierung darf ausdrücklich **nicht** als neue verbindliche Stufe-B-Architektur übernommen werden. Für Stufe B gilt: Quest State/Quest Progress sind aus dem normalen Player-Spool ausgegliedert und werden über den Quest-Persistenzpfad behandelt. Die spätere Bereinigung/Anpassung des vorhandenen Stufe-A-Codes erfolgt in einem separaten Implementierungsauftrag.

---

## 9. Kritische Transaktionen (sofort persistiert)

Nicht alle Zustandsänderungen dürfen auf den periodischen Save warten.

Kritische bzw. irreversible Vorgänge werden weiterhin **unmittelbar** persistent abgeschlossen.

Insbesondere:

**QUESTABSCHLUSS**

Ein erfolgreicher Questabschluss inklusive seiner beteiligten persistenten Änderungen bleibt eine **sofortige atomare Transaktion** gemäß `Quest-System.md` §27.26 und der dafür implementierten Questabschlussarchitektur.

> Der periodische Save darf diese Sicherheitsgarantie NICHT ersetzen oder abschwächen.

---

## 10. Questannahme

Die Annahme einer Quest ist ein bedeutender Quest-State-Übergang:

```text
AVAILABLE → ACTIVE
```

Dieser Zustandsübergang soll **unmittelbar** persistent gespeichert werden.

Normaler Objective-Fortschritt innerhalb ACTIVE wird anschließend im Realm-RAM gehalten und erst bei den relevanten Quest-Checkpoints über den Quest-Persistenzpfad persistiert (Abschnitt 8); er gehört nicht zum normalen Player-Snapshot-/Spool-Pfad (Abschnitt 23).

Aus dieser Regel werden **keine** weiteren Repeatable-/FAILED-/Abort-Regeln abgeleitet (die entsprechenden Semantiken bleiben in `Quest-System.md` offen, vgl. Abschnitte 27.20/27.21).

---

## 11. Disconnect

Bei einem normalen Spieler-Disconnect erfolgt ein **finaler Flush** der persistenzpflichtigen Dirty-Zustände des Spielers.

Die Architektur verwendet nach Möglichkeit **denselben zentralen Player-Persistenzpfad** wie der periodische Save.

Es werden keine voneinander abweichenden Persistenzregeln für dieselben Komponenten dupliziert: Der Disconnect-Save ist ein sofort ausgelöster Flush über denselben Pfad, nicht ein Satz eigener, paralleler Speicherlogik.

**LOGOUT_AT**

`logout_at` ist **kein** normaler periodischer Dirty-/Snapshot-Wert. Es besitzt einen **eigenen direkten Logout-Persistenzzeitpunkt**.

Beim regulären Logout gilt konzeptionell:

```text
Spieler beginnt Logout
→ finaler Player-Persistence-Vorgang
→ logout_at wird als Teil des Logout-Abschlusses direkt für MariaDB behandelt
→ erst danach wird die Runtime-/Session-Repräsentation entfernt
```

`logout_at` wartet NICHT auf den nächsten regulären 15-Minuten-Snapshot (Abschnitt 4) und benötigt deshalb kein normales Dirty-Bit (Abschnitt 5).

WICHTIG:

Der finale Player-Zustand beim Logout muss weiterhin den dokumentierten Sicherheitsregeln der Player Persistence entsprechen. Wenn MariaDB beim Logout nicht erreichbar ist, darf der finale persistente Player-Zustand nicht einfach verloren gehen (Abschnitte 16 und 30).

Die genaue technische Koordination zwischen dem finalen Spool-Snapshot und dem direkten `logout_at`-DB-Eintrag wird hier nicht festgelegt (Abschnitt 42).

**ZURÜCKSETZEN VON `logout_at` BEIM LOGIN**

`logout_at` wird nach der obigen Regel ausschließlich beim Logout gesetzt. Es wird **beim Login** zurückgesetzt, um die einmalige Offline-/Rested-Berechnung (Abschnitt 12 in `Erfahrung_und_Progressionssystem.md`) nicht zu wiederholen.

VERBINDLICH:

* Das Zurücksetzen und die zugehörige Gutschrift gehören fachlich zu einem **erfolgreichen Realm-Login**. Wann dieser erreicht ist, legt `Login_Realm_Architektur.md` (Abschnitt „Erfolgszeitpunkt des Realm-Logins") fest: erst wenn der Server-Commit eine Owner-Zuordnung hergestellt hat.
* **Schlägt der Einstieg vor diesem Commit fehl** — insbesondere bei Quest- oder Datenladefehlern, bei `BLOCKED` oder nicht verfügbarer Elternkontrolle sowie bei einem Registry- oder Account-Konflikt —, bleiben der vorherige `logout_at` und der noch nicht konsumierte Offline-Zeitraum fachlich erhalten. Es wird weder `logout_at` zurückgesetzt noch Offline-Zeit als verbraucht gebucht; die Rested-Berechnung erfolgt dann beim nächsten erfolgreichen Login.
* **Schlägt erst nach dem Commit `WELCOME` oder die Verbindung aus**, gilt der Login als zustande gekommen. Der Disconnect-Pfad dieser Verbindung schreibt anschließend den neuen Logout-Zeitpunkt; es findet keine Rücknahme des Einstiegs statt.
* Der Handoff-Single-Use bleibt davon unberührt: Er wird weiterhin vor dem fachlichen Realm-Login verbraucht und bei einem fehlgeschlagenen Einstieg nicht zurückgenommen (bestehende Auth-Semantik, siehe `Login_Realm_Architektur.md`).

**Abrechnungszeitpunkt und Unterbrechungsausnahme**

* Das Zurücksetzen und die zugehörige Gutschrift erfolgen als **ein gemeinsamer, unteilbarer Schritt unmittelbar vor** der Herstellung der Owner-Zuordnung — nach Abschluss aller für den Einstieg als **blockierend definierten** Vorprüfungen, einschließlich der erforderlichen Datenladeprüfungen und der Elternkontrolle. Damit wird keine neue Fail-closed-Regel für bisher tolerierte Ladefehler eingeführt.
* **Schlägt die Abrechnung fehl, wird der Login nicht committet.** Der vorherige `logout_at` und der noch nicht konsumierte Offline-Zeitraum bleiben dann vollständig erhalten.
* **At-most-once-Unterbrechungsausnahme** (Kurzname: Crash-Ausnahme): Bricht der Vorgang **abrupt** zwischen erfolgreichem DB-Commit der Abrechnung und der Herstellung der Owner-Zuordnung ab — insbesondere bei Prozessabsturz sowie, sofern der Handler an dieser Stelle abbrechbar ist, bei Task-Abbruch oder Panic —, bleibt die bereits gebuchte Gutschrift bestehen und `logout_at` bleibt zurückgesetzt; der Login gilt technisch nicht als zustande gekommen. **Die Gutschrift bleibt gebucht**, und es entsteht **keine Mehrfachgutschrift**: gegenüber der regulär vorgesehenen Abrechnung ergibt sich weder ein Wertverlust noch eine Doppelnachbuchung. Zur spielwertbezogenen Einordnung siehe `Erfahrung_und_Progressionssystem.md` (Abschnitt 12.6).

**FEHLERSEMANTIK DES DIREKTEN `logout_at`-SCHREIBENS**

Der direkte Logout-Write ist der einzige Weg, auf dem ein Logout-Zeitpunkt in MariaDB entsteht (Spool und Recovery schreiben ihn nicht; siehe Abschnitt 11 oben). Für seinen Fehlerfall gilt daher eine eigene, verbindliche Regel:

* **Begrenzter unmittelbarer Retry.** Schlägt das direkte Schreiben fehl, wiederholt der Server den Versuch **zeitlich und zahlenmäßig begrenzt** unmittelbar. Der Disconnect darf dadurch **nicht** unbegrenzt blockiert werden.
* **Derselbe Zeitpunkt.** Alle Wiederholungen verwenden den **bereits ermittelten** Logout-Zeitpunkt. Während der Retries wird **kein neuerer** Zeitpunkt erzeugt — der Logout-Zeitpunkt ist der Moment des Verbindungsendes, nicht der Moment des Schreibens.
* **Schutzmechanismen unverändert.** Der per-player-Gate- und der Owner-Schutz bleiben über alle Versuche erhalten; ein Retry ist kein Wiederholen des Logout-Ablaufs und darf keine zweite Eigentümer-Zuordnung erzeugen.
* **Parameter offen.** Die konkrete Zahl der Versuche, die Abstände und das Zeitbudget werden **nicht** hier festgelegt, sondern erst im Coding-Plan anhand der vorhandenen DB-Timeouts.

Sind alle Versuche erfolglos, gilt:

* Der Fehler wird **strukturiert** mit Charakter-ID, Versuchszahl und Fehlerklasse geloggt. **Keine** Session-ID, **keine** Tokens, **keine** Roh-IP.
* Der Realm-Betriebsstatus wird auf `Degraded` gesetzt (oder ein vorhandener gleichwertiger Degraded-Pfad verwendet).
* Der Cleanup läuft **kontrolliert weiter**; der Logout-Ablauf wird nicht abgebrochen und nicht wiederholt.
* Es erfolgt **kein** Bann und **keine** spielerseitige Sanktion.
* Der Write gilt **nicht** als stillschweigend erfolgreich.

**RESTRISIKO NACH AUSGESCHÖPFTEM RETRY** (bewusst akzeptiert)

Bleibt der Write endgültig erfolglos, ist die Folge fachlich festgelegt und abgenommen:

* Nach einem erfolgreichen Login steht `logout_at` regelmäßig auf `NULL` (der Login schreibt es gemeinsam mit der Rested-Gutschrift zurück, siehe „ZURÜCKSETZEN VON `logout_at` BEIM LOGIN" oben). Der nächste Login kann deshalb **keinen** Offline-Beginn bestimmen.
* Der vorhandene Rested-Pool bleibt **erhalten**; es wird nichts von ihm abgezogen.
* Der Zuwachs für diese Offline-Periode **geht verloren**. Er wird nicht später nachgeholt.
* Es entsteht **keine** Doppelgutschrift und **kein** wirtschaftlicher Vorteil; der Fehler ist für den betroffenen Spieler ein Nachteil, nicht ein Vorteil.
* Die Höhe des Verlusts ist durch den Rested-Deckel begrenzt und damit gedeckelt.
* Dies ist ein **bewusst akzeptiertes niedriges Persistenz-/Fairnessrestrisiko**.

**NICHT BESTANDTEIL DIESER REGEL**

* Spool und Recovery reparieren den direkten Logout-Write **nicht** — `logout_at` gehört nicht in den Persistenz-Snapshot.
* Ein `mark_dirty` ist **kein** Retry für `logout_at`; es erzeugt einen Snapshot, der den Logout-Zeitpunkt nicht enthält.
* Für P-27 werden ausdrücklich **nicht** eingeführt: `logout_at` im Persistenz-Snapshot, eine Änderung des Spool-Drahtformats, eine neue Datenbankspalte, ein persistenter Retry-Marker, ein RAM-Marker als vermeintlich dauerhafte Reparatur sowie Schema- oder Migrationsänderungen. Ein späteres Release-Audit darf eine dauerhafte Retry-/Recovery-Lösung erneut bewerten.
* Die At-most-once-Unterbrechungsausnahme des Login-Pfads (oben) ist **nicht** Teil dieser Regel.

Ob und wie dieser Zeitpunkt im Code umgesetzt wird (Reihenfolge der Schreibvorgänge, technische Transaktionsform, Verhalten bei Schreibfehlern) ist hier **nicht** festgelegt; der offene sicherheitstechnische Befund dazu steht in `docs/Security.md` unter `P-32`.

---

## 12. Graceful Shutdown

Bei einem kontrollierten Realm-Shutdown:

```text
1. keine neuen Spieler mehr aufnehmen
2. laufende Gameplay-Verarbeitung kontrolliert stoppen/einfrieren
3. konsistente Zustände der verbundenen Spieler erfassen
4. deren persistenzpflichtige Zustände final speichern
5. erst danach die benötigten DB-Ressourcen schließen
6. Realm beenden
```

Ein kontrollierter Shutdown soll möglichst **keinen normalen Spielerfortschritt verlieren**.

Der Questabschluss bleibt davon unabhängig bereits sofort persistent (Abschnitt 9).

Der Realm-Shutdown ist im Realm-Update-Ablauf bereits vorgesehen (Wartungsmodus → Shutdown → Backup → Update → Migration → Healthcheck → Freigabe, `Deployment_Betriebsarchitektur.md` §7). Die hier definierte Final-Save-Pflicht gilt für jeden kontrollierten Realm-Shutdown, einschließlich des Update-Ablaufs.

---

## 13. Crash-Verhalten

Bei einem **ungeplanten** Realm-/Host-Crash kann normaler, noch nicht periodisch persistierter Dirty-State verloren gehen.

Bei Default `900 Sekunden` bedeutet das konzeptionell:

> maximal ungefähr der seit dem letzten erfolgreichen periodischen Save entstandene normale Zwischenfortschritt.

Es wird nicht garantiert, dass der Verlust exakt höchstens 900 Sekunden beträgt, da beispielsweise ein DB-Fehler einen erfolgreichen Flush verhindern kann.

Kritische, bereits erfolgreich committete Transaktionen bleiben davon unberührt (Abschnitt 9).

Bereits sicher in der Persistence-Spool gesicherte Snapshots überstehen einen Prozess-/Host-Crash und werden beim nächsten Realm-Start im Rahmen der Recovery verarbeitet (Abschnitt 28).

---

## 14. DB-Last

Ziel der Strategie ist ausdrücklich:

> nicht jede kleine Gameplayänderung als einzelnen DB-Write auszuführen.

Insbesondere muss nicht unmittelbar persistent werden:

* jeder Kill-Questfortschritt (persistiert wird erst an relevanten Quest-Checkpoints über den Quest-Persistenzpfad, Abschnitt 8)
* jeder EXP-Punkt
* jede Positionsänderung

Dirty-State und Batch-/Periodic-Flush reduzieren den Schreibdruck.

Es werden keine konkreten maximalen Spielerzahlen oder DB-Write-Raten erfunden.

---

## 15. Snapshot / Lock-Dauer (Race-Regel)

Der periodische Persistenzvorgang soll den zentralen World-/Gameplay-Lock **nicht unnötig während langsamer DB-I/O** halten.

Bevorzugtes Prinzip:

```text
konsistenten persistierbaren Zustand erfassen/snapshotten
→ Gameplay-Lock freigeben
→ DB-Persistierung durchführen
```

**ABER:** Beim späteren Zurücksetzen von Dirty-State muss verhindert werden, dass eine Änderung verloren geht, die **NACH dem Snapshot aber VOR erfolgreichem DB-Write** entstanden ist.

Beispiel:

```text
Snapshot enthält Idia 20.
Während des DB-Writes steigt RAM auf 21.
```

Nach erfolgreichem Write von 20/100 darf der neue 21/100-Zustand **nicht versehentlich als clean** markiert werden. Diese Race-Bedingung wird über eine `persist_generation` des laufenden Realm-Prozesses erkannt und abgesichert (Abschnitt 20; Details und Verhalten in Abschnitt 39).

Der dabei erfasste konsistente Zustand entspricht dem PersistSnapshot der Stufe-B-Spool-Architektur (Abschnitt 21).

---

## 16. Save-Fehler

Ein fehlgeschlagener periodischer Save:

* darf den Realm **nicht automatisch beenden**
* muss **geloggt/telemetriert** werden
* lässt die betroffenen Zustände **dirty** (Abschnitt 7)
* darf von einem späteren Flush **erneut versucht** werden

Konkrete Retry-Abstände/-Anzahlen werden **nicht** festgelegt.

Für Graceful-Shutdown-Fehler wird die Regel in **Abschnitt 30** festgelegt (begrenzter Retry, zusammengefasster Abschlussbericht, kontrolliert beendeter Prozess).

Das **direkte Schreiben des `logout_at`** ist vom periodischen Save getrennt geregelt: Es ist **kein** Snapshot-Wert und folgt der eigenen Regel in **Abschnitt 11** (begrenzter unmittelbarer Retry, danach `Degraded` und kontrolliert weiterlaufender Cleanup). Ein `mark_dirty` ist **kein** Retry für `logout_at`, und Spool/Recovery reparieren diesen Write nicht.

In der Stufe-B-Architektur bleibt der Spool-Snapshot bei fehlgeschlagener DB-Übertragung lokal erhalten; der Realm läuft weiter und sein Persistence-Zustand wird DEGRADED (Details: Abschnitt 26).

---

## 17. Monitoring

Die Architektur soll später mindestens beobachtbar machen können:

* erfolgreiche periodische Saves
* fehlgeschlagene Saves
* Save-Dauer
* Anzahl persistierter Spieler/Komponenten
* Dirty-Zustände bzw. Backlog, soweit sinnvoll

Es wird **keine konkrete Dashboard-UI** in dieser Datei entworfen. Die vorhandene Monitoring-Schnittstelle (`monitoring_web_panel.md`, `/status`-Metriken) ist der natürliche Anzeige-Ort; die konkreten Metrikfelder legt der Coding-Auftrag fest.

Die Spool-/Recovery-spezifischen Beobachtungsanforderungen der Stufe B (Persistence-Status auf Server-/Realm-Ebene, ausstehende Spool-Snapshots, Quarantänefälle, Push-Konzept) sind in Abschnitt 27 dokumentiert.

---

## 18. V1-bewusste Einfachheit

Für V1 ausdrücklich **keine**:

* adaptive Save-Intervalle
* komplexe dynamische DB-Laststeuerung
* per-Spieler individuell lernende Intervalle
* unnötige zusätzliche Persistenzdienste

V1-Start:

```text
Dirty-State
+
konfigurierbares periodisches Intervall
+
sofortige kritische Transaktionen
+
finaler Disconnect-Save
+
Graceful-Shutdown-Flush
```

Später anhand realer Messwerte optimieren.

---

## 19. Ausdrücklich nicht Teil dieser Doku

Diese Datei ändert **keinen** Code und legt **keine** neuen Gameplayregeln fest. Ausdrücklich unberührt:

* Rust-Implementierung
* MariaDB-Migrationen
* Lua
* Godot
* Netzwerkprotokoll
* **Weltzeit und Wetter** (persistenter Realm-/Weltzustand, keine spielergebundenen Zustände; getrennt behandelt in `Weltzeit_und_Wettersystem.md`)

---

## 20. Bewusst offen gelassene technische Details

Folgende Punkte werden im separaten Coding-/Architekturauftrag anhand der bestehenden Serverarchitektur entschieden und hier bewusst **nicht** festgelegt:

* konkrete Rust-Dirty-Datenstruktur
* konkrete `persist_generation`-Datenstruktur/-Implementierung (Mechanismus und Verhalten selbst sind festgelegt: Abschnitt 39)
* exakte Save-Batchgröße
* konkrete Retry-Zeiten
* Parallelität/Worker-Anzahl der DB-Spool-Abarbeitung – für V1 ist **sequentielle** Verarbeitung festgelegt (Abschnitt 37)
* genaue Shutdown-Timeouts
* adaptive Intervalle (für V1 generell nicht vorgesehen, Abschnitt 18)
* genaue Einbindung/Auswertung des `PLAYER_PERSIST_INTERVAL_MS`-Config-Keys in `config.rs`

Zusätzlich für Stufe B (Spool-/Recovery-Architektur) bewusst offen gelassene Punkte: Abschnitt 42.

---

## 21. Stufe B – Spool-/Recovery-Architektur (Grundarchitektur)

**Status:** Architektur dokumentiert und implementiert.

Die bisherige direkte Vorstellung

```text
RAM -> MariaDB
```

wird für die normale periodische Player-Persistenz um eine lokale, dauerhafte Spool-Schicht erweitert:

```text
Live Player-State im RAM
    -> dirty
    -> periodischer Persistenzlauf
    -> PersistSnapshot erzeugen
    -> Snapshot sicher lokal in Persistence-Spool schreiben
    -> RAM-Kopie des PersistSnapshots kann danach freigegeben werden
    -> Spool-Snapshot mit aktuellem Rust-Persistenzcode nach MariaDB übertragen
    -> nach erfolgreichem DB-COMMIT Spool-Datei löschen
```

Ein Persistenzlauf erzeugt **einen** Spool-Snapshot (Batch), der die Einträge aller dirty Spieler enthält (Batch-Format: Abschnitt 35). Entfernt wird eine Spool-Datei erst, wenn **alle** darin enthaltenen Einträge erledigt (committet oder per Revision als erledigt befundet) bzw. einzeln in Quarantäne oder nach `superseded/` überführt wurden (Abschnitt 36).

WICHTIG:

Der aktive Player-State selbst bleibt selbstverständlich im RAM, solange der Spieler online ist.

Freigegeben werden kann nur die zusätzliche Snapshot-Kopie, nachdem diese sicher auf dem lokalen Datenträger liegt.

Die lokale Spool ist:

* keine zweite Live-Datenbank,
* kein Ersatz für MariaDB,
* kein zweiter autoritativer Player-State.

Sie ist eine dauerhafte Übergabe-/Recovery-Schicht zwischen RAM und MariaDB.

MariaDB bleibt der endgültige persistente Datenspeicher (Abschnitt 3; `Datenbank_Architektur.md`). RAM bleibt während der laufenden Realm-Session die autoritative Live-Repräsentation (Absatz „Grundprinzip" in Abschnitt 2).

---

## 22. Stufe B – Persistenzintervall

Der bereits dokumentierte Standard (Abschnitt 4) bleibt gültig:

```text
PLAYER_PERSIST_INTERVAL_MS=900000
```

also 15 Minuten / 900 Sekunden.

Das Intervall bleibt konfigurierbar.

Für V1 gilt weiterhin **kein adaptives Persistenzintervall** (Abschnitt 18).

Das Intervall steuert die **Erzeugung** periodischer Spool-Snapshots; die Übertragung nach MariaDB ist davon fachlich entkoppelt und erfolgt unabhängig vom Persistenzintervall (Abschnitt 38). Zu einem Zeitpunkt wird höchstens **ein** Batch erzeugt.

---

## 23. Stufe B – Vollständiger persistenter Player-Snapshot

Sobald ein Charakter für einen regulären Persistence-Lauf **dirty** ist, wird in die Spool NICHT nur die unmittelbar veränderte Komponente geschrieben.

Der Begriff „vollständiger Player-Snapshot" bedeutet hier: ein vollständiger Snapshot aller persistenten Komponenten, die dem **normalen Player-Persistence-/Spool-System** zugeordnet sind. Daraus folgt ausdrücklich **nicht**, dass sämtliche persistenten Systeme des gesamten Spiels in diesem Snapshot enthalten sein müssen. Systeme mit eigenem verbindlichem Persistenzweg können ausdrücklich außerhalb dieses Snapshots liegen.

Quest-Persistence ist ein solcher separater Persistenzweg (Abschnitt 8).

**Grundregel des Full-Snapshots:**

Der Snapshot speichert nach Möglichkeit die **eigentliche persistente Ursache** und nicht zusätzlich jeden daraus berechenbaren Wert. Abgeleitete Werte werden beim Laden bzw. bei relevanten Änderungen aus ihren persistenten Grundlagen neu berechnet.

Der Player-Eintrag im Batch enthält für V1 einen **vollständigen Snapshot** dieser dem normalen Player-Persistence-/Spool-System zugeordneten Komponenten. Dazu gehören nach der bestehenden Architektur insbesondere:

**IM NORMALEN STUFE-B-PLAYER-SNAPSHOT:**

* Position
* Progression / Level / EXP und zugehöriger normaler Fortschritt
* Idia
* persistentes Inventar / Equipment
* dauerhafte Charakterattribute (z.B. Stärke, Weisheit, Glück, Ausdauer und weitere bestehende Charakterattribute; keine neuen Attribute erfinden)
* aktuelle HP (aktuell vorhandener Wert, z.B. 437)
* aktuelles Mana (aktuell vorhandener Wert, z.B. 126)
* Klasse (gewählte bzw. entwickelte Charakterklasse)
* persistenter Zustand der Fraktionswahl/-zugehörigkeit (`faction_transition`)
* dauerhaft trainierte Weapon Skills
* dauerhaft erlernte / freigeschaltete Abilities
* laufende Ability-Cooldowns als **Ablaufzeitpunkte** (verbindlich geregelt in „Cooldowns im Player-Snapshot" unten)
* Item-Lifecycle-Metadaten (ausstehende UUID-Abkopplungen zur revisionsgebundenen Instanzfinalisierung, `inventory_system.md` §18; kein eigenes Dirty-Bit — sie reisen im vollständigen Snapshot mit)

**Idia als absoluter Gesamtbestand:**

Die normale Währung der Spielwelt Andora heißt verbindlich **Idia**. Der gespeicherte Idia-Wert ist IMMER der absolute aktuelle Gesamtbestand, den der Charakter zum Zeitpunkt des Snapshots besitzt.

Beispiel:

```text
Charakter besitzt zunächst:   8.500 Idia
Verkauf:                      +1.200 Idia
Kauf:                           -300 Idia
Aktueller Zustand:             9.400 Idia

Snapshot:  idia = 9400
```

Der Snapshot enthält NICHT die Einzeländerungen `+1200`, `-300` oder irgendein anderes Delta. Recovery stellt den absoluten Snapshot-Zustand wieder her. Dadurch darf die wiederholte Verarbeitung eines Zustands niemals dazu führen, dass eine vorherige Idia-Änderung erneut addiert oder subtrahiert wird. Die bestehenden `persist_revision`-Regeln (Abschnitt 29) bleiben hierfür maßgeblich.

**Gold → Idia (Legacy-Hinweis):**

Die bestehende Implementierung verwendet derzeit an mehreren Stellen noch `gold`/`Gold`. Das ist Legacy-Terminologie des aktuellen Codes und NICHT die gewünschte dauerhafte Andora-Terminologie. Für die normale Spielerwährung gilt verbindlich der Name **Idia**.

In einem späteren separaten Implementierungsauftrag soll die normale Spielerwährung projektweit konsistent auf Idia umgestellt werden, insbesondere `gold`/`Gold`, `PersistComponent::Gold`, entsprechende Rust-Felder, entsprechende MariaDB-Felder sowie Loot-/Persistence-Verwendungen. Dabei müssen alle tatsächlichen Vorkommen im Projekt geprüft werden, damit Dokumentation, Rust-Code, MariaDB-Schema, Persistence, Loot und Tests nicht unterschiedliche Namen für dieselbe Währung verwenden.

Dieser Dokumentationsauftrag führt die Umbenennung NICHT durch (keine Code-Umbenennung, keine DB-Umbenennung, keine Migration). Es werden ausdrücklich keine Mehrfachwährungen (z.B. Kupfer/Silber/Gold/Platin) und keine zusätzliche Währung neben Idia eingeführt.

**BERECHNET – NICHT ALS EIGENE PERSISTENTE WAHRHEIT:**

* **HP Max** – wird insbesondere aus Level, Attributen und Ausrüstung berechnet; beim Laden aus dem aktuellen persistenten Charakterzustand neu ermittelt
* **Mana Max** – wird insbesondere aus Level, Attributen und Ausrüstung berechnet; beim Laden aus dem aktuellen persistenten Charakterzustand neu ermittelt
* **zusammengefasster Armor-/Combat-Armor-Wert** – wird insbesondere aus der angelegten Ausrüstung und den relevanten Charakterwerten berechnet; beim Laden aus dem persistenten Equipment neu bestimmt

Die genauen Berechnungsformeln werden hier nicht festgelegt (Abschnitt 42). Falls das bestehende DB-Schema derzeit Felder für HP Max, Mana Max oder `combat_armor` enthält, bedeutet deren heutige Existenz nicht, dass sie Teil der neuen Stufe-B-Architektur bleiben müssen; das bestehende Feld `combat_armor` darf dokumentiert bleiben, wird aber NICHT allein aufgrund seiner Existenz als verbindliche Stufe-B-Persistenzkomponente behandelt. Es findet keine DB-Änderung statt.

**NORMATIVE DIRTY-KOMPONENTEN-ZUORDNUNG (verbindlich, entschieden):**

Die fünf Dirty-Komponenten des Abschnitts 6 sind festgelegt wie folgt. Die Zuordnung ist **semantisch** und vollständig; sie wird hier verbindlich festgeschrieben.

| Komponente | Zugeordnete Snapshot-Zustände |
|---|---|
| `Position` | `x`, `y` |
| `Progression` | `level`, `exp`, `free_attr_points`, `rested_pool`, `attributes` sowie `char_class`, `faction_transition`, `weapon_skill`, `learned_abilities` sowie `cooldowns` |
| `Idia` | `idia` |
| `Inventory` | persistenter Inventarbestand (Grundinventar, Rucksäcke/Bag-Slots, Equipment) **ohne** den Sicherheits-Puffer (Abschnitt 6) |
| `Resources` | `hp`, `mana` |

Festlegungen zur Semantik:

* **Eine Dirty-Komponente steuert die Aufnahme des Spielers in den Snapshot, nicht die Auswahl von Feldern.** Sobald mindestens eine Komponente dirty ist, wird der **vollständige** vorhandene `PersistSnapshot` erfasst. Es gibt **keine** feldweise Auswahl, kein feldweises Delta und keine teilweise Serialisierung einer Komponente.
* **Das initiale Laden ist keine Spielzustandsänderung.** Der Player wird beim Login aus der Datenbank gesetzt, ohne eine Komponente dirty zu markieren und ohne die Persistenz-Generation fortzuschreiben. Dirty-Markierungen entstehen erst durch nachfolgende Spielzustandsänderungen.
* **Abgeleitete Werte bleiben außerhalb** (Abschnitt 23: HP Max, Mana Max, Armor). Ebenso separat persistierte Systeme mit eigenem Persistenzweg (Abschnitt 8: Quest State/Progress) sowie `logout_at`.
* **Jede Zustandsänderung eines zugeordneten Feldes markiert über `Player::mark_dirty` die zugeordnete Komponente.** `mark_dirty` erhöht zusätzlich die Persistenz-Generation (Abschnitt 15/39); eine reine Bit-Manipulation ohne Generationserhöhung wäre eine Verletzung dieser Regel.
* **Keine neue Komponente.** Die fünf bestehenden Komponenten decken die persistenten Zustände vollständig ab. Die Item-Lifecycle-Metadaten begründen keine eigene Komponente: Sie reisen im vollständigen Snapshot mit, sobald irgendeine Komponente den Snapshot auslöst (bei Inventaränderungen markiert der aufrufende Spiellayer `Inventory`, `inventory_system.md` §17; ohne Dirty-State entsteht kein Snapshot und keine Finalisierung).

**Verbindliche Zuordnung ohne bestehenden Produktions-Mutationspfad:**

Für `char_class`, `faction_transition`, `weapon_skill` und `learned_abilities` ist die Komponente `Progression` normativ festgelegt, **obwohl im aktuellen Produktionscode kein Mutationspfad gefunden wurde, der diese Felder ändert**. Diese Felder sind persistiert (Snapshot und Datenbankspalten) und werden beim Laden gesetzt, können derzeit aber noch nicht im laufenden Spiel verändert werden.

Daraus folgt ausdrücklich:

* Die Zuordnung ist **festgelegt**, damit künftige Änderungsrouten sie ohne erneute Entscheidung einhalten können.
* Es wird hier **kein** neues Spielsystem und **keine** neue Änderungsroute implementiert. Das Fehlen eines Mutationspfades ist eine Eigenschaft des aktuellen Codeumfangs, kein offener Entscheidungspunkt.
* Künftige Mutationspfade für diese Felder **müssen** `Player::mark_dirty(PersistComponent::Progression)` verwenden und damit zugleich die Generation fortschreiben.
* Bis ein solcher Pfad existiert, wird für diese Felder **keine** Implementierung behauptet.

**Cooldowns im Player-Snapshot (verbindlich, `P-18`):**

* **Gegenstand:** ausschließlich die **laufenden Spieler-Ability-Cooldowns** (Ablaufzeitpunkt je Fähigkeit). Sie gehören zur Komponente `Progression` und damit in den normalen Player-Snapshot und in dieselbe DB-Transaktion wie der übrige Snapshot (Abschnitt 30).
* **Gespeicherte Form:** je Fähigkeit ein **absoluter Ablaufzeitpunkt in Millisekunden seit dem Unix-Epoch** (`SystemTime` → Epoch-Millisekunden). Die bestehende Server-Uhr (Wall-Clock) bleibt die Zeitbasis; es wird **keine** zweite Zeitquelle und **keine** monotone Zeitbasis eingeführt. Die Konvertierung in beide Richtungen ist überprüft und behandelt Werte vor dem Epoch sowie nicht darstellbare Zeitpunkte als „bereits abgelaufen".
* **Aufrundung auf ganze Millisekunden:** Beim Speichern wird ein vorhandener **Submillisekunden-Rest aufgerundet**, nicht abgeschnitten. Ein Abschneiden würde den Ablaufzeitpunkt um bis zu 999.999 µs **vorziehen** und einen laufenden Cooldown nach dem Laden zu früh freigeben. Ein **exakter** Millisekundenwert bleibt unverändert. Die Aufrundung kann den Ablauf damit **minimal verlängern** (um weniger als eine Millisekunde), aber **nie verkürzen**.
* **Erhalt über Logout, Disconnect und Reconnect:** Laufende Cooldowns werden durch Logout und Reconnect **nicht** zurückgesetzt. Der Ablaufzeitpunkt wird beim Login geladen; ein bereits abgelaufener Ablaufzeitpunkt gibt die Fähigkeit **sofort** frei.
* **Offline-Zeit:** zählt **normal** auf den Ablauf an. Es gibt **kein** Einfrieren von Cooldowns beim Logout; die verbleibende Dauer verringert sich während der Offline-Zeit wie während der Online-Zeit.
* **Uhrsemantik:** Die bestehende Wall-Clock-Semantik bleibt bewusst bestehen. Ein **Uhrsprung kann die verbleibende Dauer verändern** — eine Rückstellung der Serveruhr verlängert einen Cooldown, eine Vorstellung verkürzt ihn. Das ist eine bewusst übernommene Eigenschaft der bestehenden Zeitbasis und **kein** gesonderter Fehlerpfad.
* **Tod — allein `cooldown_persistent` entscheidet:** Beim Tod bleiben **ausschließlich** die Cooldowns **markierter** Fähigkeiten (`cooldown_persistent = 1`) bestehen; die Cooldowns **nicht** markierter Fähigkeiten werden zurückgesetzt. Das persistente Set wird aus der **tatsächlichen Ability-Registry** gebildet, nicht aus einem konstanten Leer-Set. Die vorhandene Kennzeichnung der Fähigkeiten wird **nicht** verändert: Es wird **nicht** behauptet, dass eine der bestehenden Seed-Fähigkeiten beim Tod persistent ist, solange ihr Flag `0` ist.
* **Dirty-Pfad:** Ein **tatsächlicher** Cooldown-Start (Neusetzen oder Überschreiben eines Ablaufzeitpunkts) und das **Entfernen** nichtpersistenter Cooldowns beim Tod markieren `Progression` über `Player::mark_dirty` und schreiben damit zugleich die Generation fort. Änderungen während eines laufenden Saves bleiben dadurch über die Generationsregel (Abschnitt 15) geschützt. Ein Cooldown-Start, der den gespeicherten Zustand **nicht** verändert, markiert **nicht**.
* **Erzwungener Speicherpunkt:** Der erzwungene Disconnect-Save (Abschnitt 11) erfasst die Cooldowns **vor** der Entfernung des Players; nach einem fehlgeschlagenen Save und erneuter Übernahme aus dem RAM (Abschnitt 16) gilt dieselbe Regel unverändert.
* **Vollständiges Ersetzen:** Die gespeicherte Cooldown-Map wird beim Anwenden **vollständig ersetzt**. Ein neuer Snapshot mit bewusst leerer Map entfernt zuvor gespeicherte Cooldowns; sie erscheinen beim nächsten Laden nicht wieder.
* **Altformat:** Alte Einzel- und Shared-Batch-Dateien **ohne** Cooldown-Feld bleiben lesbar und verarbeitbar. „Feld fehlt im Altformat" und „neuer Snapshot enthält bewusst eine leere Map" werden unterschieden: nur ein **vorhandenes** Feld ersetzt die gespeicherte Map; ein fehlendes Feld lässt den gespeicherten Zustand unberührt. Die Revisions-, Superseded- und Attributionsregeln (Abschnitt 29, `P-12`, `P-30`) bleiben unverändert wirksam; ein älterer Snapshot überschreibt keinen neueren bestätigten Zustand.
* **Ladefehler:** Ist der Cooldown-Zustand nicht zuverlässig lesbar, wird der Einstieg **fail-closed** abgelehnt. Ein stilles Behandeln als „keine Cooldowns" würde laufende Cooldowns zurücksetzen und die Zusage dieses Abschnitts aufheben.
* **Realm-Neustart:** Nach einem Realm-Neustart steht der zuletzt **dauerhaft gesicherte** Stand wieder zur Verfügung. Eine lückenlose Crash-Garantie seit der letzten Sicherung wird **nicht** behauptet: bis zum nächsten erfolgreichen Save gilt der letzte gesicherte Stand.
* **Ausdrücklich außerhalb (keine Änderung):** Auto-Angriff/Waffendauer, NPC-/Monster-Cooldowns, Effekt-Dauern von Buffs/Debuffs und aktive Casts bleiben unverändert RAM-/Session-Zustand. Es entsteht **keine** neue Persistenzkomponente und **keine** gesonderte Save-Infrastruktur.

**Gewichtung und Flush-Priorisierung (getrennte Frage):**

Ob Komponenten unterschiedlich gewichtet werden, ob beim Flush eine Reihenfolge oder Priorisierung gilt und ob eine spätere Komponente einen früheren verdrängen darf, ist **eine andere Frage als die Zuordnung** und wird hier **nicht** entschieden. Die Zuordnung dieses Abschnitts legt **keine** Gewichte, **keine** Flush-Regeln und **keine** Priorisierung fest. Siehe Abschnitt 42.

**EIGENER PERSISTENZPFAD – NICHT IM NORMALEN PLAYER-SNAPSHOT:**

* **Quest State / Quest Progress** – persistent, aber über einen eigenen direkten Quest-Persistenzweg behandelt; NICHT Teil des normalen Player-Snapshot-/Spool-Systems (Abschnitt 8)
* **logout_at** – eigener direkter Logout-Persistenzwert, kein normaler periodischer Dirty-/Snapshot-Wert (Abschnitt 11)

**NICHT DAUERHAFT PERSISTENT:**

* **Inventory Buffer** (runtime-only, Abschnitt 6; `inventory_system.md`)
* **Buyback History** (session-only)
* sonstiger ausdrücklich runtime-/session-only Zustand

Die Ausschlussgründe NICHT miteinander vermischen:

* **Quest State / Quest Progress:** persistent, aber über einen eigenen direkten Persistence-Pfad (Quest-Persistenzpfad, Abschnitt 8) behandelt.
* **logout_at:** eigener direkter Logout-Persistenzwert (Abschnitt 11).
* **Inventory Buffer / Buyback:** nicht Bestandteil der normalen dauerhaften Player-Persistence (runtime-/session-only).

Der normale Player-Spool darf einen neueren Quest-Zustand in MariaDB **niemals** durch einen älteren Quest-Zustand aus einem Player-Snapshot überschreiben. Da Quest State/Quest Progress nicht Bestandteil des normalen Player-Snapshots sind, enthält ein solcher Snapshot dieses System gar nicht; die Quest-Persistenz wird ausschließlich über den Quest-Persistenzpfad fortgeschrieben (Abschnitt 8). Die Revisions-/Vergleichsregeln (Abschnitt 29) beziehen sich damit nur auf die Komponenten des normalen Player-Snapshots.

Die Dirty-Bits bestimmen damit für den Spool primär:

> Kommt dieser Charakter in den nächsten Batch?

Sie bestimmen NICHT mehr:

> Welche einzelnen persistenten Komponenten dieses Charakters im Spool-Snapshot stehen?

Grund:

Jede spätere gültige Revision soll einen vollständigen, wiederherstellbaren persistenten Charakterzustand darstellen.

Beispiel:

* DB-Revision 1
* Revision 2 → beschädigt → Quarantäne
* Revision 3 → vollständig und gültig

Revision 3 darf den aktuellen vollständigen persistenten Zustand herstellen, ohne zwingend auf Revision 2 angewiesen zu sein.

Die bestehenden Stufe-A-Regeln zu komponentenbezogenem Dirty-State und zu Generationen (Abschnitte 6 und 15) bleiben für die Dirty-/Race-Erkennung gültig; die Vollständigkeitsregel gilt für den neuen Spool-/Recovery-Pfad (Stufe B).

Hinweis zur bestehenden Stufe-A-Implementierung:

Der aktuelle Stufe-A-PersistSnapshot enthält die hier für Stufe B zusätzlich aufgeführten Komponenten (Attribute, aktuelle HP, aktuelles Mana, Klasse, Fraktionszustand, Weapon Skills, Abilities) noch nicht vollständig; für einige davon existieren derzeit noch keine zentralen Writer. Das ist ein Implementierungsunterschied zwischen Stufe A und Stufe B: Die spätere Implementierung muss diese Dokumentation erfüllen, nicht umgekehrt. In diesem Dokumentationsauftrag wird kein Rust-Code geändert, keine Writer- oder Dirty-Markierungs-Umsetzung vorgenommen und keine Migration erstellt (Abschnitt 42).

---

## 24. Stufe B – Kritische / atomare Transaktionen

Die Spool ersetzt die bestehenden Sofort-/Atomar-Regeln NICHT.

Quest Acceptance:
→ unmittelbare Persistenz entsprechend der bestehenden Architektur (Abschnitt 10).

Quest Completion:
→ bestehende atomare MariaDB-Transaktion bleibt unverändert (Abschnitt 9; `Quest-System.md` §27.26).

Relevante Quest-Checkpoints werden direkt über den Quest-Persistenzpfad persistiert (Abschnitt 8); der Player-Spool ist für Quest State/Quest Progress nicht zuständig und ersetzt diesen Persistenzweg NICHT (Abschnitt 23).

Wenn die Quest Completion weitere persistente Änderungen erzeugt, die Bestandteil ihrer atomaren Transaktion sind (z.B. Questzustand und Questbelohnungen), bleiben diese Teil dieser Transaktion; der normale Player-Spool ersetzt diese Transaktion NICHT.

Die lokale periodische Spool darf nicht dazu führen, dass ein kritischer Vorgang als erfolgreich gilt, obwohl seine vorgeschriebene unmittelbare DB-Transaktion nicht erfolgreich abgeschlossen wurde.

Es werden keine neuen kritischen Transaktionstypen erfunden.

---

## 25. Stufe B – Sicheres Schreiben der Spool-Datei

Prinzip:

```text
Snapshot zunächst in temporäre Datei schreiben
-> vollständiges Schreiben sicherstellen
-> flush/fsync bzw. äquivalente dauerhafte Sicherung vorsehen
-> danach atomare Umbenennung in die endgültige Spool-Datei
```

Eine unvollständig geschriebene Datei darf nicht als gültiger Recovery-Snapshot behandelt werden.

Für V1 wird ein **menschenlesbares, versioniertes JSON-Format** festgelegt. Das konkrete Schema (Feldnamen, Struktur) wird hier nicht festgelegt (Abschnitt 42).

Architekturentscheidung:

> Emergency-/Spool-Persistenz speichert DATEN, nicht fehlgeschlagene SQL-Befehle.

Kein `backup.sql`-Konzept.

Grund:

Ein Fehler kann gerade im alten SQL-/Persistenzcode liegen. Nach einem Fix soll ein neuer Server die gespeicherten Daten mit dem aktuellen, reparierten Rust-Persistenzcode erneut nach MariaDB übertragen können.

Das Datenformat muss versionierbar sein, z.B. über eine `format_version`.

Unbekannte/nicht unterstützte Formatversionen dürfen nicht blind eingespielt werden.

Das Safe-Write-Prinzip dieses Abschnitts gilt für jede Spool-Datei (Batch) gleichermaßen: Eine temporär geschriebene Datei gilt erst nach der atomaren Umbenennung als gültig (Batch-Datei und Durability: Abschnitt 35).

---

## 26. Stufe B – DB-Fehler während des normalen Betriebs

Wenn die Übertragung eines Spool-Snapshots nach MariaDB fehlschlägt:

* Realm läuft weiter.
* Snapshot bleibt lokal erhalten.
* kein aggressiver unmittelbarer Retry-Loop.
* Fehler wird protokolliert.
* Persistence-Zustand des betroffenen Servers/Realms wird **DEGRADED**.
* spätere reguläre Persistenz-/Recovery-Versuche dürfen erneut versuchen, ausstehende Daten zu übertragen.
* fehlgeschlagene Snapshots dürfen nicht allein zur Speicherplatzbereinigung verworfen werden.

WICHTIG:

Die Warnung gilt auf **SERVER-/REALM-EBENE**, nicht pro Spieler.

200 betroffene Player-Snapshots aufgrund eines DB-Ausfalls erzeugen einen Persistence-Störfall des Servers, nicht 200 einzelne Administratorwarnungen.

Die bisherige Save-Fehler-Semantik (Abschnitt 16) bleibt gültig und wird hier um die Spool-Persistenz ergänzt.

Verhältnis zur Startup-Recovery: Der hier geregelte DEGRADED-Fall betrifft die **Übertragung im laufenden Betrieb**. Der Fehlerfall der **Startup-Recovery** ist davon getrennt in Abschnitt 28 geregelt: Dort geht der Zustand von RECOVERING auf DEGRADED über, die unerledigte Spool-Arbeit bleibt für eine Wiederholung erhalten und wird vom bestehenden periodischen Drainer fortgesetzt, **soweit die jeweilige Fehlerursache behoben ist**; READY folgt dort ausschließlich nach bestätigtem Abschluss der Start-Recovery (`P-22`). Die Betriebsregeln dieses Abschnitts werden dabei **nicht** pauschal auf den Start übertragen und der Start ergänzt **keine** weitergehende Zusicherung: Ein Fehler der Startup-Recovery beendet den Start **nicht**, trifft aber ebenso wenig eine Aussage über den Erfolg der nachfolgenden Initialisierungsschritte.

---

## 27. Stufe B – Monitoring-Vorbereitung

Keine Monitoring-Implementierung in der Stufe B. Dokumentiert werden hier die Anforderungen für eine spätere Monitoring-Stufe.

Der bestehende Webserver soll später mindestens darstellen können:

* Persistence-Status. `P-23` führt dafür das **additive** Feld `persistence_status` in der **bestehenden** `/status`-Antwort ein. Die drei festgelegten Werte sind **`recovering`**, **`ready`** und **`degraded`** (Abschnitt 28). Sie entsprechen unverändert den Zuständen des Persistenz-Status `Recovering`/`Ready`/`Degraded`; der Status-Enum wird dadurch **nicht** umbenannt. `/health` bleibt **Liveness** (der Prozess antwortet) und ist **keine** Readiness- und **keine** Spielfreigabe.
* letzter erfolgreicher DB-Persistenzzeitpunkt
* Zeitpunkt/Beginn eines anhaltenden Fehlers
* Anzahl ausstehender Spool-Snapshots
* Alter des ältesten ausstehenden Snapshots
* Gesamtgröße der ausstehenden Spool-Daten
* Anzahl offener Quarantänefälle
* Anzahl archivierter Quarantänefälle der letzten 30 Tage
* Anzahl Superseded-Snapshots
* Anzahl Superseded-Fälle der letzten 24 Stunden, 7 Tage und 30 Tage
* Anzahl unterschiedlicher betroffener Charaktere
* Häufung pro Charakter, soweit sinnvoll
* Gesamtgröße der Superseded-Ablage
* Alter des ältesten Superseded-Eintrags
* DB-Latenz für Spool-/Persistenz-Übertragungen
* Metriken zur Batch-Erstellung und Spool-Abarbeitung (Batch-Größe, Dauer, Fehler)
* soweit sinnvoll Fehlergruppen/Kategorien

Ziel der Superseded-Metriken ist die Unterscheidung zwischen einzelnen seltenen Recovery-/Crash-Sonderfällen, systemweiter Häufung und auffälliger Häufung bei einem bestimmten Charakter. Beispiele:

* Viele Superseded-Fälle über viele Charaktere → möglicher allgemeiner Persistence-/Reihenfolgefehler.
* Viele Fälle fast ausschließlich bei einem Charakter → möglicher charakterbezogener Fehler oder Sonderfall.

Dies sind Monitoring-/Diagnosehinweise, keine automatische Fehlerdiagnose (Abschnitt 34).

Systemnahe Metriken (z.B. Speicherplatz der Spool, Schreib-/Leselatenzen) werden über den bestehenden **Andora-Daemon** auf Host-Ebene (ohne Root-Zugriff) gesammelt. Für die DB-Latenzmetriken ist ein eigener DB-Monitoring-Zugang vorzusehen.

Später soll eine Admin-App diese Server-/Realm-Zustände übernehmen und Push-Benachrichtigungen erzeugen können.

Push-Konzept:

* eine Meldung pro betroffenem Server/Realm-Störfall
* keine Meldung pro betroffenem Spieler
* Recovery/Entwarnung soll ebenfalls möglich sein

Die Admin-App selbst gehört nicht zur Stufe-B-Implementierung.

---

## 28. Stufe B – Serverstart und Recovery

Beim Realm-Start muss die Persistence-Spool geprüft werden, bevor normaler Spielbetrieb freigegeben wird.

Grundablauf:

```text
Realm startet
    -> MariaDB-Verbindung herstellen
    -> Persistence-Spool prüfen

Wenn keine ausstehenden Snapshots:
    -> normaler Start / READY

Wenn Snapshots vorhanden:
    -> Realm-Zustand RECOVERING
    -> normale Spieler-Logins zunächst blockiert
    -> Snapshots validieren
    -> Persistenzstand mit MariaDB vergleichen
    -> erforderliche Snapshots mit dem AKTUELLEN Rust-Persistenzcode
       nach MariaDB übertragen
    -> erfolgreiche DB-COMMITs bestätigen
    -> erfolgreich erledigte Spool-Snapshots entfernen
```

Die Snapshots werden in Reihenfolge verarbeitet, **ältester zuerst** (Batch-Sortierung: Abschnitt 36).

Erst wenn alle normal verarbeitbaren Spool-Snapshots erledigt oder ordnungsgemäß aus der aktiven Recovery in Quarantäne überführt wurden, darf der Realm **READY** werden.

Monitoring/Administration soll während **RECOVERING** weiterhin verfügbar sein.

`P-23` (umgesetzt): Der Health-Server startet nach Erstellung seiner Abhängigkeiten (Config, Shared, Persistenz-Runtime) und **vor Beginn der Recovery**, damit Monitoring und Administration während **RECOVERING** erreichbar sind. Während der Recovery ist der Zustand über das Feld `persistence_status` in `/status` mit dem Wert `recovering` beobachtbar; danach tritt an seine Stelle `ready` oder `degraded` (Abschnitt 27). Die Monitoring-Erreichbarkeit ist **keine** Spielfreigabe: der WebSocket-Spielserver startet weiterhin erst nach der Recovery, Logins bleiben in **RECOVERING** blockiert, und ein Monitoring-Request verändert keinen Zustand.

### Recovery-Fehlerfall: Start als DEGRADED (`P-28`)

Die Recovery beginnt im Zustand **RECOVERING**. Endet sie mit einem **propagierten** Fehler aus dem Datenbank- oder Dateisystempfad, geht der Realm in **DEGRADED** über und die Startsequenz fährt im selben Prozess fort:

* **Übergang RECOVERING → DEGRADED:** Der propagierte Fehler beendet den Recovery-Durchlauf, ohne den Start abzubrechen. Der Realm startet dann als **DEGRADED**; die Nicht-Abbrech-Eigenschaft gilt **nur** für die Recovery. Jede **weitere** Initialisierung (etwa das Laden der Inhaltsdefinitionen nach der Recovery) behält ihre eigene Fehlerbehandlung und kann den Start weiterhin abbrechen. Ein DEGRADED-Start belegt daher **nicht**, dass alle nachfolgenden Initialisierungsschritte erfolgreich waren.
* **Erhalten der unerledigten Spool-Arbeit:** Die zum Zeitpunkt des Fehlers nicht verarbeiteten Spool-Batches verbleiben im Spool und stehen für eine Wiederholung zur Verfügung. Bereits bestätigte Einträge bleiben über die `persist_revision` und den Reverify-Nachweis gegen eine doppelte Übernahme geschützt.
* **Fortsetzung durch den periodischen Drainer:** Der bestehende periodische Drainer arbeitet die verbliebene Spool-Arbeit weiter ab, **soweit die jeweilige Fehlerursache behoben ist**. Die Wiederverfügbarkeit der Datenbank allein genügt dafür nicht: Ein Dateisystemfehler des Ablagebereichs — insbesondere ein nicht lesbares Spool-Verzeichnis (`count_batches`, `src/realm-rs/src/spool.rs:714-716`) oder das fehlgeschlagene Verschieben einer Datei in die Quarantäne (`src/realm-rs/src/spool.rs:1369`) — wird dadurch **nicht** behoben, und der Realm bleibt dann DEGRADED. Nicht anlegbare Verzeichnisse betreffen dagegen bereits die Spool-Initialisierung **vor** der Recovery und brechen den Start ab; sie sind kein Recovery-Fehlerfall.
* **Abschluss gemäß P-22:** Der Übergang zu **READY** erfolgt ausschließlich nach **bestätigtem Abschluss** der Start-Recovery, also erst wenn keine relevante Restarbeit in `<base>/spool/` mehr offen ist. Ein einzelner erfolgreicher Drain-Schritt hebt DEGRADED nicht vorzeitig auf; solange die Start-Recovery offen ist, bleibt der Zustand DEGRADED, bis der Abschluss bestätigt ist.
* **DEGRADED blockiert HELLO nicht pauschal:** Die Spielfreigabe ist an RECOVERING gebunden. Im Status DEGRADED ist der HELLO-Einstieg grundsätzlich möglich; die **charakterbezogenen fail-closed-Prüfungen bleiben unverändert wirksam**: das fail-closed Auflösen des Charakterladens (`resolve_character_lookup`, `src/realm-rs/src/handlers.rs:107-121`), die Quarantäne-Verfügbarkeitsprüfung (`src/realm-rs/src/handlers.rs:287-298`) und die Pending-Revision-Prüfung (`src/realm-rs/src/handlers.rs:300-313`, `db_row_is_stale` in `src/realm-rs/src/world.rs:418-424`, bei `Err` gilt die Zeile als veraltet). Ein nicht verifizierter Zustand wird nicht als aktiver Player registriert.
* **Beobachtbarkeit:** Der Zustand bleibt über `persistence_status` in `/status` mit dem Wert `degraded` beobachtbar (Abschnitt 27, `P-23`); Monitoring und Administration bleiben erreichbar.

**Abgrenzung zur regulären Quarantäne:** Nicht jede fehlerhafte oder beschädigte Datei erzeugt einen propagierten Fehler. Einträge, die der reguläre Drain nicht anwenden kann (unlesbar, fehlerhaft, unbekanntes Format, unbekannter Charakter), werden in die Quarantäne überführt und gelten dort als abgeschlossener Recovery-Anteil (Abschnitt 33, `P-14`, `P-30`); sie gelten **nicht** als offene Restarbeit der Start-Recovery (`P-22`). Erst der propagierte Fehler des Durchlaufs führt zum Übergang in DEGRADED. Für den propagierten Fehler wird **keine** allgemeine Datenverlustfreiheit behauptet: es gilt das oben genannte Erhalten der unerledigten Spool-Arbeit sowie der Schutz bereits bestätigter Einträge.

**Keine übernommene Sammelgarantie:** Die Einträge eines Batches werden nach `P-12` einzeln verarbeitet und einzeln bestätigt. Eine pauschale Rollback-Garantie für einen gesamten Batch wird hier **nicht** übernommen.

**Statusbeobachtung:** `P-28` ist **kein** Nachweis dafür, dass alle Server-Tasks vollständig überwacht sind. Insbesondere endet ein Fehler innerhalb der per Task gestarteten Spielserver-Instanz nur diese Task und wird nicht zu einem Prozessfehler; die betroffene Aufgabe ist dann nicht verfügbar, während der Prozess weiterläuft. Diese Abgrenzung ist in `docs/Security.md` unter `P-28` festgehalten.

---

## 29. Stufe B – Neuerer Zustand gewinnt / persist_revision

Jede DB-Persistenz besitzt einen serverseitig erzeugten, monoton aufsteigenden Persistenzbezug, die `persist_revision`. Beim Recovery wird anhand dieser Revision erkannt, ob der Spool-Zustand oder der vorhandene DB-Zustand neuer ist.

Die `persist_revision` gilt **pro Charakter**: Sie ist KEINE globale Realm-Revision und KEINE Batch-Revision. Jeder Charakter besitzt seine eigene monotone `persist_revision`.

Beispiel (Batch 9302):

* Charakter A: `persist_revision` 3
* Charakter B: `persist_revision` 157
* Charakter C: `persist_revision` 42

Die Batch-Identität (`batch_id`) und die Charakter-Persistenzrevision sind voneinander unabhängig. Eine spätere `batch_id` darf deshalb NIEMALS zur Bestimmung des persistenten Charakterzustands verwendet werden (Abschnitt 35).

Konzeptionell beginnt jeder Charakter mit `persist_revision = 0`.

`0` bedeutet: Für diesen Charakter wurde noch KEIN revisionierter Player-Snapshot erfolgreich in MariaDB committed.

Der erste erfolgreich erzeugte revisionierte Snapshot erhält Revision 1, danach 2, 3, 4 usw. Dies gilt auch für neu erstellte Charaktere. Ob eine Revision tatsächlich als übernommen gilt, entscheidet erst die dauerhafte Spool-Sicherung (Abschnitt 39).

Der Client darf diesen Wert NICHT bestimmen.

Die `persist_revision` ist die **alleinige autoritative Grundlage** für die Recovery-Entscheidung. Der Zeitstempel `captured_at` wird nur zu Diagnose-/Nachvollziehbarkeitszwecken geführt, NICHT als Vergleichsgrundlage.

Der Dateiname eines Snapshots darf einen lesbaren Zeitstempel enthalten, ist aber NICHT autoritative Grundlage für die Recovery-Entscheidung.

KEINE `previous_revision`-Kette: Für die automatische Vergleichsentscheidung benötigt ein Player-Snapshot insbesondere seine **eigene** `persist_revision`. Eine `previous_revision` wird NICHT als Voraussetzung eingeführt (z.B. dem Muster „DB-Revision muss exakt der previous_revision entsprechen“). Eine frühere Revision kann z.B. bereits in Quarantäne liegen; eine spätere gültige Revision muss trotzdem verarbeitet werden können.

Fachliche Regel (Vergleich gegen den in MariaDB gespeicherten Stand des Charakters):

* DB-Revision **kleiner** als Snapshot-Revision → Snapshot ist neuer → der Snapshot darf/muss mit dem aktuellen Persistence-Writer verarbeitet werden; nach erfolgreicher vollständiger DB-Transaktion wird die DB-Revision auf die Snapshot-Revision gesetzt.
* DB-Revision **gleich** Snapshot-Revision → der Zustand wurde bereits committed bzw. ist bereits vorhanden → Snapshot nicht erneut anwenden; der Eintrag darf für diesen Batch als erledigt gelten.
* DB-Revision **größer** als Snapshot-Revision → MariaDB besitzt bereits einen neueren Zustand → Snapshot NIEMALS über den neueren DB-Zustand schreiben; der Fall wird als **superseded** behandelt und der Eintrag in `superseded/` übernommen (Abschnitt 34).
* fehlender/unlesbarer Revisionszustand → nicht blind überschreiben; der Eintrag bleibt lokal erhalten und wird nicht automatisch als erledigt behandelt.

`persist_revision` ist für diese automatische Entscheidung maßgeblich. Konkrete Spaltenbezeichnungen, die DB-Migration und die genaue technische Erzeugung der Revision (z.B. pro Charakter vergebene Sequenz) werden hier nicht festgelegt (Abschnitt 42).

---

## 30. Stufe B – Graceful Shutdown und Spool

Die Spool dient gleichzeitig als Sicherheitsmechanismus beim kontrollierten Shutdown.

Wenn ein finaler Player-Zustand beim Shutdown nicht erfolgreich nach MariaDB übertragen werden kann, muss der noch nicht dauerhaft in MariaDB gesicherte Zustand lokal in der Persistence-Spool erhalten bleiben.

Dadurch ist KEIN separates `backup.sql`-System erforderlich (`Datenbank_Architektur.md` Abschnitt 25 bleibt als DB-Backup-Ebene unverändert gültig).

Nach einem späteren Bugfix kann der neue Realm-Prozess die gespeicherten Daten mit dem aktuellen Persistenzcode wiederherstellen.

Die detaillierte Shutdown-Implementierung bleibt einer späteren Stufe vorbehalten (Stufe D). Hier wird nur die Architektur dokumentiert.

**`logout_at` BEIM GRACEFUL SHUTDOWN**

Für das direkte Schreiben des Logout-Zeitpunkts beim kontrollierten Realm-Shutdown gilt dieselbe Regel wie beim normalen Disconnect (Abschnitt 11): ein **begrenzter Retry**, danach `Degraded`, strukturiertes Logging ohne Session-ID, Tokens oder Roh-IP, kontrolliertes Weiterarbeiten. Zusätzlich gilt beim Shutdown:

* **Bounded Waiting.** Der Shutdown darf **nicht** unbegrenzt auf eine nicht erreichbare Datenbank warten. Nach Ausschöpfen des begrenzten Budgets wird der Prozess **kontrolliert beendet**; ein Hängenbleiben des Shutdowns ist ausgeschlossen.
* **Pro Charakter erfassen, am Ende zusammenfassen.** Fehler werden **je Charakter** erfasst und **am Ende zusammengefasst** ausgewiesen, damit aus vielen Einzelfehlen ein Befund wird.
* **Der Abschluss muss erkennbar sein.** Der Abschlussbericht beziehungsweise das Log muss **erkennen lassen, dass der Shutdown nicht vollständig persistiert werden konnte**. Ein Shutdown mit Fehlern darf nicht als vollständig persistiert gemeldet werden.
* **Viele Spieler zugleich.** Die Zahl der gleichzeitig betroffenen Spieler kann **größer** sein als beim einzelnen Disconnect, weil alle laufenden Spieler nacheinander bearbeitet werden. Daraus wird **keine** Behauptung abgeleitet, ein Fehler sei beim Shutdown **wahrscheinlicher**.

Die Final-Save-Reihenfolge aus Abschnitt 12 bleibt unverändert gültig.

Gelingt beim Shutdown **weder** die DB-Übertragung **noch** die lokale Spool-Sicherung, wird der Vorgang NICHT als erfolgreich persistiert gemeldet: Der Zustand ist dann nicht dauerhaft gesichert und als schwerwiegender Persistence-Fehler zu behandeln (Abschnitt 41).

---

## 31. Stufe B – Irreparabel beschädigte Snapshots / Quarantäne

Ein einzelner irreparabel beschädigter oder nicht mehr automatisch verarbeitbarer Snapshot darf den gesamten Realm NICHT dauerhaft am Start hindern.

Verzeichnissemantik:

```text
persistence/
├── spool/
│   └── ausstehende, noch nach MariaDB zu übertragende Batch-Dateien (ein Batch pro Persistenzlauf)
│
├── superseded/
│   └── gültige Player-Snapshots, die nicht angewendet wurden, weil MariaDB
│       bereits eine höhere persist_revision besitzt (Abschnitt 34)
│
└── quarantine/
    ├── open/
    │   └── offene, noch nicht untersuchte fehlerhafte Einzeleinträge aus Batches
    │
    └── archive/
        └── bereits untersuchte/bearbeitete Quarantänefälle
```

Die Verzeichnisangaben sind relativ gemeint; absolute Pfade und konkrete Dateinamen werden nicht festgelegt (Abschnitt 42).

Kann ein Snapshot-/Einzeleintrag nicht sicher wiederhergestellt werden:

* betroffenen Eintrag aus aktiver Spool in `quarantine/open/` überführen
* Originaldaten für Analyse erhalten
* Fehler und relevante Metadaten protokollieren
* Recovery mit anderen Snapshots fortsetzen

Ein fehlerhafter Einzeleintrag wird **einzeln** aus seinem Batch isoliert; die übrigen Einträge desselben Batch bleiben weiter verarbeitbar, und die Batch-Datei wird erst entfernt, wenn alle Einträge erledigt sind (Einzelsemantik: Abschnitt 36).

Wenn alle übrigen verarbeitbaren Snapshots erledigt sind:

* Realm darf **READY** werden.
* betroffener Spieler/Charakter wird NICHT automatisch gesperrt.
* beim Login erhält er seinen letzten gültigen MariaDB-Stand.

Der mögliche Verlust des Fortschritts seit dem letzten gültigen Persistenzstand wird akzeptiert.

Besonders seltene/wertvolle verlorene Items können später nach manueller Prüfung gegebenenfalls durch Support kompensiert werden, z.B. über ein zukünftiges Briefkasten-/Postsystem.

KEINE automatische Kompensationslogik wird festgelegt.

---

## 32. Stufe B – Quarantäne als Fehleranalyse

Quarantäne-Snapshots dienen drei Zwecken:

1. Recovery-/Datenanalyse
2. Support bei relevanten verlorenen Zuständen/Items
3. Analyse systematischer Fehler im Persistenzcode

Sinnvolle technische Metadaten sollen vorgesehen werden, z.B.:

* Realm-/Server-Zuordnung
* Zeitpunkt
* Snapshot-/Format-Version
* Server-Build/Version, soweit verfügbar
* betroffene Persistenzkomponenten
* konkrete Validierungs-/Recovery-Fehlerkategorie

Es werden keine unnötigen vollständigen Debug-Dumps festgeschrieben.

Mehrere ähnliche Quarantänefälle sollen später über Monitoring als möglicher systematischer Fehler erkennbar sein.

Superseded-Snapshots sind eine davon **getrennte** Fehler-/Analyse-Kategorie (Abschnitt 34): Sie sind gültige, aber nicht angewendete Player-Snapshots – keine beschädigten Snapshots.

---

## 33. Stufe B – Quarantäne-Aufbewahrung

VERBINDLICHE REGEL:

Dateien in:

```text
persistence/quarantine/open/
```

sind OFFENE Fälle.

Sie werden NIEMALS aufgrund ihres Alters automatisch gelöscht.

Erst nachdem ein Fall untersucht/bearbeitet wurde, wird er nach:

```text
persistence/quarantine/archive/
```

verschoben.

ERST BEIM ARCHIVIEREN beginnt die 30-Tage-Aufbewahrungsfrist.

Archivierte Quarantänefälle dürfen 30 Tage nach ihrer Archivierung automatisch gelöscht werden.

Die Frist beginnt NICHT beim ursprünglichen Snapshot-Zeitpunkt.

Dadurch bleiben ungefähr 30 Tage bereits bearbeiteter Fehlerfälle für Statistik, Vergleich und Regressionsanalyse verfügbar.

### Verbindliche Zielregel: charakterbezogene Quarantäne-Sperre

VERBINDLICHE ZIELREGEL – **serverseitig umgesetzt und getestet** in `9196224a9978b4ec09100f140e8ac40bbc27c9c8`; die **Clientdarstellung bleibt ausstehende Clientintegration**. Die folgenden Aussagen beschreiben die beschlossene Zielsemantik und deren Umsetzungsstand. Der belegende Nachweis steht in `docs/Security.md` unter `P-30` und im Abschnitt „Nachweis" dieses Dokuments.

**Dirty-Zustand und Folgesnapshot** (heute bereits verifiziert, siehe `P-30` und `P-36`):

* Schlägt ein Save fehl, werden die ungesicherten persistenten Änderungen **nicht verworfen**.
* Die betroffenen Dirty-Komponenten bleiben im RAM dirty.
* Der nächste Save berücksichtigt diesen bestehenden Dirty-Zustand **auch dann**, wenn am Charakter seitdem keine weitere persistente Änderung erfolgt ist.
* Der Snapshot ist nach dem aktuell verifizierten Modell ein **vollständiger Charakterzustand und kein Delta**.
* Weitere Änderungen dürfen bis zum nächsten Save hinzukommen und werden gemeinsam im neueren vollständigen Snapshot abgebildet.
* Ein lediglich im RAM oder im normalen Spool vorhandener neuerer Snapshot gilt **noch nicht** als erfolgreiche Ablösung.
* Erst eine nachweislich erfolgreiche und dauerhafte Datenbankübernahme löst den technischen Quarantänefall fachlich ab.

**Charakterbezogene Sperre:**

* Ein ungelöster, **sicher zuordenbarer** Quarantänefall sperrt **ausschließlich den betroffenen Charakter**.
* Die **Kontoanmeldung bleibt möglich**.
* Die **Charakterauswahl bleibt möglich**.
* **Andere Charaktere desselben Kontos bleiben spielbar.**
* Andere Konten und der Realm bleiben unberührt.
* Der Server prüft den Status beim **tatsächlichen Charakterbeitritt erneut autoritativ**.
* Eine Clientanzeige allein ist **keine** Sicherheitsgrenze.

**Clientdarstellung (Zielregel, noch nicht implementiert – ausstehende Clientintegration):** Für einen technisch gesperrten Charakter ist folgende Zielanzeige festgelegt:

* Charakter in der Auswahl **ausgegraut**,
* Spielen-/Betreten-Schaltfläche für diesen Charakter **deaktiviert**,
* Status: `Spielstand wird geprüft`,
* Erklärung: `Dieser Charakter ist vorübergehend nicht verfügbar. Deine gespeicherten Daten bleiben erhalten. Bitte versuche es später erneut.`

Dabei dürfen **keine** internen Pfade, Dateinamen, Revisionen, Datenbankfehler, Rohfehler oder internen IDs an den Client ausgegeben werden. Die Charakterauswahl ist im aktuellen Repository **noch nicht implementiert**; dieser Clientteil ist ausdrücklich **noch ausstehende Clientintegration** und keine vorhandene Funktion.

**Nachweis (serverseitiger Teil, Commit `9196224a9978b4ec09100f140e8ac40bbc27c9c8`):**

* `550 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`.
* Baseline vor `P-30`: `502`; **netto +48** Top-Level-Tests, davon `42` in `src/realm-rs/src/spool.rs` und `6` in `src/realm-rs/src/handlers.rs`; **0** bestehende Tests entfernt.
* Clippy: **keine neue** Warnung.
* Die Attributions-, Gate- und Doppel-Drain-Tests rufen die echte Produktionsfunktion `drain_one` auf; ihre Detektionskraft ist per Mutationstest belegt.
* Umgesetzt sind damit **alle** serverseitigen Punkte dieser Zielregel: charakterbezogene Sperre, autoritative Prüfung beim Beitritt, andere Charaktere bleiben spielbar, automatische Freigabe über `database_revision >= quarantine_revision`, Archivierung abgelöster Fälle mit minimaler Kennzeichnung, Betreiberzähler und datenfreie Logausgabe.
* **Nicht** Bestandteil dieses Nachweises und weiterhin **offen**: die ausgegraute Charakterauswahl, die deaktivierte Betreten-Schaltfläche sowie Status- und Erklärungstext.

**Sichere automatische Freigabe:** Die Zielregel lautet:

```text
database_revision >= quarantine_revision
```

Diese Beziehung darf nur dann als Ablösungsnachweis gelten, solange die bereits verifizierten Voraussetzungen bestehen: Snapshots enthalten einen **vollständigen** Charakterzustand, Revisionen sind **pro Charakter monoton**, und Spielerzustand und Revision werden **atomar** in die Datenbank übernommen. Ändert sich das Persistenzformat später zu Deltas oder Komponenten-Snapshots, muss die Ablösungsregel **neu geprüft** werden.

**Analyselebenszyklus (Zielregel):**

* Ungelöste Fälle liegen unter `quarantine/open/`.
* Ein sicher durch einen DB-bestätigten neueren Stand abgelöster oder manuell technisch geklärter Fall wechselt nach `quarantine/archive/`.
* Der ursprüngliche Quarantäneinhalt bleibt als **Analysebeleg** unverändert erhalten.
* Eine minimale Kennzeichnung hält fest, dass der Fall abgelöst wurde und **durch welche bestätigte Revision**.
* Für `quarantine/archive/` gilt weiterhin die **bestehende 30-Tage-Retention** dieses Abschnitts.
* Nicht sicher zuordenbare Fälle bleiben Analyse- und Betreiberfälle, dürfen aber **keine** globale Konto-, Spieler- oder Realm-Sperre auslösen.

**Technischer Fehler ist keine Sanktion:**

* Quarantäne ist zunächst ein **technischer Persistenz- und Analysefall**.
* Quarantäne ist **kein Betrugsnachweis**.
* Ein technischer Save-Fehler darf **nicht automatisch** zu einer dauerhaften Charakter- oder Kontosperre führen.
* Ein Betrugsverdacht wird **getrennt** untersucht.
* Erst **bestätigter** Betrug kann durch eine bewusste, autorisierte administrative Entscheidung zu einer dauerhaften Charaktersperre führen.
* Eine **Kontosperre** ist wiederum eine **gesonderte** administrative Entscheidung.
* Der technische Status `save_recovery_pending` und eine administrative Sperre dürfen **nicht vermischt** werden.

Fachliche Zustände:

```text
available
save_recovery_pending
administratively_locked
```

`P-30` behandelt ausschließlich `save_recovery_pending`. Ein vollständiges **administratives Bannsystem ist nicht automatisch Bestandteil von `P-30`**.

**Shutdown und Neustart (umgesetzt):** Bei kontrollierter Abschaltung wird der aktuelle persistente RAM-Zustand der Onlinecharaktere zunächst durable in den Spool geschrieben (dieser Teil ist heute verifiziert). Nach einem Neustart verarbeitet die Recovery den gespeicherten normalen Spool vor der READY-Freigabe (ebenso heute verifiziert). Quarantänefälle werden **getrennt** bewertet: `recover` liest ausschließlich den normalen Spool (`src/realm-rs/src/spool.rs:234-244`), und die Charakterbewertung erfolgt erst beim Charakterbeitritt. Ein ungelöster Quarantänefall blockiert damit **nur den zugehörigen Charakter** und **nicht** die allgemeine READY-Freigabe des Realms.

---

## 34. Stufe B – Superseded-Snapshots

Ein **Superseded-Snapshot** entsteht, wenn MariaDB bereits eine höhere `persist_revision` besitzt als der gültige Player-Snapshot im Batch (DB-Revision > Snapshot-Revision, Abschnitt 29).

Der ältere Snapshot wird dann NICHT einfach gelöscht und NICHT als normaler Quarantänefehler behandelt, sondern einer eigenen logischen Ablage zugeführt (Verzeichnisstruktur: Abschnitt 31):

```text
persistence/
├── spool/
├── superseded/
└── quarantine/
    ├── open/
    └── archive/
```

Bedeutung:

* `spool/` = noch zu verarbeitende Batch-Snapshots
* `superseded/` = gültige Player-Snapshots, die nicht angewendet wurden, weil MariaDB bereits eine höhere `persist_revision` besitzt
* `quarantine/` = tatsächlich fehlerhafte, beschädigte oder nicht zuverlässig verarbeitbare Player-Snapshots

Superseded und Quarantäne sind ausdrücklich **unterschiedliche Fehler-/Analyse-Kategorien** (Abschnitt 32).

**Extraktion pro Player:**

Ein superseded Player-Eintrag wird analog zur Fehlerisolierung als eigener analysierbarer Player-Snapshot dauerhaft in `superseded/` übernommen. Die übrigen Player des Batches werden normal weiterverarbeitet.

Erst nachdem der superseded Player-Snapshot sicher in `superseded/` übernommen wurde, darf dieser Eintrag für den aktiven Batch als erledigt gelten (Abschnitt 36). Danach kann die normale Batch-Abarbeitung fortgesetzt werden.

Der ursprüngliche Batch wird weiterhin erst gelöscht, wenn **alle** Player-Einträge nach den dokumentierten Regeln erledigt sind (Abschnitt 36).

**Zweck:**

Superseded-Snapshots werden aufbewahrt für:

* Persistence-Fehleranalyse
* Erkennen ungewöhnlicher Revisionsreihenfolgen
* Support
* Vergleich alter/neuer Charakterzustände
* Untersuchung möglicherweise verlorener wertvoller Items
* Erkennen systematischer Fehler
* Erkennen charakterbezogener Fehlerhäufungen

Beispiel:

Revision 2 enthält ein seltenes Item. MariaDB besitzt bereits Revision 3; Revision 2 wird deshalb nicht angewendet. Der Superseded-Snapshot kann später als Hinweis dienen, ob ein gemeldetes fehlendes Item in einem älteren Zustand vorhanden war.

WICHTIG:

Das Vorhandensein eines Items in einem älteren Snapshot beweist NICHT automatisch einen Persistence-Fehler – das Item könnte zwischen den Revisionen regulär verkauft, gehandelt, verbraucht, zerstört oder anderweitig entfernt worden sein. Superseded-Daten sind daher Diagnose-/Supportinformation und KEINE Grundlage für automatische Item-Wiederherstellung oder automatische Kompensation.

**Aufbewahrung:**

Superseded-Snapshots werden 30 Tage ab dem Zeitpunkt ihrer Ablage in `superseded/` aufbewahrt und dürfen danach automatisch gelöscht werden. Sie benötigen – anders als `quarantine/open/` – keine manuelle Bearbeitung/Freigabe vor Ablauf dieser Frist.

Die bestehende Quarantäne-Aufbewahrungsregel bleibt unverändert (Abschnitt 33): Offene Quarantänefälle werden NICHT automatisch altersbasiert gelöscht; erst nach manueller Bearbeitung/Archivierung beginnt dort die bereits dokumentierte 30-Tage-Archivfrist.

Superseded-Retention und Quarantäne-Retention nicht miteinander vermischen.

---

## 35. Stufe B – Batch-Format / eine Datei pro Persistenzlauf

Ein Persistenzlauf erzeugt **eine** Spool-Datei (Batch), die die PersistSnapshots aller im Lauf erfassten dirty Spieler enthält. Es gibt bewusst **keine** getrennten Dateien pro Spieler.

Für V1 gilt:

* Format: **menschenlesbares, versioniertes JSON** (Abschnitt 25).
* Jeder Batch besitzt eine eigene `format_version`; das konkrete Schema (Feldnamen, Struktur) bleibt offen (Abschnitt 42).
* Ein Batch ist unabhängig von anderen Batches; innerhalb eines Batch sind die Player-Einträge unabhängig voneinander (Abschnitt 36).
* Jeder Player-Eintrag trägt die eigene `persist_revision` seines Charakters; `batch_id` und `persist_revision` sind unabhängig voneinander (Abschnitt 29).
* Jeder Player-Eintrag ist ein **vollständiger persistenter Player-Snapshot** (Abschnitt 23).

**Keine Realm-ID im normalen Spool-Batch:**

Jeder Realm besitzt seine eigene MariaDB und seinen eigenen Persistence-Spool. Deshalb muss ein normaler Stufe-B-Spool-Batch KEINE redundante Realm-ID als Bestandteil seines normalen Persistence-Datenmodells tragen. Die Zuordnung des normalen Spools zum Realm ist bereits durch die Realm-/Server-Umgebung eindeutig. Insbesondere darf eine Realm-ID NICHT als notwendiges Feld jedes Player-Snapshots vorgeschrieben werden.

Das bedeutet NICHT, dass Realm-/Serverinformationen für die Fehleranalyse grundsätzlich verboten sind. Für Diagnose-/Supportmetadaten sowie für Quarantäne- oder Superseded-Metadaten dürfen Realm-/Serverinformationen weiterhin gespeichert werden, wenn sie zur Zuordnung und Analyse eines Problems sinnvoll sind (Abschnitt 32). Zu unterscheiden sind:

* Normaler Persistence-Snapshot: → keine redundante Realm-ID erforderlich.
* Diagnose-/Supportmetadaten: → Realm-/Serverkennung darf enthalten sein.

Die bestehenden Quarantäne-/Support-Regeln (Abschnitte 31/32/33) bleiben unverändert gültig.

**JSON-Schema als spätere technische Aufgabe:**

Die fachlichen Inhalte des normalen Player-Snapshots sind verbindlich dokumentiert (Abschnitt 23). Das konkrete JSON-Datenmodell muss hier NICHT bis auf jedes Feld festgelegt werden; die spätere Implementierung darf die konkrete technische Struktur anhand der bestehenden Rust-Datenstrukturen sinnvoll gestalten. Verbindlich bleiben dabei insbesondere:

* versioniertes Format (Abschnitt 25)
* gemeinsamer Batch mit mehreren Player-Einträgen (dieser Abschnitt)
* Player-Einträge unabhängig verarbeitbar (Abschnitt 36)
* `persist_revision` pro Charakter (Abschnitt 29)
* `captured_at` für Diagnose (Abschnitt 29)
* vollständiger Zustand der dem normalen Player-Persistence-System zugeordneten Komponenten (Abschnitt 23)
* Idia als absoluter Gesamtbestand (Abschnitt 23)
* keine Quest State/Quest Progress-Daten im normalen Player-Snapshot (Abschnitt 8)
* kein Inventory Buffer, keine Buyback History (Abschnitt 23)
* keine notwendige Realm-ID im normalen Batch (dieser Abschnitt)

Keine konkrete JSON-Feldverschachtelung wird hier festgelegt, wenn sie bisher nicht entschieden wurde (Abschnitt 42).

**Freigabe des reservierten Snapshot-Speichers (verbindlich, P-12 entschieden):**

Ein Persistenzlauf hält die Snapshots aller im Lauf erfassten dirty Spieler zunächst **reserviert**, weil die vollständige Batch-Datei vor der Veröffentlichung geschrieben sein muss. Die Grenze, an der dieser reservierte Speicher wieder freigegeben wird, ist die **dauerhafte Spool-Übergabe**, nicht die spätere Datenbankübernahme:

1. Der Lauf erfasst die Dirty-Menge konsistent unter der World-Sperre und baut daraus die Snapshots (unverändert §23, §29, §15).
2. Der Lauf schreibt **eine** gemeinsame Batch-Datei, die ausschließlich die dirty Spieler-Snapshots dieses Laufs enthält. Ist die Dirty-Menge leer, entsteht **keine** Datei und es wird nichts reserviert.
3. Die Datei wird als **fertiger Batch** dauerhaft veröffentlicht (vollständig geschrieben, Sync-Schritte, atomare Veröffentlichung im Spool-Verzeichnis). Erst danach wird der reservierte Snapshot-Speicher freigegeben.
4. Die **Datenbankverarbeitung** erfolgt anschließend unabhängig aus dem Spool. Ein ausstehender DB-Apply verlängert den reservierten Speicher nicht.
5. Am **lebenden Spielerzustand** bleiben der aktuelle RAM-Zustand sowie die schlanken Recovery- und Revisionsmetadaten erhalten. Die Dirty-Rücknahme bleibt an die tatsächlich gesicherte Revision und an die während des Schreibens unveränderte Generation gebunden; neuere Änderungen bleiben dirty (§15, §39).
6. Schlägt die Veröffentlichung fehl, bleiben die Snapshots reserviert beziehungsweise die Dirty-Bits erhalten, sodass ein späterer Lauf denselben Zustand erneut sicher schreibt. Es geht kein ungesicherter Zustand verloren.

**Verarbeitung und Quarantäne je Eintrag (verbindlich):**

* Ein Batch wird **eintragweise** validiert und verarbeitet; die Datenbankbestätigung gilt **je Eintrag** (`persist_revision` je Charakter, §29).
* Ein problematischer, **eindeutig zuordenbarer** Eintrag wird zuerst **dauerhaft quarantänisiert** (§32); danach laufen die übrigen Einträge desselben Batches weiter. Ein problematischer Eintrag ist **keine** Sperre für den ganzen Batch.
* Schlägt die Quarantänesicherung fehl, gilt der Eintrag **nicht** als erledigt; die Datei bleibt liegen und wird erneut versucht.
* Die Batch-Datei wird **erst entfernt, wenn jeder Eintrag** datenbankbestätigt oder dauerhaft quarantänisiert ist. Nach einem Abbruch ist die Wiederaufnahme sicher und ohne Doppel-Apply.
* Ein insgesamt unlesbares Batch und ein **Attributionskonflikt** bleiben `fail-safe` nach `P-30`: keine Zuordnung wird erfunden, es erfolgt keine Teilverarbeitung mit geratenen IDs.
* Die charakterbezogenen Garantien aus `P-30` — Quarantäne-Sperre je Charakter, Charakter-Gate, Reverify, Doppel-Apply-Schutz, automatische Recovery, Betreiberzähler — bleiben unverändert wirksam.

**Kompatibilität zum bisherigen Einzeldatei-Format (V1):**

Bereits veröffentlichte Dateien im Format „eine Datei je Spieler-Snapshot" (`<captured_at_ms:013>-<player_id>-r<revision>.json`) bleiben unverändert **sicher lesbar und abarbeitbar**; sie werden weder umbenannt noch gelöscht. Neue Schreibvorgänge erzeugen ausschließlich das gemeinsame Batch-Format. Das konkrete technische Schema ist in Abschnitt 42 festgehalten.

**Stabile Batch-Darstellung und Veröffentlichung (verbindlich, P-12 entschieden):**

* Die Einträge eines Batches werden **vor** Namensbildung und Serialisierung **kanonisch** nach stabiler Spieleridentität geordnet. Name und serialisierter Inhalt hängen damit nicht von der Eingabereihenfolge ab: derselbe vollständige Batch ergibt bei vertauschter Eingabe denselben Schreibinhalt.
* Doppelte Spieleridentitäten innerhalb eines Batches werden **eindeutig abgewiesen**; es findet keine stillschweigende Verdrängung eines Eintrags statt.
* Der Dateiname ergibt sich aus `min(captured_at_ms)` und einem Digest über die kanonisch geordneten Einträge. Der Digest ist ein **Namensteil und kein kollisionsfreier Inhaltsnachweis**; Inhaltsgleichheit wird ausschließlich über einen vollständigen Strukturvergleich bestimmt.
* Eine bereits vorhandene Zieldatei wird **nicht überschrieben**. Sie gilt nur dann als bereits erledigter Schreibvorgang, wenn ihr vollständiger Inhalt mit dem zu sichernden Batch übereinstimmt; andernfalls bleibt sie erhalten, der Lauf meldet einen Konflikt und der Dirty-Zustand bleibt für einen späteren Versuch erhalten.
* Die Veröffentlichung erfolgt **atomar und ohne Überschreiben**: Der vollständige Inhalt wird ausschließlich unter einem **temporären** Namen geschrieben und dort gesichert. Erst danach wird er unter dem finalen Namen **atomar sichtbar gemacht**. Unter dem finalen Namen entsteht zu keinem Zeitpunkt eine unvollständige oder teilweise geschriebene Datei; ein während der Erstellung laufender Lesevorgang (Drain) kann daher keinen halbfertigen Batch lesen.
* Der atomare Schritt ersetzt einen belegten Zielnamen **nie**. Eine bloße Existenzprüfung vor dem Umbenennen genügt dafür nicht, weil sie zwischen Prüfung und Umbenennung anfechtbar ist; der Schritt selbst muss den vorhandenen Namen zurückweisen. Bei Konkurrenz entscheidet der vollständige Inhaltsvergleich, und eine bereits veröffentlichte Datei geht nicht verloren.
* Bricht der Vorgang **vor** der Veröffentlichung ab, bleibt höchstens eine temporäre Datei zurück und **kein** unvollständiger finaler Batch; der Zustand bleibt über einen späteren Lauf sicher wiederholbar.
* Die Dauerhaftigkeitsgrenze umfasst **Datei-Inhalt und Verzeichniseintrag**: Der Verzeichnis-Sync ist keine bloße Nebenläufigkeit, sondern Bestandteil des Nachweises. Schlägt er fehl, gilt die Veröffentlichung als **nicht** abgeschlossen — es erfolgt keine Dirty-Rücknahme und keine Freigabe der einzigen gesicherten Zustandskopie. Ein späterer Versuch bestätigt Inhalt **und** Dauerhaftigkeit erneut.

Es gibt **keine** Zusammenführung/Kompression über mehrere Batches hinweg und kein verzögerungsfreies Neuschreiben älterer Batches (V1). Bereits geschriebene Batches bleiben unverändert erhalten, bis sie gemäß Abschnitt 36 erledigt sind.

Durability: Für jede Batch-Datei gilt das Safe-Write-Prinzip aus Abschnitt 25 – erst nach vollständigem Schreiben, dauerhafter Sicherung (flush/fsync) und atomarer Umbenennung gilt die Datei als gültig.

## 36. Stufe B – Batch-Verarbeitung / Reihenfolge und Einzelsemantik

Ausstehende Batches werden in Reihenfolge verarbeitet, **ältester zuerst** (Abschnitt 28). Die `persist_revision` verhindert dabei das Überschreiben eines neueren DB-Stands.

Eine Batch-Datei wird erst dann entfernt, wenn **alle** ihre Player-Einträge erledigt sind. Ein Eintrag gilt als erledigt, wenn:

* (a) sein Snapshot erfolgreich mit dem aktuellen Persistenzcode committet wurde, oder
* (b) er per Revision als gleich/überholt befundet wurde (Abschnitt 29), oder
* (c) er einzeln in Quarantäne überführt wurde (Abschnitt 31), oder
* (d) er als Superseded-Snapshot sicher in `superseded/` übernommen wurde (Abschnitt 34).

Die Player-Einträge eines Batch werden **unabhängig** voneinander behandelt: Ein fehlerhafter, in Quarantäne überführter oder als superseded kenntlich gemachter Eintrag blockiert die übrigen Einträge desselben Batch nicht. Einzeleinträge werden einzeln isoliert; die Batch-Datei bleibt solange erhalten, bis alle Einträge gemäß (a)–(d) erledigt sind.

## 37. Stufe B – Sequentielle Verarbeitung (V1)

Für V1 ist festgelegt: Die Abarbeitung der Spool-Batches nach MariaDB erfolgt **sequenziell** in einem einzigen Durchlauf – keine parallelen Worker, kein Pool.

Eine spätere Parallelisierung bleibt möglich und wird in Abschnitt 42 als offener Punkt geführt.

## 38. Stufe B – Entkopplung von Spool und MariaDB

Die lokale Spool-Sicherung und der Transfer nach MariaDB sind fachlich **entkoppelt**:

* Die Erzeugung eines Batch ist unabhängig vom aktuellen MariaDB-Zustand.
* Zu einem Zeitpunkt wird höchstens **ein** Batch erzeugt (Abschnitt 22).
* Schlägt der MariaDB-Transfer fehl, entsteht der Rückstau in der **Spool** (bereits dauerhaft gesicherte Batch-Dateien), nicht im RAM. Neue Änderungen werden in nachfolgenden Läufen normal erfasst und als weitere Batches gesichert.

Dadurch bleibt die periodische Persistenz funktionsfähig, auch wenn MariaDB längere Zeit nicht erreichbar ist.

## 39. Stufe B – RAM-/Dirty-Verhalten und persist_generation

Die Race-Lösung aus Abschnitt 15 wird über eine serverseitig erzeugte, monoton aufsteigende `persist_generation` umgesetzt (Abschnitt 20):

* Die Generation wird beim Erfassen des PersistSnapshots mitgeführt und ist Bestandteil des Snapshot-/Batch-Modells (serverseitig, nie client-bestimmt).
* Beim Verarbeiten kann so erkannt werden, ob der Snapshot noch aus dem aktuellen Lauf/Lebenszyklus stammt und ob zwischen Snapshot und Abschluss entstandene Änderungen vorliegen (Race-Erkennung gemäß Abschnitt 15).

Dirty-Verhalten:

* Dirty-Bits werden erst nach **dauerhafter** Spool-Sicherung des betreffenden PersistSnapshots bereinigt, also nach gültigem Batch-Schreiben (Abschnitt 35; Abschnitt 7).
* Die temporäre RAM-Kopie des PersistSnapshots kann nach dauerhafter Spool-Sicherung freigegeben werden (Abschnitt 21).
* Die dauerhafte Übertragung nach MariaDB ist davon getrennt; erst nach erfolgreichem DB-COMMIT wird die Batch-Datei entfernt (Abschnitt 36).

**Revision wird erst durch durable Spool real:**

Eine neue `persist_revision` gilt im Live-Zustand erst dann als übernommen, wenn der zugehörige **vollständige** Player-Snapshot Bestandteil eines erfolgreich dauerhaft geschriebenen Batches ist (Abschnitt 23).

Beispiel:

* RAM `persist_revision` = 5, Player dirty → geplanter Snapshot `persist_revision` = 6.
* Spool-Write schlägt fehl → Revision 6 gilt NICHT als erfolgreich erzeugt/übernommen; RAM bleibt auf 5; Player bleibt dirty; der nächste reguläre Versuch darf erneut Revision 6 verwenden (fehlgeschlagene Spool-Schreibversuche erzeugen keine künstlichen Revisionslücken, Abschnitt 40).
* Spool-Write erfolgreich und nach den Durability-Regeln dauerhaft → Snapshot Revision 6 existiert dauerhaft; Live-RAM darf `persist_revision` 6 übernehmen; MariaDB darf zu diesem Zeitpunkt noch Revision 5 besitzen.
* Dadurch kann später bereits Revision 7 in einem neuen Batch entstehen, während MariaDB Revision 6 noch verarbeitet.

WICHTIG – `persist_revision` und `persist_generation` haben unterschiedliche Aufgaben:

* `persist_generation`: erkennt Änderungen/Races im aktuellen Live-RAM während der Snapshot-Erstellung (Abschnitt 15).
* `persist_revision`: ordnet dauerhaft erzeugte Player-Snapshots und DB-Stände (Abschnitt 29).

Diese beiden Mechanismen NICHT vermischen.

## 40. Stufe B – Verhalten bei Spool-Schreibfehlern

Schlägt das (dauerhafte) Sichern eines Batch in die Spool fehl:

* Die Dirty-Bits werden **nicht** bereinigt – der Zustand bleibt dirty und für einen späteren Retry verfügbar (Abschnitt 7).
* Die geplante `persist_revision` des fehlgeschlagenen Versuchs wird **nicht** verbraucht: Der nächste reguläre Versuch darf dieselbe Revision erneut verwenden – fehlgeschlagene Spool-Schreibversuche erzeugen keine künstlichen Revisionslücken (Abschnitt 39).
* Der Persistence-Zustand des betroffenen Servers/Realms wird **DEGRADED** (Abschnitt 26).
* Es werden **keine** gültigen bereits geschriebenen Spool-Dateien gelöscht.
* Temporär geschriebene Dateien gelten **niemals** als gültig (Abschnitt 25).

Der Realm läuft weiter; ein späterer normaler Persistenzlauf versucht erneut.

## 41. Stufe B – Graceful Shutdown bei beidseitigem Ausfall

Beim Graceful Shutdown sind zwei Persistierungen zu unterscheiden:

* MariaDB-Transfer erfolgreich → Zustand ist endgültig in MariaDB gesichert.
* MariaDB-Transfer fehlgeschlagen, aber lokale Spool-Sicherung gelungen → Zustand bleibt lokal für die spätere Recovery erhalten (Abschnitt 30).

Schlagen beim Shutdown **beide** Wege fehl (weder MariaDB-COMMIT noch lokale Spool-Sicherung), gilt:

* Der Vorgang wird **NICHT** als erfolgreich persistiert gemeldet.
* Der Zustand ist nicht dauerhaft gesichert – dies ist als **schwerwiegender Persistence-Fehler** zu behandeln und zu protokollieren.
* Ein vermeintlich erfolgreicher Shutdown, dessen Daten weder in MariaDB noch in der Spool enthalten sind, wäre ein Fehler.

---

## 42. Stufe B – Bewusst offen gelassene / nicht festgelegte Punkte

Ohne vorhandene Entscheidung werden NICHT festgelegt:

* konkretes JSON-Schema (Feldnamen, Struktur) – das Format selbst ist versioniertes menschenlesbares JSON (Abschnitt 35)
* konkrete Dateinamen
* konkrete Superseded-Dateinamen
* absolute Spool-/Superseded-/Quarantäne-Pfade (die Struktur selbst ist relativ festgelegt: Abschnitte 31/34)
* konkrete MariaDB-Tabelle/Spalte/Datentyp für `persist_revision` (Abschnitt 29)
* konkrete DB-Spaltennamen
* konkrete DB-Migration
* maximale Spool-Größe
* maximale Anzahl Spool-Dateien
* Spool-/Superseded-Größenlimits
* batch_id-Implementierung
* konkrete Push-Technik/App-Technik
* konkrete HTTP/API-Endpunkte
* spätere Parallelisierung der DB-Spool-Abarbeitung – für V1 ist **sequenzielle** Verarbeitung festgelegt (Abschnitt 37)
* Snapshot-Kompression
* Zusammenführen/Kompression über mehrere Batches hinweg – innerhalb eines Batch gibt es keine Zusammenführung (Abschnitt 35)
* exakte Retry-Zeitpunkte außerhalb des normalen Persistenzzyklus
* genaue technische Erzeugung der `persist_revision` (z.B. pro Charakter vergebene Sequenz) – der Mechanismus selbst ist festgelegt (Abschnitt 29)
* konkrete Rust-Strukturen der neu hinzugekommenen Snapshot-Komponenten (Abschnitte 23/25)
* konkrete Writer-Aufteilung für die neu hinzugekommenen Snapshot-Komponenten
* ~~konkrete Dirty-Bit-Aufteilung~~ – **GESCHLOSSEN** in Abschnitt 23 („NORMATIVE DIRTY-KOMPONENTEN-ZUORDNUNG“): Die Zuordnung der Attribute, aktuellen HP, aktuellen Mana, Klasse, des Fraktionszustands, der Weapon Skills und der Abilities ist verbindlich festgelegt. Die dort ausdrücklich offen gelassene **Gewichtung/Flush-Priorisierung** bleibt davon unberührt und weiterhin offen.
* genaue HP-Max-/Mana-Max-Berechnung (die Werte selbst sind abgeleitet, Abschnitt 23)
* genaue Armor-Berechnung (der Wert selbst ist abgeleitet, Abschnitt 23)
* neue Gameplay-Regeln
* Quest V1.2b
* neue Item-/Loot-Regeln

**GESCHLOSSEN — Fehlersemantik des `logout_at`-Schreibens:** Die grundsätzliche Fehlersemantik des direkten Logout-Writes ist entschieden und in Abschnitt 11 verbindlich festgelegt (begrenzter Retry; danach `Degraded`, strukturiertes Logging ohne Session-ID/Tokens/Roh-IP, kontrolliert weiterlaufender Cleanup; Shutdown-Regel in Abschnitt 30). Bewusst **nicht** festgelegt und Aufgabe des Coding-Plans sind nur noch die technischen Parameter des begrenzten Retries (Versuchsanzahl, Abstände, Zeitbudget) anhand der vorhandenen DB-Timeouts. Der Snapshot- oder Spool-Bau repariert `logout_at` nicht; dauerhafte Retry-/Recovery-Lösungen sind ausdrücklich nicht Teil dieses Punktes und werden allenfalls in einem späteren Release-Audit neu bewertet.

Enthält bestehende Dokumentation zu einem dieser Punkte bereits eine verbindliche Regel, wird sie nicht stillschweigend geändert; ein solcher Konflikt wird gemeldet.