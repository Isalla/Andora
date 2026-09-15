PROJEKTROOT / PFADE

Der Root des aktuell geöffneten Andora-Repositories ist der **Projektroot**. Er ist nicht an einen festen Pfad gebunden und darf nicht als feste Voraussetzung verwendet werden.

Alle Arbeitsregeln und Pfadangaben beziehen sich relativ auf diesen Projektroot.

Bei jedem Job/Auftrag MUSS die Coding-KI `references/README.md` mit berücksichtigen und relevante Inhalte daraus in die Arbeit einbeziehen.

Die Coding-KI arbeitet ausschließlich innerhalb des Projektroots, sofern ein Arbeitsauftrag nicht ausdrücklich etwas anderes erlaubt.

Grundstruktur (relativ zum Projektroot):

- `.tmp/` – temporäre Projektdateien (einschließlich aller Toolchains, Downloads und temporärer Artefakte)
- `deploy/` – Deployment-Artefakte
- `docs/` – Projektdokumentation
- `references/` – technische Referenzen und Bücher

Absolute Pfade werden nur dort verwendet, wo sie technisch tatsächlich notwendig sind; solche Pfade müssen konfigurierbar bzw. deploymentabhängig sein.

SYSTEMGRENZE / INSTALLATIONEN (VERBINDLICH)

Systemweite Installationen oder Änderungen am Betriebssystem sind ohne vorherige ausdrückliche Zustimmung des Nutzers verboten.

Dazu gehören insbesondere:

* `apt` / `apt-get` und andere systemweite Paketverwaltungen,
* systemweite `pip`-, `npm`-, `cargo`- oder vergleichbare Installationen,
* eigenmächtige Verwendung von `sudo` für Systemänderungen,
* Installation oder Entfernung von Diensten, Servern, Containern oder Laufzeitumgebungen,
* Änderungen an `/etc`, systemd, Benutzern, Gruppen, Berechtigungen, Firewall, Netzwerk-, Mount- oder Betriebssystemkonfiguration,
* sonstige Änderungen außerhalb des Projektroots, die den Host dauerhaft verändern.

Fehlt für eine Aufgabe ein Werkzeug, muss zuerst nach einer projektlokalen Lösung unter `<Projektroot>/.tmp/` gesucht werden (vgl. TOOLCHAINEN / TEMPORÄRE AUSLEGER).

Ist eine projektlokale Lösung nicht sinnvoll oder technisch nicht möglich, muss die Coding-KI **vor** jeder systemweiten Installation oder Änderung den Nutzer um ausdrückliche Zustimmung bitten und kurz erklären:

1. was installiert oder geändert werden soll,
2. warum es benötigt wird,
3. welche systemweiten Auswirkungen die Änderung hat.

AUTONOMES ARBEITEN BEI LANGEN AUFTRÄGEN

Bei einem laufenden Coding-Auftrag arbeitest du selbstständig weiter, bis der Auftrag vollständig abgeschlossen und geprüft wurde.

Fehlerhafte, ungültige oder leere Job-/Tool-Anfragen sind KEIN Grund, die Arbeit zu pausieren oder auf Benutzer-Input zu warten.

Wenn eine solche Anfrage auftritt:
1. Gib eine kurze sichtbare Meldung aus:
   "⚠ Fehlerhafte/leere Anfrage erkannt – Arbeit wird fortgesetzt."
2. Überspringe, entferne oder korrigiere die fehlerhafte Anfrage soweit möglich.
3. Fahre unmittelbar selbstständig mit dem aktuellen Arbeitsschritt fort.
4. Falls derselbe Versuch erneut fehlschlägt, verwende einen sinnvollen alternativen Lösungsweg.
5. Pausiere nicht allein wegen eines technischen Fehlers.

Frage den Benutzer nur dann, wenn eine zwingend notwendige fachliche oder architektonische Entscheidung getroffen werden muss, die weder aus dem aktuellen Auftrag noch aus den Projektdokumentationen oder dem bestehenden Code eindeutig hervorgeht.

Technische Probleme, fehlgeschlagene Tool-Aufrufe, leere Antworten, ungültige Jobs oder ein zunächst nicht funktionierender Lösungsweg gelten nicht automatisch als solche Entscheidung.

Ausnahme: die allgemeine Anweisung zum autonomen Weiterarbeiten hebt die Grenze aus SYSTEMGRENZE / INSTALLATIONEN ausdrücklich **nicht** auf. Eine fehlende systemweite Installation ist immer ein Zustimmungspflicht-Fall und kein Grund für eigenmächtiges Installieren.

Ziel:
Auch unbeaufsichtigte, mehrstündige Coding-Aufträge sollen ohne unnötigen Benutzer-Input bis zum Abschluss weiterlaufen.

Ein Assistant-Turn ohne Tool-Aufruf, obwohl der aktuelle Coding-Auftrag noch nicht abgeschlossen ist, darf nicht als Ende oder Pause des Auftrags behandelt werden.

Vor jedem Warten auf Benutzer-Input muss geprüft werden:
- Ist der aktuelle Auftrag vollständig abgeschlossen?
- Wurden alle vorgesehenen Arbeitsschritte durchgeführt?
- Wurden die vorgesehenen Tests/Prüfungen durchgeführt?

Falls NEIN und keine zwingende Benutzerentscheidung erforderlich ist:
- kurze Fehlermeldung ausgeben,
- selbstständig mit dem nächsten Arbeitsschritt fortfahren,
- NICHT auf Benutzer-Input warten.

KONTEXTRESERVE

Die Entwicklungs-KI darf den verfügbaren Kontext nicht vollständig aufbrauchen.

Wenn sich der aktuelle Kontext dem Modelllimit nähert, muss sie rechtzeitig vor Erreichen des Limits:

1. den aktuellen Arbeitsstand kompakt zusammenfassen,
2. wichtige Entscheidungen, offene Aufgaben, geänderte Dateien und noch notwendige Tests sichern,
3. nicht mehr benötigten Gesprächskontext reduzieren,
4. mindestens etwa 20.000 Tokens Arbeitsreserve wiederherstellen,
5. anschließend selbstständig mit dem laufenden Coding-Auftrag fortfahren.

Eine Session darf nicht erst dann komprimiert werden, wenn das Kontextlimit bereits erreicht oder überschritten wurde.

Ziel:
Für weitere Tool-Calls, Codeänderungen, Tests und Abschlussberichte müssen jederzeit ausreichend freie Tokens verbleiben.

TOOLCHAINEN / TEMPORÄRE AUSLEGER

Die Projekt-Toolchains liegen unterhalb des Projektroots (relativ):

Go-Toolchain:
- Toolchain-Binary: `.tmp/go/go-toolchain/bin/go` (relativ zum Projektroot; aktuell go1.27.1, linux/arm64)
- Mod-Cache: `.tmp/go-mod-cache`
- Build-Cache: `.tmp/go-build-cache`

Rust-Toolchain:
- Rust-Installation (rustup, rustc, cargo): `.tmp/rust` bzw. `.tmp/rustup` (relativ zum Projektroot; Profil `minimal`, Toolchain `stable-aarch64-unknown-linux-gnu`)
- Aktivierung: `source .tmp/rust/env.sh` (setzt `RUSTUP_HOME`, `CARGO_HOME`, `PATH` passend)
- Direkter Aufruf ohne Aktivierung: `.tmp/rust/bin/cargo`, `.tmp/rust/bin/rustc`,
  `.tmp/rust/bin/rustfmt`, `.tmp/rust/bin/clippy-driver` (relativ zum Projektroot).
  Diese Pfade können in beliebigen Pfaden abgelegt werden, siehe unten.
- Details: `docs/Temporäre_Dateien.md` (Abschnitt „Projekt-Toolchains in `.tmp/`")

Jeder NEUE Coding-Auftrag (gofmt, vet, build, test für Go; cargo build/test für Rust, Downloads temporärer Toolchains) MUSS die jeweils passende projektinterne Toolchain unter `.tmp/` verwenden. Der systemweite Rust-Pfad `~/.cargo` wird NICHT verwendet.

**Rust-Aufruf ohne systemweite Suche:** Toolchains werden ausschließlich innerhalb des
Projektroots unter `.tmp/` gesucht und verwendet. Für Rust ist die zu verwundende Binary
`.tmp/rust/bin/cargo` (relativ zum Projektroot). Es darf NIE systemweit nach `cargo`/`rustc`
gesucht werden (`which cargo`, `find / -name cargo`, `/etc/environment` o. Ä.). Fehlt der
Binary-Pfad, wird die Toolchain unter `.tmp/` verifiziert oder der Nutzer gefragt — nicht
außerhalb des Projektroots.

Regeln:
- `/etc` und (dauerhafte Projektdateien in) `/tmp` werden für Coden und Kompilieren NICHT benutzt.
- Kurzlebige Arbeits- und Zwischendaten dürfen in `/tmp/opencode` bzw. systemweites `/tmp`
  (z. B. Buch-/EPUB-Extraktionen, temporäre Analysedaten, Session-Artefakte); dauerhafte
  Projektdateien dort sind verboten (Details: `docs/Temporäre_Dateien.md`).
- Temporäre Toolchains, Downloads und temporäre Artefakte ohne Bestand landen ausschließlich in `.tmp/`
  (relativ zum Projektroot, z. B. `.tmp/go` für eine neu geladene Toolchain).
- Das Operating System des Projekts lässt sich über `.tmp/os-release` (relativ zum Projektroot)
  prüfen (aktuell Debian GNU/Linux 13 (trixie), ARM64). Diese Datei ersetzt das systemweite
  Auslesen aus `/etc/os-release`.
- Entwicklungs-/Testsystem: Die Test-Binärdateien werden auf einem ARM64-System gebaut
  (arm64, z. B. `go1.x.x.linux-arm64`).
  - Produktions-Binärdateien: Für die Produktion fallen ARM64 und AMD64 an
  (CROSS-COMPILIATION von der ARM64-Entwicklungsumgebung nach AMD64, z. B. via
  `GOARCH=amd64 GOOS=linux go build ...`).

PROJEKT-STATUS-DOKUMENTATION

Der tatsächliche Implementierungsstand aller Dienste und Systeme wird zentral in
`docs/Projekt-Status.md` (relativ zum Projektroot) festgehalten.

Die Coding-KI MUSS `docs/Projekt-Status.md` selbstständig und unmittelbar aktualisieren,
wann immer sich der tatsächliche Implementierungsstand ändert (neue Endpoints/Handler,
implementierte/freigeschaltete C2S-Typen, neue Module/Migrationen, Tests, Wegfall oder
Neustufung von Legacy-Bestand). Abschlussberichte verweisen auf den aktualisierten Eintrag.

Maßstab sind Code, Tests und DB-Migrationen – eine Spezifikation gilt erst als
implementiert, wenn Code existiert. Die Statusstufen und die Gliederung richten sich nach
`docs/Projekt-Status.md`.

OPENCODE-SESSIONS- UND DATENBANKPFLEGE

OpenCode-Sessions sind temporäre Arbeitsdaten, keine dauerhafte Projektdokumentation.

* Alle OpenCode-Sessions mit letzter Aktivität älter als 3 Tage werden automatisch
  und ohne Rückfrage gelöscht.
* Ausnahme: keine automatische Löschung, wenn die Session noch nicht in Git
  gesicherte relevante Projektarbeit enthält (relevante Architektur-/ und
  Implementierungsstände müssen vorher in `docs/` bzw. Git abgelegt werden).
* Datenbankpflege der OpenCode-Datenbank (Kompaktierung): nur bei nachweislich
  relevantem Freelist-/Fragmentations-Bestand, vorher Backup, nur ohne parallelen
  OpenCode-Prozess, danach Integritätsprüfung (`PRAGMA integrity_check`) und
  testweiser Start.
* Wenn OpenCode einen offiziellen Löschmechanismus anbietet, hat dieser Vorrang
  vor direkten SQLite-Eingriffen.
* Konfigurationen, Skills, Provider-/Modelleinstellungen und sonstige
  nicht sessionspezifische Daten werden durch die Bereinigung nicht entfernt.

Details: `docs/OpenCode_Session_Pflege.md`.