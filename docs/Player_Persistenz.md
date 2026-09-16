# Andora – Player-Persistenzstrategie (laufende Spielerzustände)

## 1. Status

**Dokumentation (Strategie, noch nicht vollständig implementiert).**

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
* Dirty-Zustände werden **periodisch nach MariaDB** geschrieben.
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

---

## 16. Save-Fehler

Ein fehlgeschlagener periodischer Save:

* darf den Realm **nicht automatisch beenden**
* muss **geloggt/telemetriert** werden
* lässt die betroffenen Zustände **dirty** (Abschnitt 7)
* darf von einem späteren Flush **erneut versucht** werden

Konkrete Retry-Abstände/-Anzahlen werden **nicht** festgelegt.

Für Graceful-Shutdown-Fehler darf später eine eigene begrenzte Retry-/Shutdown-Regel definiert werden.

---

## 17. Monitoring

Die Architektur soll später mindestens beobachtbar machen können:

* erfolgreiche periodische Saves
* fehlgeschlagene Saves
* Save-Dauer
* Anzahl persistierter Spieler/Komponenten
* Dirty-Zustände bzw. Backlog, soweit sinnvoll

Es wird **keine konkrete Dashboard-UI** in dieser Datei entworfen. Die vorhandene Monitoring-Schnittstelle (`monitoring_web_panel.md`, `/status`-Metriken) ist der natürliche Anzeige-Ort; die konkreten Metrikfelder legt der Coding-Auftrag fest.

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