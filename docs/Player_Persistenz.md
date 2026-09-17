# Andora – Player-Persistenzstrategie (laufende Spielerzustände)

## 1. Status

**Player-Persistenz Stufe A (Dirty-State, periodischer Player-Save, Disconnect-/Shutdown-Flush) ist implementiert und abgeschlossen.**

**Die Spool-/Recovery-Architektur (Stufe B) ist in dieser Datei dokumentiert, aber noch nicht implementiert.**

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

Ein Spieler besitzt während seiner Session einen kompletten, konsistenten persistenzen Abbildzustand im Realm-RAM (Spielposition, Progression, Gold, Inventar, Questzustand/-fortschritt). Das Spiellayer arbeitet ausschließlich gegen diese RAM-Repräsentation.

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
Questfortschritt:   12/100 → 13/100
EXP-Zuwachs
Goldänderungen
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
gold dirty
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

---

## 7. Periodischer Flush

Beim periodischen Save werden **nur tatsächlich dirty** gewordene persistente Spielerzustände geschrieben.

Ein Spieler, dessen persistenter Zustand sich seit dem letzten erfolgreichen Save nicht verändert hat, benötigt keinen unnötigen Player-State-Write.

Regeln:

* Nach **erfolgreicher** Persistierung darf der entsprechende Dirty-Zustand **zurückgesetzt** werden.
* Bei **fehlgeschlagener** Persistierung gilt der Zustand **nicht als sauber**: Er bleibt dirty und muss für einen späteren Retry verfügbar bleiben (Abschnitt 16).

In der Stufe-B-Architektur verläuft der periodische Persistenzlauf über die lokale Persistence-Spool (Abschnitt 21): Der erfasste PersistSnapshot wird zunächst sicher lokal geschrieben und von dort mit dem aktuellen Persistenzcode nach MariaDB übertragen.

---

## 8. Questfortschritt

Normaler Questfortschritt wird **nicht bei jedem einzelnen Ereignis sofort** persistent geschrieben.

Beispiel:

```text
Quest: Töte 100 Wölfe.
```

Nicht:

```text
1/100 → DB
2/100 → DB
3/100 → DB
...
```

Sondern:

```text
Kill bestätigt
→ Questfortschritt im Realm-RAM erhöhen
→ Questkomponente dirty markieren
```

Beim nächsten periodischen/finalen Flush wird der aktuelle Fortschritt persistiert.

Bei einem ungeplanten Prozess-/Host-Crash kann dadurch Fortschritt seit dem letzten erfolgreichen Flush verloren gehen. Das ist für normalen Zwischenfortschritt **bewusst akzeptiert** (insofern verändert sich gegenüber dem bisherigen Verhalten nichts: auch bisher wurde EXP erst bei Disconnect gespeichert, vgl. `Erfahrung_und_Progressionssystem.md`).

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

Normaler Objective-Fortschritt innerhalb ACTIVE darf anschließend über Dirty-State/periodischen Flush laufen (Abschnitt 8).

Aus dieser Regel werden **keine** weiteren Repeatable-/FAILED-/Abort-Regeln abgeleitet (die entsprechenden Semantiken bleiben in `Quest-System.md` offen, vgl. Abschnitte 27.20/27.21).

---

## 11. Disconnect

Bei einem normalen Spieler-Disconnect erfolgt ein **finaler Flush** der persistenzpflichtigen Dirty-Zustände des Spielers.

Die Architektur verwendet nach Möglichkeit **denselben zentralen Player-Persistenzpfad** wie der periodische Save.

Es werden keine voneinander abweichenden Persistenzregeln für dieselben Komponenten dupliziert: Der Disconnect-Save ist ein sofort ausgelöster Flush über denselben Pfad, nicht ein Satz eigener, paralleler Speicherlogik.

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

* jeder Kill-Questfortschritt
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
Snapshot enthält Quest 20/100.
Während des DB-Writes steigt RAM auf 21/100.
```

Nach erfolgreichem Write von 20/100 darf der neue 21/100-Zustand **nicht versehentlich als clean** markiert werden. Eine passende Technik (beispielsweise Generation Counter, Version oder Vergleich) muss diese Race-Bedingung verhindern.

Die konkrete technische Lösung wird **nicht** in dieser Doku festgelegt (Abschnitt 20) – die Implementierung muss die Bedingung jedoch verhindern.

Der dabei erfasste konsistente Zustand entspricht dem PersistSnapshot der Stufe-B-Spool-Architektur (Abschnitt 21).

---

## 16. Save-Fehler

Ein fehlgeschlagener periodischer Save:

* darf den Realm **nicht automatisch beenden**
* muss **geloggt/telemetriert** werden
* lässt die betroffenen Zustände **dirty** (Abschnitt 7)
* darf von einem späteren Flush **erneut versucht** werden

Konkrete Retry-Abstände/-Anzahlen werden **nicht** festgelegt.

Für Graceful-Shutdown-Fehler darf später eine eigene begrenzte Retry-/Shutdown-Regel definiert werden.

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
* Generation-Counter-Implementierung / Race-Lösung aus Abschnitt 15
* exakte Save-Batchgröße
* konkrete Retry-Zeiten
* Parallelitätsgrad der DB-Saves
* genaue Shutdown-Timeouts
* adaptive Intervalle (für V1 generell nicht vorgesehen, Abschnitt 18)
* genaue Einbindung/Auswertung des `PLAYER_PERSIST_INTERVAL_MS`-Config-Keys in `config.rs`

Zusätzlich für Stufe B (Spool-/Recovery-Architektur) bewusst offen gelassene Punkte: Abschnitt 34.

---

## 21. Stufe B – Spool-/Recovery-Architektur (Grundarchitektur)

**Status:** Architektur dokumentiert; Implementierung in einem separaten Coding-Auftrag.

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

---

## 23. Stufe B – Für die Spool vorgesehene normale Dirty-Zustände

Die Spool ist für normale persistierbare Player-Zustände vorgesehen, insbesondere:

* Position
* Progression
* Gold
* persistentes Inventar
* Quest-State / normaler Quest-Fortschritt

Die bestehenden Regeln zu komponentenbezogenem Dirty-State und zu Generationen bleiben bestehen (Abschnitte 6 und 15).

Der **Inventory Buffer** bleibt runtime-only und darf NICHT persistiert werden (Abschnitt 6; `inventory_system.md`).

Die **Buyback History** bleibt session-only und gehört NICHT in diese Player-Persistenz.

---

## 24. Stufe B – Kritische / atomare Transaktionen

Die Spool ersetzt die bestehenden Sofort-/Atomar-Regeln NICHT.

Quest Acceptance:
→ unmittelbare Persistenz entsprechend der bestehenden Architektur (Abschnitt 10).

Quest Completion:
→ bestehende atomare MariaDB-Transaktion bleibt unverändert (Abschnitt 9; `Quest-System.md` §27.26).

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

Ein endgültiges Serialisierungsformat/API wird hier nicht festgelegt (Abschnitt 34).

Architekturentscheidung:

> Emergency-/Spool-Persistenz speichert DATEN, nicht fehlgeschlagene SQL-Befehle.

Kein `backup.sql`-Konzept.

Grund:

Ein Fehler kann gerade im alten SQL-/Persistenzcode liegen. Nach einem Fix soll ein neuer Server die gespeicherten Daten mit dem aktuellen, reparierten Rust-Persistenzcode erneut nach MariaDB übertragen können.

Das Datenformat muss versionierbar sein, z.B. über eine `format_version`.

Unbekannte/nicht unterstützte Formatversionen dürfen nicht blind eingespielt werden.

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

---

## 27. Stufe B – Monitoring-Vorbereitung

Keine Monitoring-Implementierung in der Stufe B. Dokumentiert werden hier die Anforderungen für eine spätere Monitoring-Stufe.

Der bestehende Webserver soll später mindestens darstellen können:

* Persistence-Status, z.B. **HEALTHY / DEGRADED / RECOVERING**
* letzter erfolgreicher DB-Persistenzzeitpunkt
* Zeitpunkt/Beginn eines anhaltenden Fehlers
* Anzahl ausstehender Spool-Snapshots
* Alter des ältesten ausstehenden Snapshots
* Gesamtgröße der ausstehenden Spool-Daten
* Anzahl offener Quarantänefälle
* Anzahl archivierter Quarantänefälle der letzten 30 Tage
* soweit sinnvoll Fehlergruppen/Kategorien

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

Erst wenn alle normal verarbeitbaren Spool-Snapshots erledigt oder ordnungsgemäß aus der aktiven Recovery in Quarantäne überführt wurden, darf der Realm **READY** werden.

Monitoring/Administration soll während **RECOVERING** weiterhin verfügbar sein.

---

## 29. Stufe B – Neuerer Zustand gewinnt

Jede DB-Persistenz soll einen serverseitig erzeugten zeitlichen bzw. versionierten Persistenzbezug besitzen, sodass beim Recovery erkannt werden kann, ob der Spool-Zustand oder der vorhandene DB-Zustand neuer ist.

Der Client darf diesen Wert NICHT bestimmen.

Der Dateiname eines Snapshots darf einen lesbaren Zeitstempel enthalten, ist aber NICHT alleinige autoritative Grundlage für die Recovery-Entscheidung.

Die entscheidenden Metadaten müssen Bestandteil des Snapshot-/Persistenzmodells sein.

Konkrete Spaltenbezeichnungen oder DB-Migrationen werden hier nicht erfunden (Abschnitt 34).

Fachliche Regel:

* Spool neuer als DB → Recovery erforderlich.
* DB gleich oder neuer → Snapshot darf als bereits überholt/erledigt behandelt werden.
* unklarer Zustand → nicht blind überschreiben.

---

## 30. Stufe B – Graceful Shutdown und Spool

Die Spool dient gleichzeitig als Sicherheitsmechanismus beim kontrollierten Shutdown.

Wenn ein finaler Player-Zustand beim Shutdown nicht erfolgreich nach MariaDB übertragen werden kann, muss der noch nicht dauerhaft in MariaDB gesicherte Zustand lokal in der Persistence-Spool erhalten bleiben.

Dadurch ist KEIN separates `backup.sql`-System erforderlich (`Datenbank_Architektur.md` Abschnitt 25 bleibt als DB-Backup-Ebene unverändert gültig).

Nach einem späteren Bugfix kann der neue Realm-Prozess die gespeicherten Daten mit dem aktuellen Persistenzcode wiederherstellen.

Die detaillierte Shutdown-Implementierung bleibt einer späteren Stufe vorbehalten (Stufe D). Hier wird nur die Architektur dokumentiert.

Die Final-Save-Reihenfolge aus Abschnitt 12 bleibt unverändert gültig.

---

## 31. Stufe B – Irreparabel beschädigte Snapshots / Quarantäne

Ein einzelner irreparabel beschädigter oder nicht mehr automatisch verarbeitbarer Snapshot darf den gesamten Realm NICHT dauerhaft am Start hindern.

Verzeichnissemantik:

```text
persistence/
├── spool/
│   └── ausstehende, noch nach MariaDB zu übertragende Snapshots
│
└── quarantine/
    ├── offene, noch nicht untersuchte fehlerhafte Snapshots
    │
    └── archive/
        └── bereits untersuchte/bearbeitete Quarantänefälle
```

Kann ein Snapshot nicht sicher wiederhergestellt werden:

* aus aktiver Spool in `quarantine/` verschieben
* Originaldaten für Analyse erhalten
* Fehler und relevante Metadaten protokollieren
* Recovery mit anderen Snapshots fortsetzen

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

---

## 33. Stufe B – Quarantäne-Aufbewahrung

VERBINDLICHE REGEL:

Dateien direkt in:

```text
persistence/quarantine/
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

---

## 34. Stufe B – Bewusst offen gelassene / nicht festgelegte Punkte

Ohne vorhandene Entscheidung werden NICHT festgelegt:

* endgültiges JSON-/Binärschema
* konkrete Dateinamen
* konkrete DB-Spaltennamen
* konkrete DB-Migration
* maximale Spool-Größe
* maximale Anzahl Spool-Dateien
* konkrete Push-Technik/App-Technik
* konkrete HTTP/API-Endpunkte
* Parallelität/Worker-Anzahl der DB-Spool-Abarbeitung
* Snapshot-Kompression
* Zusammenführen/Ersetzen mehrerer Snapshots desselben Spielers
* exakte Retry-Zeitpunkte außerhalb des normalen Persistenzzyklus
* neue Gameplay-Regeln
* Quest V1.2b
* neue Item-/Loot-Regeln

Enthält bestehende Dokumentation zu einem dieser Punkte bereits eine verbindliche Regel, wird sie nicht stillschweigend geändert; ein solcher Konflikt wird gemeldet.