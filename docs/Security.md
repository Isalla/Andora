# Andora – Security, Persistenz und Zustandsintegrität (zentrale Aufgabenübersicht)

## 1. Zweck und Statusregeln

**Zweck:** Diese Datei sammelt alle offenen Sicherheits-, Persistenz- und Zustandsintegritätsprüfungen des Andora-Projekts als verbindliche Aufgabenübersicht. Sie dient dazu, dass offene Punkte bei Dokumentationsprüfungen sichtbar bleiben. Sie ersetzt keine Fachdokumente (`docs/Player_Persistenz.md`, `docs/Serverautoritaet_und_Anti-Manipulation_V1.md`); bei Widersprüchen haben die Fachdokumente Vorrang, bis ein Punkt hier als `ERLEDIGT` geführt wird.

**Verhältnis zu den Fachdokumenten:** `Security.md` überschreibt niemals automatisch ein Fachdokument. Widersprüche zwischen `Security.md` und einem Fachdokument werden als offene Abweichung erfasst. Die normative Entscheidung wird ausdrücklich getroffen und anschließend im zuständigen Fachdokument festgehalten; `Security.md` trägt die Abweichung und den Stand der Entscheidung nur fort.

**Geltungsbereich dieser Datei:** Nur Dokumentation. Aus dieser Datei folgt keine Produktionscode-Änderung, kein neuer Test und kein externer Scan. Kein Punkt wird allein aufgrund dieser Aufgabe als bestätigt markiert. Jeder Eintrag erscheint genau einmal. Es werden keine Pfade, Dateinamen, Implementierungsstände oder Befunde erfunden; verwendet werden nur im Repository verifizierte Fakten mit Belegstellen.

**Statusregeln:**

- `ZU PRÜFEN` – noch kein bestätigter Fehler; Prüfung steht aus.
- `BESTÄTIGT` – Doku und Code belegen den Befund gemeinsam.
- `ENTSCHEIDUNG OFFEN` – zuerst ist eine fachliche oder architektonische Entscheidung nötig.
- `ERLEDIGT` – implementiert, dokumentiert, getestet, bestätigt.

**Status und Priorität sind getrennt:** Status bezeichnet den Erkenntnisstand (`ZU PRÜFEN`, `BESTÄTIGT`, `ENTSCHEIDUNG OFFEN`, `ERLEDIGT`); Priorität (`GERING` / `MITTEL` / `HOCH`) bezeichnet die Dringlichkeit beziehungsweise das mögliche Risiko. Eine Priorität bestätigt keinen ungeprüften Befund; sie ändert den Status nicht. Ist im Auftrag keine Priorität vergeben, steht der Eintrag auf `offen (im Auftrag nicht vergeben)`; eine Priorität wird nicht erfunden.

## 2. Bereits abgeschlossen

### Serverautorität und Anti-Manipulation V1

- **Status:** `ERLEDIGT`
- **Belege:** Commit `60b10fe` („Implement server authority and anti-manipulation V1“, 12 Dateien, u. a. `src/realm-rs/src/security.rs` +857 Zeilen, `docs/Serverautoritaet_und_Anti-Manipulation_V1.md` +177 Zeilen); 16 Testfunktionen in `src/realm-rs/src/security.rs`; verlinkt im Inhaltsverzeichnis (`docs/README.md`, Kategorie 11).
- **Abschlussnachweis:** Code, Fachdoku und Inhaltsverzeichnis-Eintrag vorhanden; Testfunktionen im Repository zählbar.

### P-20 – Verhalten bei fehlgeschlagenem Spool-Write

- **Status:** `ERLEDIGT`
- **Belege:** Test `failed_spool_write_sets_degraded_and_keeps_dirty_and_revision` in `src/realm-rs/src/spool.rs:837` mit Code-Kommentar zu `docs/Player_Persistenz.md` §40 (`src/realm-rs/src/spool.rs:838`); Doku-Abschnitt `docs/Player_Persistenz.md:1078` (§40). Verifiziertes Verhalten: Fehlschlag setzt `PersistStatus::Degraded`, Dirty-Bit und `persist_revision` bleiben erhalten.
- **Abschlussnachweis:** Test im Repository vorhanden; Doku §40 beschreibt die Semantik.

### DOK-01 – Veraltete Stage-B-Statusbeschreibung korrigiert

- **Status:** `ERLEDIGT`
- **Belege:** Commit `fcbbc5d` („Update Stage-B persist logic and documentation“); Diff in `docs/Player_Persistenz.md`: „dokumentiert, aber noch nicht implementiert“ → „implementiert und in der Datei dokumentiert“; §21-Statuszeile ebenfalls auf „dokumentiert und implementiert“ korrigiert.
- **Abschlussnachweis:** Korrektur per `git show fcbbc5d -- docs/Player_Persistenz.md` nachvollziehbar.

### AUTH-03A – Verbindungs-Einzigkeit und Takeover im Realm

- **Status:** `ERLEDIGT`
- **Belege:** Commit `e499dec8d9e9db471f0a3ecb222b1fd869bf643c` („Enforce single realm connection per character“, 7 Dateien: `src/realm-rs/src/handlers.rs`, `net.rs`, `parental.rs`, `persist.rs`, `security.rs`, `spool.rs`, `world.rs`). Genau eine aktive, zur Spiellogik berechtigte Realm-Verbindung pro Charakter: `by_conn` wird ausschließlich über `commit_login` verändert (`src/realm-rs/src/world.rs:304-375`), alle übrigen Pfade lesen nur. Authentifizierter Takeover: Handoff-/Session-Prüfung, Account-/Charakterprüfung und alle falliblen Vorprüfungen einschließlich Elternkontrolle laufen vor dem Commit (`src/realm-rs/src/handlers.rs:138-151`, `:296-320`); die alte `conn_id` wird vor dem Setzen der neuen entfernt und erst danach der vorhandene Closer signalisiert (`src/realm-rs/src/handlers.rs:327-365`). Autoritativer RAM-Zustand bleibt maßgeblich: der bestehende Player wird nicht ersetzt, aktualisiert werden nur `tx`, `session_id`, `lang` und `last_activity` (`src/realm-rs/src/world.rs:254-259`); WELCOME wird aus dem RAM-Stand gesendet (`src/realm-rs/src/handlers.rs:386-400`). Stale Cleanup berührt den neuen Owner nicht: `disconnect_conn` entfernt ausschließlich die Zuordnung der übergebenen `conn_id` statt charakterweit (`src/realm-rs/src/world.rs:597-612`), `is_owner` wird vor dem Flush, vor `logout_at` und unter derselben Sperre wie das Entfernen geprüft (`src/realm-rs/src/world.rs:239-241`, `src/realm-rs/src/net.rs:316-374`), Lesefehler beenden die Schleife kontrolliert statt den Cleanup zu überspringen (`src/realm-rs/src/net.rs:168-186`), der Parental-Force-Logout schließt gezielt die Eigentümer-Verbindung ohne vorzeitiges Cleanup (`src/realm-rs/src/parental.rs:319-327`). Login und Disconnect-Commit sind pro `player_id` über das bestehende per-player-Gate serialisiert (`src/realm-rs/src/spool.rs:95-102`, `src/realm-rs/src/handlers.rs:150-151`, `src/realm-rs/src/net.rs:316-317`); die Sperrenreihenfolge ist Gate → World → Elternkontrolle/Gruppen, ein inverser Pfad World → Gate ist im Repository nicht vorhanden. Takeover-Logging als INFO-Ereignis `authenticated_connection_takeover` mit Account-ID, Charakter-ID, alter und neuer `conn_id` sowie Zustand der verdrängten Verbindung, ohne Roh-IP (`src/realm-rs/src/security.rs:170-189`).
- **Abschlussnachweis:** 436 Tests bestanden, 0 fehlgeschlagen (`cargo test` in `src/realm-rs`; 436 Testattribute im Quellcode zählbar); `cargo clippy --all-targets --all-features` ohne Fehler (Warnungen unverändert gegenüber dem Vorzustand). Nicht Bestandteil dieses Abschlusses und weiterhin offen: die dauerhafte Takeover-IP-Protokollierung mit 14-Tage-Löschung (Logging-Anforderung in Abschnitt 4.1, `AUTH-03`), der Widerspruch im Erstellungsverhalten von `db::load_character` sowie der fehlende Integrationstest der `pending_revision`-Verdrahtung im Login-Pfad (kein Datenbank-Testlauf möglich; `pending_revision` und die Entscheidungsfunktion sind einzeln getestet).

### Testbestand (Zählung im Quellcode verifiziert)

- **Status:** `ERLEDIGT` (historisch belegt; kein Testlauf in dieser Aufgabe)
- **Belege:** Historischer Verifikationsnachweis aus dem P-20-Abschlussbericht: 416 Tests bestanden, 0 fehlgeschlagen. Im Rahmen der Erstellung dieser Dokumentation wurde die Testsuite nicht erneut ausgeführt. Durch Quellcode-Suche wurden 416 Testfunktionen erfasst (davon `security.rs`: 16, `spool.rs`: 7); diese Zählung ersetzt keinen Testlauf.
- **Abschlussnachweis:** Historischer Nachweis im P-20-Abschlussbericht vermerkt; Quellzählung im Repository wiederholbar.

## 3. Bekannte offene Punkte

### P-11 – Verzeichnis-fsync nach atomarem Rename des Spool-Files

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Auftrag nicht vergeben)
- **Betroffener Bereich:** Spool-Durability (`src/realm-rs/src/spool.rs`, `docs/Player_Persistenz.md` §25)
- **Bekannte Ausgangslage:** `write_atomic` sichert die Temp-Datei per `f.sync_all()` und benennt danach um; ein Verzeichnis-fsync nach dem Rename findet nicht statt.
- **Offene Frage / Entscheidung:** Ob für Crash-Sicherheit zusätzlich ein fsync des Zielverzeichnisses nach dem Rename erforderlich ist.
- **Verifizierte Belege:** `src/realm-rs/src/spool.rs:552-559` (`write_atomic`: `sync_all` in Zeile 555, `rename` in Zeile 557, danach Funktionsende).
- **Nächster zulässiger Schritt:** Dateisystem-Semantik (Rename-Durability ohne Verzeichnis-fsync) prüfen; danach Doku-Entscheidung oder Code-Auftrag.
- **Abschlussnachweis:** ausstehend.

### P-12 – Eine Datei je Persistenzlauf vs. eine Datei je Spieler

- **Status:** `ENTSCHEIDUNG OFFEN`
- **Priorität:** `GERING`
- **Betroffener Bereich:** Batch-Format Doku vs. Code (`docs/Player_Persistenz.md` §35, `src/realm-rs/src/spool.rs`)
- **Bekannte Ausgangslage:** Die Doku beschreibt einen gemeinsamen Batch je Lauf ohne getrennte Dateien pro Spieler; der Code schreibt je Spieler-Snapshot eine eigene Datei.
- **Offene Frage / Entscheidung:** Welche Seite normativ ist (Doku an Code oder Code an Doku anpassen).
- **Verifizierte Belege:** `docs/Player_Persistenz.md:974-976` (§35: ein Lauf erzeugt eine Datei mit allen dirty Spielern, bewusst keine getrennten Dateien pro Spieler); `src/realm-rs/src/spool.rs:210-221` (`write_batch`, Dateiname `<captured_at_ms:013>-<player_id>-r<revision>.json`); `src/realm-rs/src/spool.rs:283` („Ein Eintrag je Batch-Datei (V1)“).
- **Nächster zulässiger Schritt:** Architekturentscheidung dokumentieren, danach die unterlegene Seite angleichen.
- **Abschlussnachweis:** ausstehend.

### P-14 – Quarantäne-Retention: automatische Archivierung vs. manuelle Bearbeitung

- **Status:** `ZU PRÜFEN`
- **Priorität:** `MITTEL`
- **Betroffener Bereich:** Quarantäne-Aufbewahrung (`docs/Player_Persistenz.md` §33, `src/realm-rs/src/spool.rs`)
- **Bekannte Ausgangslage:** Die Doku verbietet automatisches Löschen offener Fälle aufgrund des Alters und lässt die 30-Tage-Frist erst mit der Archivierung beginnen; der Code verschiebt offene Dateien älter als 30 Tage automatisch ins Archiv und beschneidet danach das Archiv.
- **Offene Frage / Entscheidung:** Ob das Code-Verhalten die beabsichtigte Umsetzung ist (Doku veraltet) oder die Automatik gegen die Doku verstößt; dabei sind die vorhandenen Retention-Tests als Beleg zu prüfen. Ohne diese Prüfung wird kein endgültiger Fehler festgestellt.
- **Verifizierte Belege:** `docs/Player_Persistenz.md:881-911` (§33: Zeile 893 „NIEMALS aufgrund ihres Alters automatisch gelöscht“, Zeile 903 „ERST BEIM ARCHIVIEREN beginnt die 30-Tage-Aufbewahrungsfrist“); `src/realm-rs/src/spool.rs:39` (`RETENTION_SECS` = 30 Tage); `src/realm-rs/src/spool.rs:475-493` (`run_retention_at`, Verschiebung in Zeile 490); Retention-Tests um `src/realm-rs/src/spool.rs:695-742`.
- **Nächster zulässiger Schritt:** Retention-Tests und Code gegen §33 abgleichen; danach Doku oder Code korrigieren.
- **Abschlussnachweis:** ausstehend.

### P-17 – Dirty-Bit-Zuordnung für einzelne Snapshot-Komponenten

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Auftrag nicht vergeben)
- **Betroffener Bereich:** Dirty-State / Snapshot-Komponenten (`src/realm-rs/src/persist.rs`, `docs/Player_Persistenz.md` §23/§42)
- **Bekannte Ausgangslage:** Das Komponenten-Enum kennt nur fünf Varianten; die konkrete Dirty-Bit-Verteilung für Attribute, HP, Mana, Klasse, Fraktion, Waffenskills und Fähigkeiten ist laut Doku bewusst offen.
- **Offene Frage / Entscheidung:** Welche Komponente welchen Zustand abdeckt und ob die Abdeckung vollständig ist.
- **Verifizierte Belege:** `src/realm-rs/src/persist.rs:36-51` (`PersistComponent` mit Position, Progression, Idia, Inventory, Resources); `src/realm-rs/src/persist.rs:10-19` (Header: §42 lässt die Gewichtung offen); `docs/Player_Persistenz.md:1105` (§42: bewusst offene Punkte).
- **Nächster zulässiger Schritt:** Komponenten-Zuordnung in der Doku festlegen; danach Implementierung und Tests prüfen.
- **Abschlussnachweis:** ausstehend.

### P-18 – Persistenz von Cooldowns (Runtime vs. Snapshot)

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Auftrag nicht vergeben)
- **Betroffener Bereich:** Snapshot-Abgrenzung / Kampf (`src/realm-rs/src/persist.rs`, `src/realm-rs/src/world.rs`, `src/realm-rs/src/combat/ability.rs`)
- **Bekannte Ausgangslage:** Cooldowns sind bewusst nicht Teil des Snapshots; laufende Cooldowns leben als Runtime-Map im Spieler; in den Ability-Definitionen existiert ein statisches Persistenz-Kennzeichen.
- **Offene Frage / Entscheidung:** Welche Cooldown-Zustände einen Logout überdauern müssen und wo sie persistiert werden.
- **Verifizierte Belege:** `src/realm-rs/src/persist.rs:15-19` (Header: Cooldowns bewusst nicht Teil des Snapshots); `src/realm-rs/src/world.rs:77` (`cooldowns: BTreeMap`); `src/realm-rs/src/combat/ability.rs:36,92,114` (`cooldown_persistent`); `docs/Player_Persistenz.md:490` (§23: Snapshot-Inhalt).
- **Nächster zulässiger Schritt:** Cooldown-Semantik über Tod/Logout in der Doku klären; danach ggf. Code-Test beauftragen.
- **Abschlussnachweis:** ausstehend.

### P-22 – Recovery-Limit: `Ok` bzw. READY trotz Rest-Batches

- **Status:** `ZU PRÜFEN`
- **Priorität:** `GERING`
- **Betroffener Bereich:** Startup-Recovery (`src/realm-rs/src/spool.rs`, `src/realm-rs/src/main.rs`)
- **Bekannte Ausgangslage:** Die Recovery-Schleife ist auf 10.000 Iterationen begrenzt und gibt danach `Ok` zurück, auch wenn Batches übrig sind; der Aufrufer setzt bei `Ok` den Status READY.
- **Offene Frage / Entscheidung:** Ob das Limit als Notventil mit READY-Status beabsichtigt ist oder bei Restarbeit ein anderer Status (z. B. DEGRADED mit Hinweis) gesetzt werden muss.
- **Verifizierte Belege:** `src/realm-rs/src/spool.rs:207-218` (`recover`, Schleifenbedingung mit `guard < 10_000` in Zeile 210, `Ok(total)` in Zeile 217); `src/realm-rs/src/main.rs:93-105` (`Ok`-Zweig setzt `PersistStatus::Ready`).
- **Nächster zulässiger Schritt:** Fachliche Entscheidung zur Status-Semantik nach Erreichen des Limits; danach ggf. Behandlung und Tests.
- **Abschlussnachweis:** ausstehend.

### P-23 – Monitoring/Admin erst nach der Recovery verfügbar

- **Status:** `ZU PRÜFEN`
- **Priorität:** `GERING`
- **Betroffener Bereich:** Startreihenfolge / Monitoring (`src/realm-rs/src/main.rs`, `docs/Player_Persistenz.md` §28)
- **Bekannte Ausgangslage:** Der Health-Server wird erst nach der Startup-Recovery gestartet; die Doku verlangt Verfügbarkeit von Monitoring/Administration auch während RECOVERING; die Runtime startet im Status Recovering.
- **Offene Frage / Entscheidung:** Ob der Health-Server vor die Recovery gezogen wird oder die Doku angepasst wird.
- **Verifizierte Belege:** `src/realm-rs/src/main.rs:93-113` (Recovery vor `health_task`); `src/realm-rs/src/spool.rs:95` (Startstatus `Recovering`); `docs/Player_Persistenz.md:718` (§28) mit Zeile 747 (Verfügbarkeit während RECOVERING).
- **Nächster zulässiger Schritt:** Entscheidung zur Startreihenfolge; danach ggf. Änderung mit Tests.
- **Abschlussnachweis:** ausstehend.

### P-26 – Verhalten bei beidseitigem Ausfall (DB und Spool)

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Auftrag nicht vergeben)
- **Betroffener Bereich:** Drain-Fehler / Shutdown (`src/realm-rs/src/main.rs`, `src/realm-rs/src/spool.rs`, `docs/Player_Persistenz.md` §26/§41)
- **Bekannte Ausgangslage:** Drain-Fehler setzen den Status DEGRADED; bei DB-Fehler bleibt der Batch erhalten und der Drain bricht ab; die Doku beschreibt den beidseitigen Ausfall beim Shutdown.
- **Offene Frage / Entscheidung:** Ob alle Shutdown-Pfade (Spool-Write und DB-Drain) die Doku-Vorgabe erfüllen, dass ein beidseitiger Fehlschlag nie als Erfolg gemeldet wird.
- **Verifizierte Belege:** `src/realm-rs/src/main.rs:377` (Drain-Fehler → `Degraded`); `src/realm-rs/src/spool.rs:124` (Fehlerpfad setzt `Degraded`); `docs/Player_Persistenz.md:1090` (§41: beidseitiger Ausfall).
- **Nächster zulässiger Schritt:** Shutdown-Pfade gegen §41 in einer read-only Prüfung abgleichen; nach der fachlichen Entscheidung können Dokumentation, Code und/oder Tests betroffen sein.
- **Abschlussnachweis:** ausstehend.

### P-27 – Fehler-Semantik des direkten `logout_at`-Schreibens

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Auftrag nicht vergeben)
- **Betroffener Bereich:** Disconnect / Shutdown (`src/realm-rs/src/db.rs`, `src/realm-rs/src/main.rs`, `docs/Player_Persistenz.md` §11)
- **Bekannte Ausgangslage:** Der Logout-Zeitpunkt wird per direktem DB-Update geschrieben; im Shutdown wird ein Fehler nur protokolliert und mit dem nächsten Spieler fortgefahren.
- **Offene Frage / Entscheidung:** Welche Fehler-Semantik für das direkte Schreiben des Logout-Zeitpunkts nach Disconnect verbindlich gelten soll; ob die aktuelle Semantik (bloßes Protokollieren ohne weitere Maßnahme) fachlich akzeptabel ist.
- **Verifizierte Belege:** `src/realm-rs/src/db.rs:399-405` (`write_logout_at`, direktes `UPDATE characters SET logout_at`); `src/realm-rs/src/main.rs:424` (Fehler wird nur geloggt); `docs/Player_Persistenz.md:226` (§11: Disconnect).
- **Nächster zulässiger Schritt:** Fachliche Entscheidung zur Fehler-Semantik im Zuge einer Doku-Prüfung; danach können je nach Entscheidung Doku, Code und/oder Tests betroffen sein.
- **Abschlussnachweis:** ausstehend.

### P-28 – Start als DEGRADED nach Recovery-DB-Fehler

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Auftrag nicht vergeben)
- **Betroffener Bereich:** Serverstart / Verfügbarkeit (`src/realm-rs/src/main.rs`, `src/realm-rs/src/handlers.rs`, `docs/Player_Persistenz.md` §28)
- **Bekannte Ausgangslage:** Bei Recovery-Fehler startet der Realm als DEGRADED und der Start bricht nicht ab; Logins bleiben in DEGRADED möglich, während RECOVERING blockiert; die Doku beschreibt den RECOVERING-Pfad mit zunächst blockierten Logins.
- **Offene Frage / Entscheidung:** Ob der dokumentierte RECOVERING-Pfad den Start-DB-Fehler abdeckt oder Code/Doku zum DEGRADED-Start angeglichen werden müssen.
- **Verifizierte Belege:** `src/realm-rs/src/main.rs:108-109` (Recovery-Fehler → DEGRADED, Start läuft weiter); `src/realm-rs/src/handlers.rs:116-122` (Recovering blockiert Login, DEGRADED lässt Logins zu); `docs/Player_Persistenz.md:718` (§28: Start und Recovery).
- **Nächster zulässiger Schritt:** Doku und Code auf Konsistenz prüfen; DEGRADED-Semantik dokumentieren.
- **Abschlussnachweis:** ausstehend.

### P-29 – Cleanup der `in_flight`-Gates

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Auftrag nicht vergeben)
- **Betroffener Bereich:** Nebenläufigkeit / Persistenz (`src/realm-rs/src/spool.rs`, `src/realm-rs/src/persist.rs`)
- **Bekannte Ausgangslage:** Pro Spieler wird ein Serialisierungs-Gate in einer Map angelegt; eine Entfernung der Einträge ist nicht nachgewiesen.
- **Offene Frage / Entscheidung:** Wann Gates entfernt werden (Erfolg/Fehler) und ob ein Leck über die Prozesslaufzeit besteht.
- **Verifizierte Belege:** `src/realm-rs/src/spool.rs:74` (`in_flight`-Map); `src/realm-rs/src/persist.rs:256-257` (Gate-Erwerb in `persist_player`) und `src/realm-rs/src/spool.rs:95-102` (Anlage per `or_insert_with` in `player_gate`); keine Entfernung im Repository nachweisbar.
- **Nächster zulässiger Schritt:** Codepfade (Erfolg/Fehler von `persist_player`) nachverfolgen; danach Tests für das Cleanup ergänzen oder beauftragen.
- **Abschlussnachweis:** ausstehend.

### P-30 – Quarantänisierter Batch ohne DB-Write: Login kann eine ältere DB-Zeile laden

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Auftrag nicht vergeben)
- **Betroffener Bereich:** Drain-Quarantäne und Login-Vorabprüfung (`src/realm-rs/src/spool.rs`, `src/realm-rs/src/handlers.rs`, `docs/Player_Persistenz.md` §33)
- **Bekannte Ausgangslage:** Ein Batch, der beim Drain nicht anwendbar ist (unlesbar, fehlerhaft, unbekanntes Format, unbekannter Charakter), wird über `quarantine` aus `spool/` nach `quarantine/open/` verschoben, ohne dass ein DB-Write stattfindet. Die Login-Vorabprüfung wertet ausschließlich offene Dateien in `spool/` aus; ein quarantänisierter Batch gilt ihr deshalb nicht als ausstehend. Ein folgender Login kann damit eine DB-Zeile laden, die älter ist als der zuletzt autoritative Snapshot; der Batch bleibt als offener Fall in `quarantine/open/` liegen und wird nicht angewendet.
- **Offene Frage / Entscheidung:** Wie ist mit einem quarantänisierten Batch umzugehen: Login blockieren, den Account markieren oder einen kontrollierten Operator-/Recovery-Ablauf definieren. Eine Lösung und eine Priorität werden hier nicht festgelegt.
- **Verifizierte Belege:** `src/realm-rs/src/spool.rs:443-464` (`quarantine`: `move_file` nach `quarantine/open/`, sonst `remove_file`); `src/realm-rs/src/spool.rs:353`, `:362`, `:371`, `:391` (Auslöser Unreadable, Malformed, UnknownFormat, UnknownCharacter — jeweils vor jedem DB-Write); `src/realm-rs/src/spool.rs:281-301` (`pending_revision` liest ausschließlich `spool/`); `src/realm-rs/src/handlers.rs:171-181` (Fail-closed-Auswertung über `pending_revision`); `docs/Player_Persistenz.md:881-909` (§33: offene Fälle, „NIEMALS aufgrund ihres Alters automatisch gelöscht“, Verschiebung ins Archiv erst nach Bearbeitung). Abgrenzung: `P-14` betrifft die Aufbewahrung/Retention der Quarantänefälle, `P-22` das Recovery-Limit, `P-26` den beidseitigen Ausfall von DB und Spool; keiner davon den Login-Zustand bei einem quarantänisierten Batch.
- **Nächster zulässiger Schritt:** Fachliche Entscheidung zum Umgang mit quarantänisierten Batches im Login-Pfad; danach sind je nach Entscheidung Dokumentation, Code und/oder Tests betroffen. Bis dahin keine Änderung am Quarantänepfad.
- **Abschlussnachweis:** ausstehend.

## 4. Noch ausstehendes Sicherheits-Audit

Ein eigenständiges Sicherheits-Audit wurde in dieser Aufgabe nicht durchgeführt und steht als Folgeauftrag aus. Die unten aufgeführten Punkte sind ausstehende, read-only Prüfungen dieser Folgeaufträge.

**Rahmen des ersten Audits:**

- Der erste Audit ist ein read-only Repository-Audit: Es werden vorhandener Code, vorhandene Doku, vorhandene Tests und Tool-Auswertungen eingesehen; Produktionscode wird dabei nicht geändert.
- Vorhandene Tests dürfen ausgeführt werden, um vorhandene Nachweise zu erbringen.
- Es werden keine neuen Tests geschrieben.
- Es erfolgen keine externen Netzwerk-, Last- oder Penetrationstests.
- Solche dynamischen Tests benötigen einen eigenen späteren Auftrag und sind von diesem read-only Audit getrennt.

### 4.1 Authentifizierung und Session-Validierung

- **Status:** `ERLEDIGT` (nur der read-only Auditlauf; die unten dokumentierten offenen Befunde und Entscheidungen sind nicht erledigt)
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Realm-Auth, Login/HELLO (`src/realm-rs/src/handlers.rs`, `src/realm-rs/src/net.rs`, `src/realm-rs/src/security.rs`)
- **Bekannte Ausgangslage:** Die HELLO-Prüfung blockiert Login im Status `Recovering` (`src/realm-rs/src/handlers.rs:116-122`); nicht authentifizierte Frames werden vor der Spiellogik verworfen (`src/realm-rs/src/security.rs:242-263`, Test `unauthenticated_requests_dropped_before_logic` in `src/realm-rs/src/security.rs:695-707`).
- **Auditergebnis:** Der read-only Auditlauf wurde am 25.09.2026 abgeschlossen. Die bestehenden Rust-Tests ergaben 416 bestandene und 0 fehlgeschlagene Tests; `go test ./...` im Modul `src/api` war für `andora/authapi` erfolgreich. `git status --short` hatte nach dem Audit eine leere Ausgabe; der Arbeitsbaum war sauber. Der Auditlauf ist damit abgeschlossen, die nachfolgenden offenen Befunde und Entscheidungen sind jedoch nicht erledigt.
- **Verifizierte Belege:** `src/realm-rs/src/handlers.rs:116-122`, `src/realm-rs/src/security.rs:242-263`, Test `unauthenticated_requests_dropped_before_logic` in `src/realm-rs/src/security.rs:695-707`; Testläufe `cargo test --manifest-path src/realm-rs/Cargo.toml` (416 bestanden, 0 fehlgeschlagen) und `go test ./...` in `src/api` (`ok andora/authapi`); `git status --short` mit leerer Ausgabe.
- **Nächster zulässiger Schritt:** Die nachfolgenden Einträge getrennt nach ihrem jeweiligen Status bearbeiten; aus dem abgeschlossenen Auditlauf folgt keine Aussage, dass der geprüfte Bereich insgesamt sicher abgeschlossen ist.
- **Abschlussnachweis:** Auditlauf am 25.09.2026 abgeschlossen; offene Befunde und Entscheidungen bleiben ausstehend.

#### AUTH-01 – Fail-closed HELLO-Einstieg

- **Status:** `BESTÄTIGT` (Schutzmechanismus; kein Schwachstellenbefund)
- **Priorität:** keine Risikostufe, da kein Befund
- **Betroffener Bereich:** Realm-Auth und Charakterladen (`src/realm-rs/src/handlers.rs`, `src/realm-rs/src/auth_api.rs`, `src/realm-rs/src/db.rs`, `src/api`)
- **Bekannte Ausgangslage:** Bei aktivierter Auth-API ist HELLO fail-closed: Der Einstieg erfordert einen one-shot, an den Ziel-Realm gebundenen Handoff sowie eine gültige Session mit identischer `account_id`. Das Charakterladen bindet `char_id` zusätzlich an dieselbe `account_id`.
- **Offene Frage / Entscheidung:** Keine; der geprüfte Pfad wird als bestätigter Schutzmechanismus und nicht als offene Aufgabe geführt.
- **Verifizierte Belege:** `src/realm-rs/src/handlers.rs:59-92` (`verify_entry`), Aufruf in `src/realm-rs/src/handlers.rs:138`; `src/realm-rs/src/auth_api.rs:165-172`; `src/api/store.go:655-668`, `src/api/endpoints.go:516-546`, `src/api/store.go:557-574`; Ownership-Filter in `src/realm-rs/src/db.rs:165-179` (`WHERE id = ? AND account_id = ?`).
- **Nächster zulässiger Schritt:** Keiner; kein offener Befund.
- **Abschlussnachweis:** Schutzmechanismus durch Code, vorhandene Tests und den Auditlauf vom 25.09.2026 bestätigt.

#### AUTH-02a – Semantik ablaufender Sessions während bereits autorisierter Realm-Verbindungen

- **Status:** `ENTSCHEIDUNG OFFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Session-Lebenszyklus nach erfolgreichem HELLO (`src/realm-rs/src/handlers.rs`, `src/realm-rs/src/security.rs`, `src/realm-rs/src/world.rs`, `src/api`)
- **Bekannte Ausgangslage:** Die Session wird beim HELLO validiert und anschließend als `session_id` am Player gespeichert. Während einer bereits autorisierten Verbindung prüft `gate_frame` nur den Authentifizierungsstatus der Verbindung; auch `handle_heartbeat` validiert die Session nicht erneut. Die Auth-API kennt einen Ablaufzeitpunkt und eine Session-TTL. Daraus wird keine bestätigte Schwachstelle abgeleitet, weil die gewünschte Lebenszyklus-Semantik noch nicht dokumentiert ist.
- **Offene Frage / Entscheidung:** Soll der reguläre Ablauf einer Session während einer bereits autorisierten Realm-Verbindung die Verbindung beenden, oder gilt die beim HELLO erteilte Autorisierung bis zum Verbindungsende?
- **Verifizierte Belege:** `src/realm-rs/src/handlers.rs:84` (`validate_session` beim Einstieg), `src/realm-rs/src/handlers.rs:268` und `src/realm-rs/src/world.rs:34` (`session_id` am Player), `src/realm-rs/src/security.rs:242-263` (`gate_frame`), `src/realm-rs/src/handlers.rs:1167-1188` (`handle_heartbeat`), `src/api/store.go:557-574` (`expires_at`), `src/api/config.go:174` (`SESSION_TTL_MINUTES`).
- **Nächster zulässiger Schritt:** Gewünschte Lebenszyklus-Semantik fachlich dokumentieren; erst danach gegebenenfalls einen Code-/Test-Auftrag ableiten.
- **Abschlussnachweis:** ausstehend.

#### AUTH-02b – Propagation ausdrücklicher Session-Widerrufe auf bereits aktive Realm-Verbindungen

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Expliziter Session-Widerruf und aktive Realm-Verbindungen (`src/realm-rs/src/handlers.rs`, `src/realm-rs/src/parental.rs`, `src/api/store.go`)
- **Bekannte Ausgangslage:** Die Auth-API unterstützt ausdrücklichen Session-Widerruf, unter anderem im Zusammenhang mit Passwortänderung oder Passwort-Reset. Der Realm validiert die Session nach dem HELLO nicht erneut; der bestehende `force_logout`-Pfad gehört zur Elternkontrolle und belegt keine allgemeine Propagation ausdrücklicher Session-Widerrufe. Ablauf und ausdrücklicher Widerruf werden getrennt bewertet.
- **Offene Frage / Entscheidung:** Müssen ausdrückliche Session-Widerrufe bereits aktive Realm-Verbindungen beenden, und existiert dafür ein noch nicht nachgewiesener Propagationspfad?
- **Verifizierte Belege:** `src/api/store.go:576-585` (`RevokeSession`), `src/api/store.go:816-832` und `src/api/store.go:863-867` (Session-Widerruf bei Passwortänderung beziehungsweise Passwort-Reset), `src/realm-rs/src/handlers.rs:84` (Session-Validierung beim Einstieg), `src/realm-rs/src/parental.rs:309-311` und `src/realm-rs/src/parental.rs:319-327` (`force_logout`-Kickerpfad), `src/realm-rs/src/parental.rs:330-338` (10-Sekunden-Poller).
- **Nächster zulässiger Schritt:** Read-only prüfen und fachlich entscheiden, ob und wie ausdrückliche Widerrufe auf aktive Realm-Verbindungen propagiert werden müssen; danach gegebenenfalls Code-/Test-Auftrag.
- **Abschlussnachweis:** ausstehend.

#### AUTH-03 – Keine Occupancy-/Einzigkeitsprüfung paralleler Verbindungen desselben Charakters

- **Status:** `ERLEDIGT` für Verbindungs-Einzigkeit, Takeover und Cleanup (Befund bestätigt, Entscheidung umgesetzt, Commit `e499dec8d9e9db471f0a3ecb222b1fd869bf643c`, Abschlussnachweis in Abschnitt 2). Die Logging-Anforderung dieses Eintrags ist nur teilweise umgesetzt und bleibt unten ausdrücklich offen.
- **Priorität:** `MITTEL`
- **Betroffener Bereich:** Spielerregistrierung, Verbindungszuordnung und Verbindungsende (`src/realm-rs/src/handlers.rs`, `net.rs`, `world.rs`, `parental.rs`, `spool.rs`, `security.rs`)
- **Bekannte Ausgangslage:** Vor Commit `e499dec` wurden Player- und Verbindungszuordnung beim HELLO ohne vorherige Occupancy-/Einzigkeitsprüfung eingefügt, und `disconnect_player` entfernte anschließend sämtliche `by_conn`-Zuordnungen desselben Charakters; damit konnte ein fremdes, verzögertes Cleanup den neuen Eigentümer mit entfernen. Dieser historische Befund ist mit dem Commit behoben; die damalige Belegstellung ist unten durch aktuelle Belegstellen ersetzt.
- **Offene Frage / Entscheidung:** Fachlich entschieden: Pro Charakter darf höchstens eine aktive, zur Spiellogik berechtigte Realm-Verbindung existieren. Eine neue vollständig authentifizierte Verbindung übernimmt; die alte Verbindung wird vor der Übergabe entmachtet und anschließend getrennt. Die verbindliche Single-Connection-/Takeover-Semantik steht in `docs/Login_Realm_Architektur.md` (Abschnitt „Verbindungs-Einzigkeit und Takeover“). Die Einordnung der Quell-IPs als Sicherheitssignale steht in `docs/netzwerk_ip_schutz.md`; Zweckbindung, Zugriffsbegrenzung und Löschung der Takeover-IP-Daten stehen in `docs/datenschutz_zugang.md`.
- **Logging-Anforderung:** Ein einzelner Takeover ist das INFO-Ereignis `authenticated_connection_takeover`; auffällige Wiederholungen dürfen nur einen WARN-/Alarmhinweis ohne automatische Sanktion erzeugen. Vollständige Session-IDs, Handoff-Tokens, Passwörter und andere Zugangsdaten werden nicht geloggt. Roh-IP-Adressen und mit Accounts verknüpfte IP-Daten dieser Ereignisse werden nach 14 Tagen automatisch gelöscht. Umgesetzt sind Ereignisname, Account-ID, Charakter-ID, alte und neue `conn_id` sowie der Zustand der verdrängten Verbindung; eine Roh-IP wird bewusst nicht ausgegeben, weil dafür noch kein freigegebener Log-Sink mit Löschfrist existiert. Der Teil „Roh-IP-Protokollierung mit 14-Tage-Löschung“ bleibt damit offen und ist durch diesen Abschluss nicht erledigt.
- **Verifizierte Belege:** `src/realm-rs/src/world.rs:304-375` (`commit_login` als einziger Eintrag in `by_conn`, Takeover entfernt alt vor neu); `src/realm-rs/src/handlers.rs:327-365` (Commit, Takeover-Log, Signal an den vorhandenen Closer); `src/realm-rs/src/world.rs:597-612` (`disconnect_conn` ohne charakterweites `retain`); `src/realm-rs/src/net.rs:316-374` (Eigentümerprüfungen und Gate); `src/realm-rs/src/security.rs:170-189` (Takeover-Logzeile ohne Roh-IP); `src/realm-rs/src/spool.rs:95-102` (per-player-Gate), `src/realm-rs/src/spool.rs:281-301` (`pending_revision`).
- **Nächster zulässiger Schritt:** Die dauerhafte Takeover-IP-Protokollierung mit 14-Tage-Löschung bleibt der nächste Schritt dieses Eintrags; dafür ist zuerst ein freigegebener, zugriffsgeschützter Log-Sink mit Rotation und Löschung fachlich festzulegen. Der vollständige Logging-Audit (Abschnitt 4.5) ist davon nicht abgedeckt. Ein dynamischer Ausnutzungsnachweis wird nicht behauptet.
- **Abschlussnachweis:** Statischer Codebefund bestätigt, fachliche Entscheidung in den drei zuständigen Fachdokumenten festgehalten, Implementierung und Tests über Commit `e499dec8d9e9db471f0a3ecb222b1fd869bf643c` abgeschlossen (436 Tests bestanden, 0 fehlgeschlagen); Logging-Anforderung teilweise offen, siehe oben.

#### AUTH-04 – Leere `AUTHAPI_URL` aktiviert den Dev-Modus ohne Auth

- **Status:** `ZU PRÜFEN`
- **Priorität:** `MITTEL`
- **Betroffener Bereich:** Konfiguration der Auth-Anbindung (`src/realm-rs/src/auth_api.rs`, `src/realm-rs/src/handlers.rs`, `src/realm-rs/src/parental.rs`, `src/realm-rs/src/config.rs`, `src/realm-rs/config.env.example`)
- **Bekannte Ausgangslage:** Bei leerer `AUTHAPI_URL` ist die Auth-API deaktiviert; `verify_entry` gibt ohne Auth-Prüfung `account_id = 0` zurück, und der Parental-Attach wird bei `account_id = 0` übersprungen. Dieser Modus ist ausdrücklich für Entwicklung/Testprototypen dokumentiert. Eine produktive Fehlkonfiguration ist durch das Repository nicht belegt.
- **Offene Frage / Entscheidung:** Wird in produktiven Deployments verbindlich verhindert, dass der Realm mit leerer `AUTHAPI_URL` startet?
- **Verifizierte Belege:** `src/realm-rs/src/auth_api.rs:113-115` (`enabled`), `src/realm-rs/src/handlers.rs:65-67` (`Ok(0)`), `src/realm-rs/src/parental.rs:109-111` (Parental-Attach übersprungen), `src/realm-rs/src/config.rs:155-161` (bewusster Dev-/Testmodus), `src/realm-rs/src/config.rs:577-581` (Laden der Auth-API-Konfiguration), `src/realm-rs/config.env.example`.
- **Nächster zulässiger Schritt:** In einem getrennten Deployment-/Konfigurationsaudit prüfen, ob `AUTHAPI_URL` für Produktionsprofile verbindlich gesetzt sein muss und erzwungen wird.
- **Abschlussnachweis:** ausstehend; der Dev-Modus ist bestätigt, eine produktive Fehlkonfiguration nicht.

#### AUTH-05 – Vollständige Session-ID in `sec-reject`-Logs

- **Status:** `ERLEDIGT` (Befund bestätigt, Entscheidung umgesetzt, Commit `e499dec8d9e9db471f0a3ecb222b1fd869bf643c`)
- **Priorität:** `GERING`
- **Betroffener Bereich:** Ablehnungs-Logging (`src/realm-rs/src/security.rs`), fachlicher Bezug zu §4.5 „Logging und Schutz sensibler Daten“
- **Bekannte Ausgangslage:** Vor Commit `e499dec` schrieb `log_reject` die vollständige `session_id` in die Warn-Logzeile; die vollständige Session-ID war dort weder für die Korrelation belegt noch festgelegt. Seit diesem Commit wird die Session-ID in diesem Logformat nicht mehr ausgegeben; der Player-Zustand (Position, Ressourcen, Level, Erfahrung, Gold, freie Attributpunkte) wird unverändert erfasst.
- **Offene Frage / Entscheidung:** Entschieden und umgesetzt: In der `sec-reject`-Zeile wird **keine** Session-Darstellung verwendet — kein Hash, kein Präfix, keine interne Korrelations-ID. Die im Befund genannten Alternativen waren eine Liste zulässiger Optionen, keine Pflicht zur Einführung eines Ersatzidentifikators. Der verbleibende Umgang mit anderen Logdaten ist weiterhin Gegenstand des offenen Audit-Punkts in Abschnitt 4.5 und nicht Teil dieses Eintrags.
- **Verifizierte Belege:** `src/realm-rs/src/security.rs:134-148` (`reject_log_line` ohne Session-Feld), `src/realm-rs/src/security.rs:158-160` (`log_reject` schreibt ausschließlich diese Zeile), `src/realm-rs/src/net.rs:402-419` (Aufruf im V1-Gate für `GateDecision::Drop`); Test `reject_log_line_contains_no_session_id` in `src/realm-rs/src/security.rs:719-736` (prüft Zeile und die Felder `sess-1`/`session=`); Commit `e499dec8d9e9db471f0a3ecb222b1fd869bf643c`.
- **Nächster zulässiger Schritt:** Für diesen Eintrag nichts offen. Der Abschnitt 4.5 bleibt als read-only Prüfungsgebiet bestehen und ist nicht durch diesen Abschluss abgedeckt.
- **Abschlussnachweis:** Befund bestätigt, Entscheidung ohne Ersatzidentifikator umgesetzt und im Repository getestet; `cargo test` in `src/realm-rs` mit 436 bestandenen Tests über den genannten Commit.

#### AUTH-06 – Kein natives TLS im Realm; Deployment-TLS nicht nachgewiesen

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Realm-Transport und Deployment (`src/realm-rs/src/net.rs`, `src/realm-rs/src/health.rs`, `src/realm-rs/src/config.rs`, `docs/Projekt-Status.md`)
- **Bekannte Ausgangslage:** Im Realm-Code wurde kein natives TLS für den Spieler- oder Health-Transport nachgewiesen. Daraus folgt keine Aussage, dass der produktive Transport sicher unverschlüsselt ist: Eine TLS-Terminierung durch Reverse-Proxy oder andere Deployment-Komponenten wurde in diesem read-only Repository-Audit nicht geprüft.
- **Offene Frage / Entscheidung:** Erfolgt die TLS-Terminierung in produktiven Deployments nativ im Realm oder außerhalb des Realm-Prozesses, insbesondere durch einen Reverse-Proxy?
- **Verifizierte Belege:** Repository-Suche `grep -rni "tls\|ssl\|wss" src/realm-rs/src` mit dem Treffer `src/realm-rs/src/config.rs:23` („ohne TLS“ zur MariaDB-DSN); `docs/Projekt-Status.md:26` (mTLS-Vorbereitung für den Andora-Agent, kein Nachweis für den Realm-Spielertransport).
- **Nächster zulässiger Schritt:** Deployment-/Betriebsarchitektur read-only auf eine vorgeschaltete TLS-Terminierung prüfen; danach gegebenenfalls Doku- oder Implementierungsauftrag.
- **Abschlussnachweis:** ausstehend; Reverse-Proxy-Terminierung ungeprüft.

#### AUTH-07 – Unauthentifizierter `/players`-Endpoint

- **Status:** `BESTÄTIGT` (bezogen auf den Endpoint; externe Erreichbarkeit ungeprüft)
- **Priorität:** `MITTEL`
- **Betroffener Bereich:** Health-/Status-HTTP (`src/realm-rs/src/health.rs`)
- **Bekannte Ausgangslage:** Der HTTP-Endpoint `/players` liefert Player-Daten ohne Authentifizierung. Die externe Erreichbarkeit des Endpoints hängt von der Laufzeit-/Netzwerkkonfiguration ab und wurde in diesem read-only Repository-Audit nicht geprüft.
- **Offene Frage / Entscheidung:** Soll `/players` auf interne Erreichbarkeit beschränkt oder mit einer Authentifizierung versehen werden?
- **Verifizierte Belege:** `src/realm-rs/src/health.rs:13-45` (Routing ohne Auth-Prüfung), `src/realm-rs/src/health.rs:61-150` (`/players` und Antwortaufbereitung).
- **Nächster zulässiger Schritt:** Externe Erreichbarkeit getrennt prüfen und eine Access-Control-Entscheidung dokumentieren; danach gegebenenfalls Code-/Test-Auftrag.
- **Abschlussnachweis:** Endpoint-Befund bestätigt; externe Erreichbarkeit ungeprüft.

### 4.2 Replay-Schutz

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Sequenz-/Replay-Prüfung (`src/realm-rs/src/security.rs`)
- **Bekannte Ausgangslage:** Die Gate-Reihenfolge prüft die Sequenz; der Test `seq_duplicates_are_tolerated` zeigt, dass Duplikate toleriert werden (`src/realm-rs/src/security.rs:666`).
- **Offene Frage / Entscheidung:** Ob die tolerierten Duplikate Replay- oder Wiederholungsattacken ermöglichen und was die verbindliche Replay-Semantik sein soll.
- **Verifizierte Belege:** `src/realm-rs/src/security.rs:205-224` (Gate-Reihenfolge Größe → Format → Session → Sequenz → Rate), `src/realm-rs/src/security.rs:666`.
- **Nächster zulässiger Schritt:** Read-only Prüfung der Sequenz-Logik im Folgeauftrag; danach Doku-Entscheidung oder Code-/Test-Auftrag.
- **Abschlussnachweis:** ausstehend.

### 4.3 Protokoll- und Nachrichten-Gating

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Frame-Handling vor der Spiellogik (`src/realm-rs/src/security.rs`, `src/realm-rs/src/net.rs`)
- **Bekannte Ausgangslage:** `gate_frame` prüft in der Reihenfolge Größe → Format (JSON) → Session → Sequenz → Rate-Limit, bevor Game-Logic/DB erreicht werden; Frames über der Größenlimite werden vor dem Parse verworfen (`frame_too_large`, Test `oversize_frames_rejected_before_parse`); ungültige Frames erreichen keine teuren Systeme (`invalid_requests_never_reach_expensive_systems`).
- **Offene Frage / Entscheidung:** Vollständigkeit (alle Nachrichtentypen, alle Pfade) und Konsistenz der Gate-Reihenfolge mit der Doku.
- **Verifizierte Belege:** `src/realm-rs/src/security.rs:205-224` (Kopfkommentar Reihenfolge, Funktion `gate_frame`), Tests `oversize_frames_rejected_before_parse` (`src/realm-rs/src/security.rs:658`) und `invalid_requests_never_reach_expensive_systems` (`src/realm-rs/src/security.rs:814`); Doku `docs/Serverautoritaet_und_Anti-Manipulation_V1.md`.
- **Nächster zulässiger Schritt:** Read-only Gegenprüfung Code/Doku im Folgeauftrag; Befunde hier vermerken.
- **Abschlussnachweis:** ausstehend.

### 4.4 Rate-Limits und vorhandene Testnachweise

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Rate-Limits pro Verbindung (`src/realm-rs/src/security.rs`, `src/realm-rs/src/config.rs`)
- **Bekannte Ausgangslage:** Gestaffelte Rate-Limits existieren (`check_rate` in `src/realm-rs/src/security.rs:167`); vorhandene Tests: `flood_of_rare_requests_is_capped` (`src/realm-rs/src/security.rs:576`), `movement_limit_exceeds_rare_limit` (`src/realm-rs/src/security.rs:598`), `repeated_violations_disconnect` (`src/realm-rs/src/security.rs:616`). Die Werte und die Verbands-/Disconnect-Semantik sind im Configuration-Modul hinterlegt (`src/realm-rs/src/config.rs`).
- **Offene Frage / Entscheidung:** Angemessenheit der Schwellwerte und der Disconnect-Auswirkung; Vollständigkeit der Testabdeckung aller gestaffelten Klassen.
- **Verifizierte Belege:** `src/realm-rs/src/security.rs:167` (`check_rate`), die drei genannten Tests, `src/realm-rs/src/config.rs` (SecurityCfg).
- **Nächster zulässiger Schritt:** Read-only Prüfung der Schwellwerte und Testauswertung im Folgeauftrag; Tests dürfen dabei ausgeführt werden, neue Tests werden nicht geschrieben.
- **Abschlussnachweis:** ausstehend.

### 4.5 Logging und Schutz sensibler Daten

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Protokollierung (`src/realm-rs/src/*.rs`, `docs/chat_system.md`)
- **Bekannte Ausgangslage:** Protokollierung existiert (z. B. `log::info!`/`log::error!` in `src/realm-rs/src/main.rs:108`, `src/realm-rs/src/main.rs:128-`); Chat-/Voice-Logging ist in `docs/chat_system.md` als Regelbereich dokumentiert; ob Logdaten geheime/sensible Inhalte enthalten oder wie sie geschützt werden, ist noch nicht geprüfter Befund.
- **Vorhandene Teilregel:** Die beschlossenen Takeover-Logging-Regeln sind bereits in `docs/datenschutz_zugang.md` dokumentiert; der vollständige Logging-Audit wurde dadurch nicht durchgeführt.
- **Offene Frage / Entscheidung:** Welche Daten in Logs landen, welche davon sensibel sind und ob die Aufzeichnung/Retention datenschutzkonform ist.
- **Verifizierte Belege:** `src/realm-rs/src/main.rs:108` (Log-Aufruf), `docs/chat_system.md` (Chat-Logging-Regeln laut Inhaltsverzeichnis-Eintrag in `docs/README.md`).
- **Nächster zulässiger Schritt:** Read-only Code- und Doku-Auswertung im Folgeauftrag; Ergebnis als Befund/Abweichung erfassen.
- **Abschlussnachweis:** ausstehend.

### 4.6 Persistenz, Spool und Recovery

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Stufe-B-Persistenz (`src/realm-rs/src/spool.rs`, `src/realm-rs/src/persist.rs`, `src/realm-rs/src/main.rs`, `docs/Player_Persistenz.md`)
- **Bekannte Ausgangslage:** Die persistenzspezifischen offenen Punkte sind bereits einzeln in Abschnitt 3 geführt; dieser Audit-Punkt dupliziert sie nicht, sondern fasst sie als read-only Prüfungsgebiet zusammen.
- **Offene Frage / Entscheidung:** Zusammenführung der schon erkannten Befunde nach dem Folge-Audit; welche Punkte den Status `BESTÄTIGT` oder `ERLEDIGT` erreichen.
- **Verifizierte Belege:** Abschnitt 3 dieser Datei mit den dort je genannten Belegstellen.
- **Nächster zulässiger Schritt:** Read-only Abgleich der bereits erkannten Punkte im Folgeauftrag; Statusänderungen nur nach Beleg.
- **Abschlussnachweis:** ausstehend.

### 4.7 Zustandsintegrität von Handel, Auktion, Loot, Crafting, Gold, EXP und Inventar

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Serverautorität über Zustandsänderungen (`src/realm-rs/src/security.rs`, `src/realm-rs/src/handlers.rs` sowie Fachdokumente `docs/Lootsystem.md`, `docs/Crafting.md`, `docs/Erfahrung_und_Progressionssystem.md`, `docs/inventory_system.md`, `docs/Auktionshaus und Marktplatz`)
- **Bekannte Ausgangslage:** Anti-Manipulation V1 prüft Auktionskäufe aus Serverwerten (fail-closed-Stub ohne AH-State) und verwirft manipulierte Attribute-/HP-/Goldwerte; vorhandene Tests: `manipulated_attribute_value_is_ignored` (`src/realm-rs/src/security.rs:476`), `manipulated_gold_does_not_buy` (`src/realm-rs/src/security.rs:534`), `affordable_auction_passes_with_server_price` (`src/realm-rs/src/security.rs:545`), `invalid_auction_buys_rejected` (`src/realm-rs/src/security.rs:553`), `manipulated_hp_and_damage_are_ignored` (`src/realm-rs/src/security.rs:732`). Eine vollständige Zustandsintegritätsprüfung über Loot, Crafting, EXP und Inventarpfade ist noch kein bestätigter Befund.
- **Offene Frage / Entscheidung:** Welche Zustandsänderungen aller sieben Gebiete serverseitig vollständig gegen Manipulation/Replay/Doppelanwendung geprüft werden und wo fehlende Prüfungen (z. B. AH-State) nachgeliefert werden müssen.
- **Verifizierte Belege:** `src/realm-rs/src/security.rs:476,534,545,553,732` (bestehende Tests), `docs/Serverautoritaet_und_Anti-Manipulation_V1.md` (fail-closed Auktionskauf-Validierung laut Inhaltsverzeichnis-Eintrag in `docs/README.md`), `src/realm-rs/src/handlers.rs` (Login-/Status-Logik).
- **Nächster zulässiger Schritt:** Read-only Auswertung der bestehenden Prüfungen und Pfadabdeckung im Folgeauftrag; Befunde hier eintragen.
- **Abschlussnachweis:** ausstehend.

### 4.8 Vorhandene Unit- und Integrationstests

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Testbestand `src/realm-rs` (insgesamt 416 Testfunktionen, z. B. async-Integrationstests in `src/realm-rs/src/spool.rs` und `src/realm-rs/src/security.rs`)
- **Bekannte Ausgangslage:** 416 Testfunktionen zählen per Quellzählung (historischer Testlauf: 416 bestanden, 0 fehlgeschlagen; Nachweis siehe Abschnitt 2 „Testbestand“); die aktuell bestehende Auswertung ist auf diesem read-only Audit erneut auszuführen.
- **Offene Frage / Entscheidung:** Aktueller Ausgabestand der Testsuite und Abdeckung kritischer Pfade (Persistenz, Gating, Zustandsintegrität).
- **Verifizierte Belege:** Abschnitt 2 „Testbestand“ dieser Datei; `src/realm-rs/src/security.rs` (16 Testfunktionen), `src/realm-rs/src/spool.rs` (7, davon async-Tests, z. B. `failed_spool_write_sets_degraded_and_keeps_dirty_and_revision`).
- **Nächster zulässiger Schritt:** Vorhandene Tests im Folgeauftrag ausführen (erlaubt); keine neuen Tests schreiben; Ausgabestand hier vermerken.
- **Abschlussnachweis:** ausstehend.

### 4.9 cargo audit

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Abhängigkeitsbestand von `src/realm-rs` (`Cargo.toml`/`Cargo.lock`)
- **Bekannte Ausgangslage:** Die Auswertung ist als read-only Tool-Aufruf vorgesehen (Abhängigkeitsdatenbank, keine Code-Änderung); in dieser Aufgabe wurde sie nicht ausgeführt.
- **Offene Frage / Entscheidung:** Bestehende Schwachstellen in Abhängigkeiten und deren Einordnung (kritisch/behebbar/dokumentiert).
- **Verifizierte Belege:** `src/realm-rs/Cargo.toml`/`Cargo.lock` (Abhängigkeitsbestand im Repository).
- **Nächster zulässiger Schritt:** `cargo audit` im Folgeauftrag read-only ausführen; Ergebnis hier vermerken.
- **Abschlussnachweis:** ausstehend.

### 4.10 cargo clippy --all-targets --all-features

- **Status:** `ZU PRÜFEN`
- **Priorität:** offen (im Audit-Auftrag nicht vergeben)
- **Betroffener Bereich:** Statische Analyse aller Targets und Features von `src/realm-rs`
- **Bekannte Ausgangslage:** Die Auswertung ist als read-only Tool-Aufruf vorgesehen (nur Berichte, keine Code-Änderungen); in dieser Aufgabe wurde sie nicht ausgeführt.
- **Offene Frage / Entscheidung:** Bestehende Hinweise (u. a. Nebenläufigkeit, Unsicherheiten in Fehlerbehandlung) und ihre fachliche Bewertung.
- **Verifizierte Belege:** `src/realm-rs/Cargo.toml` (Workspace-Target), Code unter `src/realm-rs/src/` als Analysezustand.
- **Nächster zulässiger Schritt:** `cargo clippy --all-targets --all-features` im Folgeauftrag read-only ausführen; Befunde hier eintragen.
- **Abschlussnachweis:** ausstehend.

## 5. Einheitliches Eintragsformat

Das vollständige Eintragsformat gilt für neue und offene Aufgaben in den Abschnitten 3 und 4:

- ID und Titel (z. B. `P-XX – Beispieltitel` bzw. Audit-Punkt mit Nummer)
- Status (`ZU PRÜFEN` / `BESTÄTIGT` / `ENTSCHEIDUNG OFFEN` / `ERLEDIGT`, Regeln siehe Abschnitt 1)
- Priorität (`GERING` / `MITTEL` / `HOCH`, oder offen falls im Auftrag nicht vergeben; Dringlichkeit/Risiko, kein Erkenntnisstand)
- Betroffener Bereich (Komponente, Datei, Doku-Abschnitt)
- Bekannte Ausgangslage (nur verifizierte Fakten)
- Offene Frage / Entscheidung
- Verifizierte Belege (Datei mit Zeile bzw. Doku mit Abschnitt und Zeile)
- Nächster zulässiger Schritt (Prüfung, Entscheidung oder Folgeauftrag; kein Vorgriff auf Code-Änderungen)
- Abschlussnachweis (Beleg oder „ausstehend“)

Abgeschlossene historische Nachweise in Abschnitt 2 dürfen das verkürzte Nachweisformat verwenden: Status, Belege und Abschlussnachweis; die übrigen Felder (offene Frage, betroffener Bereich im Einzelnen, nächster Schritt) entfallen, weil der Punkt keine offene Aufgabe mehr ist.
