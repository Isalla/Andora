# Deploy-Anleitung: Andora-Monitoring (Vorlagen in diesem Ordner)

Diese Dateien sind **Vorlagen**. Sie werden manuell auf dem Produktionsserver
installiert. Auf dem Entwicklungsrechner wurde **nichts** installiert.

> Produktions-Root-Beispiel (anpassen!): `/opt/andora`
> - `/opt/andora/server`    = das komplette `src/realm/`-Verzeichnis (inkl. `build/`, `config.env`)
> - `/opt/andora/monitor`   = das komplette `web/andora-monitor/`-Verzeichnis (PHP-Panel)
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
| `src/realm/` (inkl. `build/`, `config.env`) | `/opt/andora/server/` |
| `web/andora-monitor/` (PHP-Panel) | `/opt/andora/monitor/` |
| `deploy/systemd/andora-monitor-fpm.service` | `/etc/systemd/system/andora-monitor-fpm.service` |
| `deploy/systemd/andora-monitor-apache.service` | `/etc/systemd/system/andora-monitor-apache.service` |
| `deploy/systemd/andora-server.service` | `/etc/systemd/system/andora-server.service` |
| `deploy/conf/monitor.conf` | `/opt/andora/monitor/monitor.conf` |
| `deploy/conf/monitor-fpm.conf` | `/opt/andora/monitor/monitor-fpm.conf` |
| `deploy/conf/monitor-apache.conf` | `/opt/andora/monitor/monitor-apache.conf` |
| `deploy/conf/andora-monitor-site.conf` | `/opt/andora/monitor/andora-monitor-site.conf` |
| `deploy/sudoers/andora-monitor` | `/etc/sudoers.d/andora-monitor` |

Vorher auf dem Zielsystem (Pakete):
```bash
sudo apt install php8.2-fpm apache2 libapache2-mod-proxy-fcgi php8.2-curl
# Apache-Module für den Proxy ins Global-Config laden (siehe monitor-apache.conf):
sudo a2enmod proxy proxy_fcgi
```

---

## 2. Besitzer & Dateirechte

```bash
# Game-Server (läuft als User "andora")
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

# systemd-Units: root-eigenn, standard Rechte
sudo chown root:root /etc/systemd/system/andora-monitor-*.service /etc/systemd/system/andora-server.service
sudo chmod 644 /etc/systemd/system/andora-monitor-*.service /etc/systemd/system/andora-server.service

# sudoers-Datei: root-eigenn, genau 0440 (wichtig!)
sudo chown root:root /etc/sudoers.d/andora-monitor
sudo chmod 0440 /etc/sudoers.d/andora-monitor
sudo visudo -cf /etc/sudoers.d/andora-monitor   # Syntax-Check
```

---

## 3. systemd-Befehle (Installation + Aktivierung)

```bash
sudo cp deploy/systemd/andora-server.service            /etc/systemd/system/andora-server.service
sudo cp deploy/systemd/andora-monitor-fpm.service       /etc/systemd/system/andora-monitor-fpm.service
sudo cp deploy/systemd/andora-monitor-apache.service    /etc/systemd/system/andora-monitor-apache.service
sudo cp deploy/conf/monitor.conf monitor-fpm.conf monitor-apache.conf andora-monitor-site.conf /opt/andora/monitor/
sudo cp deploy/sudoers/andora-monitor       /etc/sudoers.d/andora-monitor

sudo systemctl daemon-reload
sudo systemctl enable andora-server andora-monitor-fpm andora-monitor-apache
sudo systemctl start andora-server
sudo systemctl start andora-monitor-fpm
sudo systemctl start andora-monitor-apache
```

**Reihenfolge/Bind-Modus:** Vor dem Start den gewünschten `Listen`-Block in
`/opt/andora/monitor/andora-monitor-site.conf` aktivieren (Standard [1],
`127.0.0.1`). Für Dual-Stack Block [4] verwenden und in `monitor.conf`
`ANDORA_MONITOR_BIND=dual` setzen (Konsistenz Haltegriff, Apache-Listen ist
maßgeblich).

---

## 4. sudoers-Konfiguration (Minimale Rechte)

`deploy/sudoers/andora-monitor` erlaubt **nur** für einen explizit genannten User:

```
pi ALL=(root) NOPASSWD: /usr/bin/systemctl start andora-server.service, \
                        /usr/bin/systemctl stop andora-server.service, \
                        /usr/bin/systemctl restart andora-server.service
```

- Nur diese drei Befehle, nicht `systemctl` allgemein, keine Shell.
- Der User in der Regel muss zum `User=pi` in `andora-monitor-fpm.service` passen.
- Alle anderen sudo-Aufrufe bleiben wie gehabt.

---

## 5. Verifikation

```bash
# Services laufen?
sudo systemctl status andora-server andora-monitor-fpm andora-monitor-apache

# Sudo-Regel ohne Passwort (sollte "active" liefern):
sudo -n systemctl is-active andora-server.service

# Health/Status des Gameservers:
curl -s http://127.0.0.1:3002/health
curl -s http://127.0.0.1:3002/status

# Panel (URL, Bind + Token beachten) — Apache bevorzugt bei Aufruf auf 127.0.0.1:
curl -s http://127.0.0.1:3003/api/status
curl -s http://127.0.0.1:3003/api/config
curl -s http://127.0.0.1:3003/api/history
curl -s http://127.0.0.1:3003/          # Dashboard

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
sudo systemctl stop andora-monitor-apache andora-monitor-fpm andora-server
sudo systemctl disable andora-monitor-apache andora-monitor-fpm andora-server
sudo rm /etc/systemd/system/andora-monitor-apache.service /etc/systemd/system/andora-monitor-fpm.service
sudo rm /etc/systemd/system/andora-server.service
sudo rm /etc/sudoers.d/andora-monitor
sudo rm -r /opt/andora/monitor /opt/andora/server
sudo systemctl daemon-reload
```

---

## Fehlerdiagnose

| Symptom | Ursache / Check |
|---|---|
| Panel zeigt Game-Server als offline, aber `systemctl status` ist aktiv | `ANDORA_GAME_SERVER_URL` / `PORT_HTTP` in `monitor.conf` prüfen |
| `/api/config` POST schreibt falsche Datei | `ANDORA_SERVER_CONFIG` fehlt/zeigt nicht auf die Server-`config.env` |
| Start/Stop/Restart im Panel schlägt fehl | sudo-Regel fehlt oder User-Fehler: `sudo -l -U pi` |
| Apache startet nicht / Port belegt | `Listen`-Block gewählt? `journalctl -u andora-monitor-apache -e` |
| FPM liefert 502 / `proxy:unix`-Socket fehlt | FPM läuft? `ls -l /run/andora-monitor/php-fpm.sock`, `journalctl -u andora-monitor-fpm -e` |
| Panel meldet leere History | `data/history.json` erst nach erstem `/api/status`; Rechte auf `data/` prüfen |
| Config-Änderung greift nicht ohne Neustart | Erwartung: Die meisten Werte lesen beim Start; `restart` ausgeben |
