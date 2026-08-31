# Deploy-Anleitung: Andora-Monitoring (Vorlagen in diesem Ordner)

Diese Dateien sind **Vorlagen**. Sie werden manuell auf dem Produktionsserver
installiert. Auf dem Entwicklungsrechner wurde **nichts** installiert.

> Produktions-Root-Beispiel (anpassen!): `/opt/andora`
> - `/opt/andora/server`   = das komplette `server/`-Verzeichnis (inkl. `build/`, `config.env`)
> - `/opt/andora/monitor`  = das komplette `monitor/`-Verzeichnis
> - Das Repository selbst bleibt auf dem Dev-Rechner; nur Build-Artefakte landen bei Produktion.

---

## 1. Welche Datei wohin kopieren

| Datei in diesem Projekt | Produktionsziel |
|---|---|
| `server/` (inkl. `build/`, `config.env`) | `/opt/andora/server/` |
| `monitor/` | `/opt/andora/monitor/` |
| `deploy/systemd/andora-server.service` | `/etc/systemd/system/andora-server.service` |
| `deploy/systemd/andora-monitor.service` | `/etc/systemd/system/andora-monitor.service` |
| `deploy/conf/monitor.conf` | `/opt/andora/monitor/monitor.conf` |
| `deploy/sudoers/andora-monitor` | `/etc/sudoers.d/andora-monitor` |

Vorher auf dem Zielsystem:
```bash
su - andora   # Zieluser anlegen (nicht root!)
sudo useradd -m andora
```

---

## 2. Besitzer & Dateirechte

```bash
# Game-Server (läuft als User "andora")
sudo chown -R andora:andora /opt/andora/server
sudo chmod 755 /opt/andora/server
# config.env enthält DB-Passwort/Token -> nur für "andora" lesbar
sudo chmod 600 /opt/andora/server/config.env

# Monitoring-Panel (läuft als User "pi")
sudo chown -R pi:pi /opt/andora/monitor
sudo chmod 755 /opt/andora/monitor
# monitor.conf enthält evtl. Token -> nur für "pi" lesbar
sudo chmod 600 /opt/andora/monitor/monitor.conf

# systemd-Units: root-eigenn, standard Rechte
sudo chown root:root /etc/systemd/system/andora-server.service /etc/systemd/system/andora-monitor.service
sudo chmod 644 /etc/systemd/system/andora-server.service /etc/systemd/system/andora-monitor.service

# sudoers-Datei: root-eigenn, genau 0440 (wichtig!)
sudo chown root:root /etc/sudoers.d/andora-monitor
sudo chmod 0440 /etc/sudoers.d/andora-monitor
# Syntax-Check:
sudo visudo -cf /etc/sudoers.d/andora-monitor
```

---

## 3. systemd-Befehle (Installation + Aktivierung)

```bash
sudo cp deploy/systemd/andora-server.service  /etc/systemd/system/andora-server.service
sudo cp deploy/systemd/andora-monitor.service /etc/systemd/system/andora-monitor.service
sudo cp deploy/conf/monitor.conf              /opt/andora/monitor/monitor.conf
sudo cp deploy/sudoers/andora-monitor         /etc/sudoers.d/andora-monitor

sudo systemctl daemon-reload
sudo systemctl enable andora-server andora-monitor
sudo systemctl start andora-server
sudo systemctl start andora-monitor
```

---

## 4. sudoers-Konfiguration (Minimale Rechte)

`deploy/sudoers/andora-monitor` erlaubt **nur** für einen explizit genannten User:

```
pi ALL=(root) NOPASSWD: /usr/bin/systemctl start andora-server.service, \
                        /usr/bin/systemctl stop andora-server.service, \
                        /usr/bin/systemctl restart andora-server.service
```

- Nur diese drei Befehle, nicht `systemctl` allgemein, keine Shell.
- Der User in der Regel muss zum `User=pi` in `andora-monitor.service` passen.
- Alle anderen sudo-Aufrufe bleiben wie gehabt.

---

## 5. Verifikation

```bash
# Services laufen?
sudo systemctl status andora-server andora-monitor

# Sudo-Regel ohne Passwort (sollte "active" liefern):
sudo -n systemctl is-active andora-server.service

# Health/Status des Gameservers:
curl -s http://127.0.0.1:3002/health
curl -s http://127.0.0.1:3002/status

# Panel (URL + Token beachten):
curl -s http://127.0.0.1:3003/api/status
curl -s http://127.0.0.1:3003/api/config
```

---

## 6. Rollback (Zurückführen)

```bash
sudo systemctl stop andora-monitor andora-server
sudo systemctl disable andora-monitor andora-server
sudo rm /etc/systemd/system/andora-monitor.service /etc/systemd/system/andora-server.service
sudo rm /etc/sudoers.d/andora-monitor
sudo rm -r /opt/andora/monitor /opt/andora/server
sudo systemctl daemon-reload
```

---

## Fehlerdiagnose

| Symptom | Ursache / Check |
|---|---|
| Panel zeigt Game-Server als offline, aber `systemctl status` ist aktiv | `ANDORA_GAME_SERVER_URL` / `PORT_HTTP` in `monitor.conf` prüfen |
| Start/Stop/Restart im Panel schlägt fehl | sudo-Regel fehlt oder User-Fehler: `sudo -l -U pi` |
| Panel startet nicht | `journalctl -u andora-monitor -e`, um `monitor.conf` / Bind-Port prüfen |
| Config-Änderung greift nicht ohne Neustart | Erwartung: Die meisten Werte lesen beim Start; `restart` ausgeben |
