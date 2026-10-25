# Deployment- und Betriebsarchitektur

## 1. Ziel

Dieses Dokument beschreibt die verbindliche Betriebs- und Deployment-Architektur von Andora.

Sie gilt für alle Andora-Komponenten:

```text
API/Auth-Service (Go, src/api)
Login-Service (Go, src/login — implementiert)
Realm-Server (Rust, src/realm-rs — implementiert; der frühere Node.js/TS-
  Realm src/realm ist entfernt)
Coordinator (Go, src/coordinator — implementiert; Realm-Anbindung offen)
Voice-Service (späterer Release)
Andora-Agent
andora-updater
```

Zielkette für den Spieleinstieg: `Auth/API → Login → Realm`. Einen
separaten Worldserver-Dienst gibt es nicht; der Realm-Server führt
seinen Realm direkt aus (Handoff-Übergabe, realm-gebunden, einmalig).

Ziel der Architektur ist:

- eine zentrale, übersichtliche Verwaltung aller Andora-Server,
- ein einheitlicher, überprüfbarer Update-Prozess für alle Dienste,
- ein sicheres Fernzugriffsmodell **ohne Remote-Shell**,
- der Betrieb aller Komponenten **ohne Root-Berechtigung**,
- eigenständige und unabhängige Realm-Updates.

---

## 2. Fünf getrennte Serverdienste

Die Andora-Serverdienste sind bewusst voneinander getrennt:

- **API/Auth-Service (Go)** – einziger Dienst mit direktem Auth-DB-Zugriff.
- **Login-Service** – separate Login-Komponente.
- **Realm-Server (Rust)** – Spiel-Autorität (Combat, Loot, AH, NPC-AI via Coordinator/KI-Provider (lokal: Ollama)); verwaltet seine Realm-Datenbank `realm_state_<realm>` inklusive statischer Inhaltsversion.
- **Coordinator** – zentrale KI-Queue-/Provider-Schnittstelle ohne Datenbankrechte (angeschlossener lokaler Provider: Ollama; Architektur providerunabhängig, siehe `docs/Coordinator.md`).
- **Voice-Service** – Voice-Komponente (späterer Release, Teil der Zielarchitektur).

Jeder Dienst kann eigenständig auf einem eigenen Debian-/Linux-Server betrieben werden.

Die Zielplattformen sind mindestens:

```text
linux-amd64
linux-arm64
```

Alle Binaries eines Dienstes werden aus demselben Quellstand erzeugt und sind funktional identisch.

---

## 3. Zentrales Admin-/Deployment-Panel

Das zentrale Admin-/Deployment-Panel verwaltet sämtliche Andora-Server im Betrieb.

Es bietet unter anderem:

- Übersicht aller verwalteten Server, Dienste und Realms,
- Status-/Healthcheck-Anzeige,
- Start, Stopp und Neustart von Diensten über den jeweiligen Agenten,
- Anstoßen und Überwachen von Updates,
- Anstoßen und Überwachen von Realm-Updates (Wartungsmodus),
- Zugriff auf Logs (eingeschränkt).

Das Panel kommuniziert **ausschließlich** über den eingangs beschriebenen mTLS-Kanal. Es besitzt keine Remote-Shell auf den verwalteten Servern.

---

## 4. Andora-Agent pro Server

Auf jedem verwalteten Andora-Server läuft genau ein **Andora-Agent**.

### Aufgaben

Der Agent:

- stellt die Verbindung zum zentralen Panel her,
- ist alleiniger Ansprechpartner für Verwaltungs- und Update-Aufträge,
- verwaltet ausschließlich die Andora-Komponenten des Servers,
- führt Teile des Update- und Realm-Update-Ablaufs lokal aus,
- meldet Status und Healthchecks zurück.

### Kommunikation

Panel und Agent kommunizieren über einen **dedizierten Port** des Servers mit **mTLS** (beidseitige Zertifikate). Es gibt:

- keine generische Remote-Shell,
- keinen SSH-Direktzugriff für das Panel,
- keine offenen Verwaltungsendpunkte nach außen.

Entschlüsselung, Betrieb und Zertifikatsverwaltung bleiben Teil der definierten Betriebskonfiguration.

### Umsetzungsstand (Debug-Build)

Erste Implementierung unter `src/agent/` (Go, Stdlib-only, `go test`):
verwaltete Dienste (Units), systemctl/journalctl via `sudo -n` (feste
sudoers-Regel, `deploy/sudoers/andora-agent`), Health/Version-Proben,
Logs mit Zeilen-/Byte-Obergrenzen. Authentifizierung ist die aufgeführte
**Zwischenlösung** Token (`X-Andora-Token`, fail-closed bei fehlendem
`AGENT_TOKEN`); mTLS ist konfigurationsseitig und serverseitig
(`RequireAndVerifyClientCert`) vorbereitet. Das PHP-Panel
(`web/andora-monitor`, `lib/agent.php`) nutzt den Agent als bevorzugten
Weg mit Legacy-Fallback (`via: agent|systemd`). Noch nicht umgesetzt:
Update- und Realm-Update-Abläufe (Aufgaben des künftigen
`andora-updater`).

Der Agent verwaltet ausschließlich die für diesen Server konfigurierten
Andora-Units — kein generisches systemctl, keine Shell.

---

## 5. Nicht-Root-Benutzer `andora`

Alle Andora-Komponenten laufen unter dem dedizierten Linux-Benutzer:

```text
andora
```

Es gelten:

- Komponenten laufen mit diesem Benutzer; kein generischer Root-Zugriff.
- Das lokale Panel führt **keine Root-Systembefehle** aus. Es spricht
  ausschließlich den Agenten an (Übergang: Legacy-Fallback auf die enge,
  weiter unten definierte sudo-Regel für `systemctl`-Steuerung).
- Der Agent erhebt Root-Rechte **nur** über eine eng begrenzte, fixierte
  sudo-Regel (`deploy/sudoers/andora-agent`): exakte `systemctl
  start|stop|restart`- und `journalctl`-Befehle für feste
  Andora-Units, ohne Shell, ohne generisches systemctl. Alle Lese-Zugriffe
  (Status, Health, Version) laufen ohne sudo.
- Das ist eine bewusste **Übergangslösung** (Debug-Build): Die Zielarchitektur
  sieht „kein sudo für Andora-Komponenten" vor; sobald alle Verwaltungs- und
  Update-Aufgaben über den Agenten laufen (inkl. `andora-updater`), wird die
  sudo-Regel zurückgebaut.
- systemd-Units (falls eingesetzt) starten die Dienste mit diesem Benutzer.

Systemnahe Änderungen (Pakete, Firewall, Benutzeranlage) sind bewusst und manuell vom Serververantwortlichen durchzuführen und gehören nicht zu den Rechten von Panel, Agent oder Updater.

---

## 6. `andora-updater`

Der **`andora-updater`** aktualisiert alle Andora-Dienste eines Servers – einschließlich des Andora-Agenten selbst.

### Ablauf

```text
Manifest prüfen (signiert)
        ↓
Prüfsummen verifizieren
        ↓
Downloads/Sammlung prüfen
        ↓
Zielkomponente in Wartung versetzen (falls nötig)
        ↓
Update einspielen
        ↓
Healthcheck des aktualisierten Dienstes
        ↓
bei Erfolg: freigeben
bei Fehler: automatisches Rollback
```

### Regeln

- Updates werden nur über **signierte Manifeste** angestoßen.
- Jede Datei wird anhand ihrer Prüfsumme verifiziert.
- Vor dem Update entsteht – soweit fachlich erforderlich – ein Backup.
- Nach dem Update folgt ein **Healthcheck** des Dienstes.
- Schlägt der Healthcheck fehl, wird automatisch auf den letzten funktionierenden Stand zurückgesetzt (Rollback).
- Der Updater aktualisiert auch den Agenten selbst, damit der Betrieb nicht dauerhaft an einer alten Agentenversion hängt.
- Fehlgeschlagene Dienste dürfen nicht unbemerkt stehen bleiben.

---

## 7. Realm-Updates

Realm-Updates (Content-/Schema-/Datenmigrationen eines Realms) sind ein automatisierter Sonderfall des Update-Prozesses.

Jeder Realm besitzt seinen vollständigen statischen (Inhaltsversion) und dynamischen Datenstand in `realm_state_<realm>` und ist damit eigenständig aktualisierbar und migrierbar.

### Ablauf für einen einzelnen Realm

```text
Wartungsmodus aktivieren (Realm für Spieler schließen/warten)
        ↓
Realm-Shutdown
        ↓
Backup
        ↓
Update (statische Definitionen / Schema)
        ↓
Migration (automatisch beim Realm-Start, je eigene DB/db_version)
        ↓
Healthcheck
        ↓
Freigabe (Realm wieder öffnen)
```

Der Migrationsschritt wird vom Realm-Server selbst ausgeführt: Beim Start wendet er automatisch alle noch fehlenden, versionsbasierten SQL-Migrationen seiner eigenen Datenbanken an (siehe `Datenbank_Architektur.md`, Abschnitt Migrationen und db_version). Schlägt eine Migration fehl, bricht der Start mit klarer Fehlermeldung ab, bevor Spieler zugelassen werden; der Realm bleibt bis zur Behebung geschlossen.

### Regeln

- Jeder Realm wird getrennt aktualisiert.
- Ein Realm-Update erzwingt **keinen** Neustart oder Downtime anderer Realms.
- Die Migrationen und Updates betreffen ausschließlich die Datenbanken des betroffenen Realms (eigene `db_version`-Historie je Datenbank, keine zentrale globale Migrationssteuerung).
- Der Wartungsmodus verhindert, dass Spieler während der Migration in einen inkonsistenten Realmzustand geraten.
- Im Fehlerfall greifen die Rollback- und Backup-Regeln des Updaters.
- Destruktive oder nicht rückwärtskompatible Migrationen (in der Datei mit `-- destructive: <Grund>` markiert) laufen nur mit `ALLOW_DESTRUCTIVE_MIGRATIONS=1`. Diese Freigabe darf ausschließlich innerhalb dieses Ablaufs **nach** dem Backup-Schritt gesetzt und muss danach wieder entfernt werden. Ohne Freigabe bricht der Realm-Start vor der destruktiven Migration ab.

### Realm-Versionen

Verschiedene Realm-Versionen können parallel betrieben werden, beispielsweise:

```text
Live
Classic
Test
Event
```

Jede Realm-Version kann einen eigenen Inhalts- und Datenstand besitzen und wird unabhängig aktualisiert.

**Hinweis:** Realm-Versionen (Live, Classic, Test, Event) sind Inhaltsversionen eines Realms, keine clientgespezifischen Realms. Es gibt keine PC-, Pi-, Browser- oder UE-Realms; alle offiziellen Clients verbinden sich mit denselben Realms (`Mehrere_Offizielle_Clients.md`).

---

## 8. Verhältnis zur bisherigen lokalen Monitoring-Doku

Die bisherige Umsetzung – ein **lokales Monitoring-/Admin-Panel** auf dem Rechner des Gameservers, gesteuert über `sudo -n systemctl` auf demselben Host – ist der **Übergangs-/Legacy-Stand**.

Sie ist in `docs/monitoring_web_panel.md` als Übergangsstand dokumentiert und wird durch die hier beschriebene zentrale Panel-/Agent-Architektur abgelöst.

Die Vorlagen unter `deploy/` (systemd-Unit-Vorlagen usw.) beschreiben den bisherigen Stand und sind nicht Teil dieser Zielarchitektur.

---

## 9. Leitsätze

> Jeder Andora-Dienst kann auf einem eigenen Debian-/Linux-Server laufen.

> Zielplattformen sind mindestens `linux-amd64` und `linux-arm64`.

> Die fünf Serverdienste bleiben getrennt und eigenständig betreibbar.

> Panel und Agent kommunizieren ausschließlich über mTLS – niemals über eine Remote-Shell.

> Alle Andora-Komponenten laufen unter dem Nicht-Root-Benutzer `andora`.

> Updates erfolgen ausschließlich über signierte Manifeste mit Prüfsummen, Healthchecks und Rollback.

> Der `andora-updater` aktualisiert auch den Agenten selbst.

> Jeder Realm ist eigenständig aktualisierbar und migrierbar.

> Ein Realm-Update erzwingt keinen Neustart anderer Realms.

> Realm-Versionen (Live, Classic, Test, Event) können parallel und unabhängig betrieben werden.

> Client-Plattformen ändern die Server-Zielarchitektur nicht: alle offiziellen Clients verbinden sich mit denselben Realms (`Mehrere_Offizielle_Clients.md`).

## 10. Netzwerkgrenzen der Realm-Listener (Audit H-3)

Der Realm-Server betreibt zwei getrennte TCP-Listener mit unterschiedlichen
Defaults:

| Listener | Port-Variable | Default-Port | Inhalt | Bindung bei leerem Bind-Host |
|---|---|---|---|---|
| WebSocket-Weltserver (Spielclients) | `PORT_WS` | 3001 | Spielprotokoll (HELLO, MOVE, …) | alle Interfaces — **unverändert** |
| Health-/Status-/Mail-Inbound | `PORT_HTTP` | 3002 | `/health`, `/status`, `/players`, `/characters`, `POST /internal/mail/*` | **nur `127.0.0.1`** (H-3) |

### Endpunkte des Health-Listeners und ihre Preisgabe

| Endpunkt | Authentifizierung | Gezeigte Daten |
|---|---|---|
| `GET /health` | keine | realm_id, Laufzeit |
| `GET /status` | keine | **Software-Version**, Spielerzahl, Tick-Latenzen, Persistenzstatus, Quarantänebestand |
| `GET /players` | keine | **Name, Charakter-ID, Zone, Ping und Inaktivitätszeit jeder online spielenden Person** |
| `GET /characters` | gültige Login-Session (Validierung über die Auth-API) | ausschließlich eigene Charaktere (id + Name) |
| `POST /internal/mail/*` | Service-HMAC (`MAIL_INBOUND_*`), ±300 s-Zeitfenster, Eigentumsprüfung gegen die Realm-DB | Mail-Bestand des betroffenen Charakters |

`/status` und `/players` sind nicht harmlos: die Versionsnummer erlaubt einen
Fingerabdruck der eingesetzten Realm-Version, `/players` gibt Anwesenheit und
Aktivitätsverhalten online spielender Personen preis. Beide Antworten bleiben
unverändert (keine API-Änderung); die Begrenzung erfolgt über Bind-Adresse
und Netzwerk.

### Bindung und Startwarnung

* Kein oder leerer `HEALTH_BIND_HOST` → der Health-Listener lauscht
  ausschließlich auf `127.0.0.1`.
* Ausdrücklich gesetzte Werte behalten ihre Bedeutung: `0.0.0.0`, `::`,
  `auto`, `ipv4`, `ipv6`, `dual`, IP-Literale (auch private) und Hostnamen.
* Eine ausdrücklich konfigurierte **Wildcard-Bindung** (`0.0.0.0` oder `[::]`)
  wird beim Start mit deutlicher Warnung protokolliert: Nennung der
  betroffenen Bind-Adresse und Hinweis, dass `/health`, `/status` und
  `/players` ohne Authentifizierung erreichbar sind. Secrets, Header oder
  Konfigurationswerte erscheinen im Log nicht. Loopback-Bindungen erzeugen
  keine Warnung.
* Ein unauflösbarer Host ist ein Fehler des Health-Starts. Es wird nicht
  still auf eine – erst recht nicht auf eine unsichere – Ersatzadresse
  zurückgefallen.

### Externe Aufrufer: Go-Mail-Forward und Charakterliste

Auf den Health-Port (`PORT_HTTP`, Default 3002) richten sich zwei
Ausnahmen von der Loopback-Betrachtung:

* **Go-Mail-Forward:** `MAIL_FORWARD_URL` des Go-API zeigt auf
  `POST /internal/mail/*` dieses Listeners.
* **Charakterwahl der Spielclients:** `GET /characters` liegt auf demselben
  Listener und wird vom Client über die Realm-`http_url` erreicht, die der
  Login-Dienst aus `REALM_HTTP_URLS` liefert.

Laufen Go-API und Realm auf verschiedenen Hosts oder in verschiedenen
Containern, muss `HEALTH_BIND_HOST` ausdrücklich auf eine **interne**, dem
Go-API erreichbare Adresse gesetzt werden (zum Beispiel `192.168.10.5` oder
ein interner Hostname); der neue Loopback-Default würde diese Verbindungen
abschneiden. Soll die Charakterliste Clients unmittelbar erreichbar sein,
ist ebenfalls eine ausdrückliche Bindung (interne Adresse des Client-Netzes)
oder ein Reverse-Proxy auf diesen Port zu wählen. Panel und Agent rufen
`127.0.0.1:3002` auf (`deploy/conf/monitor.conf`, `deploy/conf/agent.conf`)
und bleiben vom neuen Default unberührt.

Empfohlen: Bindung an das interne Interface **und** netzwerkseitige
Beschränkung des Health-Ports auf die Absender von Go-API und Panel
(Firewall, Security Group oder Container-Netz). Die HMAC-Prüfung bleibt
unverändert bestehen und ist die inhaltliche Trust-Grenze; sie ersetzt keine
Netzwerksegmentierung.

### Migration bestehender Installationen

Installationen, die den Health-Listener bisher über den leeren Wert an alle
Interfaces gebunden haben, lauschen nach diesem Stand nur noch auf dem
Loopback. Für das bisherige Verhalten wird `HEALTH_BIND_HOST=0.0.0.0`
ausdrücklich gesetzt; produktiv ist eine interne Adresse oder ein interner
Hostname der bessere Wert. Der WebSocket-Listener (`WS_BIND_HOST`) bleibt
vom Umstieg unberührt.