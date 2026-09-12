# Deploy-Anleitung: Andora-Monitoring (Vorlagen in diesem Ordner)

Diese Dateien sind **Vorlagen**. Sie werden manuell auf dem Produktionsserver
installiert. Auf dem Entwicklungsrechner wurde **nichts** installiert.

> Produktions-Root-Beispiel (anpassen!): `/opt/andora`
> - `/opt/andora/server`    = Rust-Realm: Binary `andora-realm` (Release-Build aus
>   `src/realm-rs/`) + `migrations/` (neben dem Binary) + `config.env`
> - `/opt/andora/monitor`   = das komplette `web/andora-monitor/`-Verzeichnis (PHP-Panel)
> - `/opt/andora/agent`     = `src/agent/` gebaut + `config.env` (Andora-Agent, Management-API)
> - Das Repository selbst bleibt auf dem Dev-Rechner; nur Build-Artefakte landen bei Produktion.

---

## 0. Produktionsbetrieb des PHP-Panels (Apache + PHP-FPM)

Das PHP-Panel wird in Produktion **nicht** mit `php -S` betrieben (ein Prozess
kann nur einen Listener abhören → kein echtes Dual-Stack, kein robuster
Produktionsbetrieb). Stattdessen laufen **zwei** systemd-Units:

1. `andora-monitor-fpm.service` — dedizierter PHP-FPM-Master (führt `public/index.php` aus, Unix-Socket)
2. `andora-monitor-apache.service` — dedizierte Apache-Instance (bedient Port 3003, leitet PHP an FPM weiter)

`php -S` bleibt ausschließlich für die lokale Entwicklung (`readme.md`).

**IPv4/IPv6/Dual-Stack** (Netz-Regel `4b53e36`): Dual-Stack = zwei explizite
Listener. Die `Listen`-Direktiven stehen im Block [1]–[4] von
`deploy/conf/andora-monitor-site.conf`; es wird genau ein Block aktiviert
(Standard: [1] = nur `127.0.0.1`). Es wird **nicht** auf implizite
IPv4-mapped-IPv6-Sockets vertraut (plattformabhängig).

---

## 1. Welche Datei wohin kopieren

| Datei in diesem Projekt | Produktionsziel |
|---|---|
| `src/realm-rs/` (Release-Build `andora-realm` + `migrations/` + `config.env`) | `/opt/andora/server/` |
| `web/andora-monitor/` (PHP-Panel) | `/opt/andora/monitor/` |
| `src/agent/` + Binär `agent` | `/opt/andora/agent/` |
| `deploy/systemd/andora-realm.service` | `/etc/systemd/system/andora-realm.service` |
| `deploy/systemd/andora-monitor-fpm.service` | `/etc/systemd/system/andora-monitor-fpm.service` |
| `deploy/systemd/andora-monitor-apache.service` | `/etc/systemd/system/andora-monitor-apache.service` |
| `deploy/systemd/andora-agent.service` | `/etc/systemd/system/andora-agent.service` |
| `deploy/conf/monitor.conf` | `/opt/andora/monitor/monitor.conf` |
| `deploy/conf/monitor-fpm.conf` | `/opt/andora/monitor/monitor-fpm.conf` |
| `deploy/conf/monitor-apache.conf` | `/opt/andora/monitor/monitor-apache.conf` |
| `deploy/conf/andora-monitor-site.conf` | `/opt/andora/monitor/andora-monitor-site.conf` |
| `deploy/conf/agent.conf` | `/opt/andora/agent/config.env` |
| `deploy/sudoers/andora-monitor` | `/etc/sudoers.d/andora-monitor` |
| `deploy/sudoers/andora-agent` | `/etc/sudoers.d/andora-agent` |

Realm-Binary (Rust) bauen und mitkopieren:
```bash
source .tmp/rust/env.sh
cargo build --release --manifest-path src/realm-rs/Cargo.toml
# target/release/andora-realm + migrations/ + config.env (aus src/realm-rs/config.env
# kopiert und befüllt) wandern nach /opt/andora/server/
```

Agent-Binary selbst bauen und mitkopieren:
```bash
cd src/agent && .tmp/go/go-toolchain/bin/go build -o agent .
# agent + config.env (aus agent.conf) wandern nach /opt/andora/agent/
```

Vorher auf dem Zielsystem (Pakete):
```bash
sudo apt install php8.2-fpm apache2 libapache2-mod-proxy-fcgi php8.2-curl
# Apache-Module für den Proxy ins Global-Config laden (siehe monitor-apache.conf):
sudo a2enmod proxy proxy_fcgi
```

---

## 2. Besitzer & Dateirechte

```bash
# Realm-Gameserver (Rust, läuft als User "andora")
sudo chown -R andora:andora /opt/andora/server
sudo chmod 755 /opt/andora/server
sudo chmod 600 /opt/andora/server/config.env   # enthält DB-Passwort/Token

# Monitoring-Panel (läuft als User "pi" — User in den Units ggf. anpassen)
sudo chown -R pi:pi /opt/andora/monitor
sudo chmod 755 /opt/andora/monitor
# monitor.conf enthält evtl. Token -> nur für "pi" lesbar
sudo chmod 600 /opt/andora/monitor/monitor.conf
# data/ muss vom FPM-Pool geschrieben werden (history.json)
sudo chown -R pi:pi /opt/andora/monitor/data
sudo chmod 700 /opt/andora/monitor/data

# Log-/Runtime-Verzeichnisse des Panels
sudo mkdir -p /run/andora-monitor /var/log/andora-monitor
sudo chown pi:pi /run/andora-monitor /var/log/andora-monitor

# Andora-Agent (läuft als User "andora")
sudo chown -R andora:andora /opt/andora/agent
sudo chmod 755 /opt/andora/agent
sudo chmod 600 /opt/andora/agent/config.env   # enthält AGENT_TOKEN

# systemd-Units: root-eigenn, standard Rechte
sudo chown root:root /etc/systemd/system/andora-realm.service /etc/systemd/system/andora-monitor-*.service /etc/systemd/system/andora-agent.service
sudo chmod 644 /etc/systemd/system/andora-realm.service /etc/systemd/system/andora-monitor-*.service /etc/systemd/system/andora-agent.service

# sudoers-Dateien: root-eigenn, genau 0440 (wichtig!)
sudo chown root:root /etc/sudoers.d/andora-monitor /etc/sudoers.d/andora-agent
sudo chmod 0440 /etc/sudoers.d/andora-monitor /etc/sudoers.d/andora-agent
sudo visudo -cf /etc/sudoers.d/andora-monitor   # Syntax-Check
sudo visudo -cf /etc/sudoers.d/andora-agent     # Syntax-Check
```

---

## 3. systemd-Befehle (Installation + Aktivierung)

```bash
sudo cp deploy/systemd/andora-realm.service             /etc/systemd/system/andora-realm.service
sudo cp deploy/systemd/andora-monitor-fpm.service       /etc/systemd/system/andora-monitor-fpm.service
sudo cp deploy/systemd/andora-monitor-apache.service    /etc/systemd/system/andora-monitor-apache.service
sudo cp deploy/systemd/andora-agent.service             /etc/systemd/system/andora-agent.service
sudo cp deploy/conf/monitor.conf monitor-fpm.conf monitor-apache.conf andora-monitor-site.conf /opt/andora/monitor/
sudo cp deploy/conf/agent.conf                          /opt/andora/agent/config.env
sudo cp deploy/sudoers/andora-monitor       /etc/sudoers.d/andora-monitor
sudo cp deploy/sudoers/andora-agent         /etc/sudoers.d/andora-agent

sudo systemctl daemon-reload
sudo systemctl enable andora-realm andora-monitor-fpm andora-monitor-apache andora-agent
sudo systemctl start andora-realm
sudo systemctl start andora-monitor-fpm
sudo systemctl start andora-monitor-apache
sudo systemctl start andora-agent
```

**Agent nach dem Start prüfen:** `AGENT_TOKEN` in `/opt/andora/agent/config.env`
setzen, sonst verweigert der Agent alle Management-Requests (fail-closed). Bei
Nutzung des Panels über den Agent müssen dieselben Werte in `/opt/andora/monitor/
monitor.conf` stehen: `ANDORA_AGENT_URL`, `ANDORA_AGENT_TOKEN`,
`ANDORA_AGENT_SERVICE_KEY` (Standard `realm`).

**Reihenfolge/Bind-Modus:** Vor dem Start den gewünschten `Listen`-Block in
`/opt/andora/monitor/andora-monitor-site.conf` aktivieren (Standard [1],
`127.0.0.1`). Für Dual-Stack Block [4] verwenden und in `monitor.conf`
`ANDORA_MONITOR_BIND=dual` setzen (Konsistenz Haltegriff, Apache-Listen ist
maßgeblich).

---

## 4. sudoers-Konfiguration (Minimale Rechte)

`deploy/sudoers/andora-monitor` erlaubt **nur** für einen explizit genannten User:

```
pi ALL=(root) NOPASSWD: /usr/bin/systemctl start andora-realm.service, \
                        /usr/bin/systemctl stop andora-realm.service, \
                        /usr/bin/systemctl restart andora-realm.service
```

- Nur diese drei Befehle, nicht `systemctl` allgemein, keine Shell.
- Der User in der Regel muss zum `User=pi` in `andora-monitor-fpm.service` passen.
- Alle anderen sudo-Aufrufe bleiben wie gehabt.

**Andora-Agent** (`deploy/sudoers/andora-agent`): erlaubt `User=andora`
genau diese Befehle (NOPASSWD) für die konfigurierten „Standard"-Units:

```
andora ALL=(root) NOPASSWD: \
  /usr/bin/systemctl start andora-realm.service, ... restart ..., \
  ... andora-coordinator.service ...,
  /usr/bin/journalctl --no-pager --lines [0-9]* -u andora-realm.service, \
  /usr/bin/journalctl --no-pager --lines [0-9]* -u andora-coordinator.service
```

- Exakt die gelisteten Befehle + Units; kein generisches `systemctl`,
  keine Shell. Zeilenzahl für Logs ist serverseitig begrenzt
  (`AGENT_LOG_LINE_LIMIT`, 1–10000).
- **sudoers-Glob-Härtung:** Muster enden auf einem exakten Token
  (`... restart <unit>` bzw. `-u <unit>`); `--lines` steht in der Mitte
  mit reinem Ziffern-Glob `[0-9]*`. Ein nachgestelltes `*` würde in
  sudoers auch weitere Argumente (andere `-u`-Units, `-o`, `--since`)
  erlauben — das wird so verhindert. Der Agent ruft exakt
  `journalctl --no-pager --lines <n> -u <unit>`.
- Der Agent selbst läuft **ohne** `NoNewPrivileges` (sudo/setuid wird
  benötigt); weitere Setuid-Programme werden nicht genutzt.
- Units hier nach Bedarf ergänzen/entfernen (z. B. wenn
  `andora-monitor-{fpm,apache}.service` mit verwaltet werden sollen) —
  sie müssen zur `AGENT_SERVICES`-Liste in `agent.conf` passen.

---

## 5. Verifikation

```bash
# Services laufen?
sudo systemctl status andora-realm andora-monitor-fpm andora-monitor-apache andora-agent

# Sudo-Regel ohne Passwort (sollte "active" liefern):
sudo -n systemctl is-active andora-realm.service

# Health/Status des Gameservers:
curl -s http://127.0.0.1:3002/health
curl -s http://127.0.0.1:3002/status

# Andora-Agent (Management-API, Token setzen!)
curl -s http://127.0.0.1:9443/health
curl -s http://127.0.0.1:9443/status
curl -s -H 'X-Andora-Token: <token>' http://127.0.0.1:9443/api/v1/services
curl -s -H 'X-Andora-Token: <token>' http://127.0.0.1:9443/api/v1/services/realm

# Panel (URL, Bind + Token beachten) — Apache bevorzugt bei Aufruf auf 127.0.0.1:
curl -s http://127.0.0.1:3003/api/status
curl -s http://127.0.0.1:3003/api/config
curl -s http://127.0.0.1:3003/api/history
curl -s http://127.0.0.1:3003/          # Dashboard

# Panel via Agent (Status-Block "agent" + Control via "via": "agent"):
curl -s http://127.0.0.1:3003/api/status | grep -o '"agent":{[^}]*}'
curl -s -X POST -H 'Content-Type: application/json' \
  -d '{"action":"restart","confirm":true}' http://127.0.0.1:3003/api/control

# Dual-Stack: bei Block [4] beide Familien prüfen
curl -s http://127.0.0.1:3003/api/status
curl -s -g "http://[::1]:3003/api/status"
```

**FPM/Config-Pfad beachten:** In Produktion muss `ANDORA_SERVER_CONFIG` in
`monitor.conf` auf die Server-`config.env` zeigen, sonst schreibt
`POST /api/config` in die falsche Datei (Dev-Root-Ableitung).

---

## 6. Rollback (Zurückführen)

```bash
sudo systemctl stop andora-monitor-apache andora-monitor-fpm andora-realm andora-agent
sudo systemctl disable andora-monitor-apache andora-monitor-fpm andora-realm andora-agent
sudo rm /etc/systemd/system/andora-monitor-apache.service /etc/systemd/system/andora-monitor-fpm.service
sudo rm /etc/systemd/system/andora-realm.service
sudo rm /etc/systemd/system/andora-agent.service
sudo rm /etc/sudoers.d/andora-monitor /etc/sudoers.d/andora-agent
sudo rm -r /opt/andora/monitor /opt/andora/server /opt/andora/agent
sudo systemctl daemon-reload
```

---

## Fehlerdiagnose

| Symptom | Ursache / Check |
|---|---|
| Panel zeigt Game-Server als offline, aber `systemctl status` ist aktiv | `ANDORA_GAME_SERVER_URL` / `PORT_HTTP` in `monitor.conf` prüfen |
| `/api/config` POST schreibt falsche Datei | `ANDORA_SERVER_CONFIG` fehlt/zeigt nicht auf die Server-`config.env` |
| Start/Stop/Restart im Panel schlägt fehl | sudo-Regel fehlt oder User-Fehler: `sudo -l -U pi` |
| Panel meldet `agent.enabled=false` | `ANDORA_AGENT_URL` in `monitor.conf` fehlt |
| Status-Block `agent` zeigt `connected=false` | Agent läuft nicht / Token falsch: `curl http://127.0.0.1:9443/health`, `ANDORA_AGENT_TOKEN` abgleichen |
| Agent verweigert alle Management-Requests („fail-closed") | `AGENT_TOKEN` in `/opt/andora/agent/config.env` fehlt oder leer |
| Agent-Aktion im Panel scheitert, falls Unit unbekannt | Services-Registry: `AGENT_SERVICES`/`AGENT_SERVICE_<KEY>_UNIT` in `agent.conf` (auch sudoers-Liste) |
| Apache startet nicht / Port belegt | `Listen`-Block gewählt? `journalctl -u andora-monitor-apache -e` |
| FPM liefert 502 / `proxy:unix`-Socket fehlt | FPM läuft? `ls -l /run/andora-monitor/php-fpm.sock`, `journalctl -u andora-monitor-fpm -e` |
| Panel meldet leere History | `data/history.json` erst nach erstem `/api/status`; Rechte auf `data/` prüfen |
| Config-Änderung greift nicht ohne Neustart | Erwartung: Die meisten Werte lesen beim Start; `restart` ausgeben |
