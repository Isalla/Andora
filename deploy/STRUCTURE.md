# deploy — Vorlagen für die Produktion-Installation des Andora-Monitorings
#
# Dies sind KEINE Dateien, die automatisch installiert werden. Sie sind
# Vorlagen, die manuell auf dem Produktionsserver kopiert werden.
# Siehe deploy/README.md für Installation, Rechte, systemd-Befehle,
# sudoers-Konfiguration, Verifikation und Rollback.

## Struktur
deploy/
├── systemd/
│   ├── andora-server.service        # Game-Server-Dienst (systemd, Restart nach Crash)
│   ├── andora-monitor-fpm.service   # PHP-Panel: dedizierter PHP-FPM-Master (Nicht-root)
│   ├── andora-monitor-apache.service# PHP-Panel: dedizierte Apache-Instance, Port 3003
│   └── andora-agent.service         # Andora-Agent: lokaler Verwaltungsdaemon (Port 9443)
├── conf/
│   ├── monitor.conf                 # EnvironmentFile des Panels (Ports, Token, Zielpfade, Agent)
│   ├── monitor-fpm.conf             # PHP-FPM-Master/-Pool (Unix-Socket, own-root)
│   ├── monitor-apache.conf          # Apache-Main-Config der Panel-Instance
│   ├── andora-monitor-site.conf     # Apache-Vhost + explizite Listen (Bind/Dual-Stack)
│   └── agent.conf                   # Konfigurationsvorbild des Agent -> /opt/andora/agent/config.env
├── sudoers/
│   ├── andora-monitor               # minimale, passwordlose sudo-Regel für den Panel-User
│   └── andora-agent                 # sudo-Regel für den Agent (systemctl/journalctl, feste Units)
└── README.md