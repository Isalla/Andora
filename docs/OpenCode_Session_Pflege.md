# Pflege der OpenCode-Sessions und der OpenCode-Datenbank

## Einordnung

OpenCode-Sessions sind **temporäre Arbeitsdaten** der Entwicklungs-KI und keine dauerhafte Projektdokumentation.

Sie liegen in der SQLite-Datenbank `~/.local/share/opencode/opencode.db` (Tabelle `session` sowie alle sessionsgebundenen Tabellen, z. B. `message`, `part`, `event`, `session_message`, `todo`, `session_input`, `session_context_epoch`, `session_share`, `session_context_epoch`).

Dauerhaft relevantes Projektwissen (Architekturentscheidungen, Implementierungsstände, offene Aufgaben, fachliche Entscheidungen) darf **niemals ausschließlich in einer OpenCode-Session** gespeichert sein. Solches Wissen muss in den regulären Andora-Dokumentationen (`docs/`) bzw. im Git-Arbeitsstand enthalten sein.

## Automatische Löschung

* Alle OpenCode-Sessions, deren **letzte Aktivität mehr als 3 Tage** zurückliegt, werden **automatisch und ohne Rückfrage** gelöscht.
* „Letzte Aktivität" bedeutet den Zeitpunkt der letzten tatsächlichen Änderung der Session (`time_updated` der Tabelle `session`, ggf. der zugehörigen sessionsgebundenen Tabellen).
* Nur Inaktivität ist das Löschkriterium. Die Reihenfolge oder Dauer vorheriger Aktivität ist nicht relevant.
* Die Löschung umfasst die Session **inklusive aller ihr zugehörigen Daten** (Messages, Parts, Events, Todos, Inputs, Epochs, Shares).

## Ausnahme: nicht Git-gesicherte relevante Projektarbeit

Eine Session darf **nicht** automatisch gelöscht werden, wenn sie **relevante Projektarbeit enthält, die noch nicht in Git gesichert ist**.

Dazu gehören insbesondere:

* noch nicht commitete, aber bereits erarbeitete Änderungen (Working-Tree-Änderungen/dirty Files aus dieser Session),
* architektonische oder fachliche Entscheidungen, die noch nicht in `docs/` übernommen wurden,
* nicht abgeschlossene oder nicht getestete Implementierungszustände, die für die weitere Entwicklung benötigt werden.

Vor jeder automatischen Löschung ist daher zu prüfen:

1. Ist der relevante Arbeitsstand dieser Session in Git gesichert (committet bzw. ohnehin Teil des Git-Arbeitsstands)?
2. Ist dauerhaft relevantes Projektwissen bereits in den regulären Andora-Dokumentationen (`docs/`) enthalten?

Erst wenn beide Punkte erfüllt sind, ist die automatische Löschung zulässig.

## Datenbankpflege der OpenCode-Datenbank

* Nach der Session-Bereinigung wird geprüft, ob durch die Löschung eine **relevante Menge freier SQLite-Seiten** entstanden ist (z. B. via `PRAGMA freelist_count` und Tabellengroßen wie `dbstat`).
* Ist eine Kompaktierung sinnvoll, darf die OpenCode-Datenbank anschließend mit einem geeigneten SQLite-Verfahren verkleinert werden (z. B. `VACUUM` in einem Offline-Pass bzw. `sqlite3 ... ".compact"`).
* **Schreibende direkte Datenbankoperationen und Offline-Kompaktierung sind nur zulässig, wenn kein OpenCode-Prozess auf die Datenbank zugreift.**
* Vor jeder schreibenden Pflege wird ein **Backup der Datenbank** angelegt.
* Nach jeder schreibenden Pflege wird die **Integrität der Datenbank geprüft** (z. B. `PRAGMA integrity_check`) und OpenCode wird **testweise gestartet**, um die Funktionsfähigkeit zu bestätigen.
* Es werden **keine Daten pauschal gelöscht** und es wird nichts entfernt, dessen Zweck und Entbehrlichkeit nicht eindeutig verifiziert ist.

## Nicht sessionspezifische Daten dürfen nicht entfernt werden

Folgende Daten der OpenCode-Umgebung dürfen durch die Bereinigung **nie entfernt oder verändert** werden:

* OpenCode-Konfiguration (z. B. `opencode.jsonc`),
* Skills,
* Provider- und Modelleinstellungen,
* Konten-/Authentifizierungszustände (z. B. `auth.json`, `account`, `account_state`, `credential`-Tabellen),
* Projekt- und Arbeitssplatzreferenzen (`project`, `workspace`, `project_directory`),
* Berechtigungs-/Auditing-/Migrationsdatensätze,
* sonstige nicht sessionspezifische Tabellendaten.

Gekürzt werden darf ausschließlich sessionsgebundener Bestand gemäß den Regeln oben.

## Bevorzugter Löschmechanismus

Wenn OpenCode einen **offiziellen, sicheren Mechanismus zur Session-Löschung** anbietet (CLI-/API-Befehl oder offizielle Archivierungs-/Löschlogik), ist dieser **vor direkten SQLite-Eingriffen zu bevorzugen**.

Direkte SQLite-Eingriffe sind nur dann zulässig, wenn kein offizielles Werkzeug verfügbar ist oder wenn sie gegen die oben genannten Regeln als sicher dokumentiert und testweise verifiziert sind.

## Abgrenzung

Diese Regel gilt **ausschließlich** für die OpenCode-Session- und Datenbankpflege der Entwicklungs-KI.

Für die MariaDB-Datenbanken der Andora-Dienste (`auth`, `realm_state_<realm>` usw.) gelten die dort definierten Regeln, u. a. in `Datenbank_Architektur.md` (Abschnitt 23) – dort gilt insbesondere, dass keine Datenbanken, Rechte oder Zugangsdaten außerhalb der definierten Bereiche verändert werden dürfen.

Die Regel ist Bestandteil des Dev-Prozesses und wird in `ai_jobs.md` sowie im Inhaltsverzeichnis der Projektdokumentation (`docs/README.md`) verlinkt.
