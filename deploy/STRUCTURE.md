# deploy — Vorlagen für die Produktion-Installation des Andora-Monitorings
#
# Dies sind KEINE Dateien, die automatisch installiert werden. Sie sind
# Vorlagen, die manuell auf dem Produktionsserver kopiert werden.
# Siehe deploy/README.md für Installation, Rechte, systemd-Befehle,
# sudoers-Konfiguration, Verifikation und Rollback.

## Struktur
deploy/
├── systemd/
│   ├── andora-server.service     # Game-Server-Dienst (systemd, Restart nach Crash)
│   └── andora-monitor.service    # Panel-Dienst (systemd, eigener Prozess, nicht root)
├── conf/
│   └── monitor.conf              # EnvironmentFile des Panels (Ports, Token, Zielpath)
├── sudoers/
│   └── andora-monitor            # minimale, passwordlose sudo-Regel für den Panel-User
└── README.md
