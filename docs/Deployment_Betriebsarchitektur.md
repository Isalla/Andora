# Deployment- und Betriebsarchitektur

## 1. Ziel

Dieses Dokument beschreibt die verbindliche Betriebs- und Deployment-Architektur von Andora.

Sie gilt für alle Andora-Komponenten:

```text
API/Auth-Service (Go, src/api)
Login-Service (Go, src/login — implementiert)
Realm-Server (Rust, src/realm-rs — implementiert; Node.js/TS-Übergangsstand
  src/realm bleibt aktiv, bis die Ablösung abgeschlossen ist)
Coordinator (detailliert spezifiziert, Implementierung folgt)
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
- **Realm-Server (Rust)** – Spiel-Autorität (Combat, Loot, AH, NPC-AI via Coordinator/Ollama); verwaltet seine Realm-Datenbank `realm_state_<realm>` inklusive statischer Inhaltsversion.
- **Coordinator** – zentrale KI-Queue-/Ollama-Schnittstelle ohne Datenbankrechte.
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

---

## 5. Nicht-Root-Benutzer `andora`

Alle Andora-Komponenten laufen unter dem dedizierten Linux-Benutzer:

```text
andora
```

Es gelten:

- kein Root und kein sudo für Andora-Komponenten,
- das Panel führt keine Root-Systembefehle aus,
- der Agent verwaltet ausschließlich Andora-eigene Dienste und Dateien im Bereich dieses Benutzers,
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