# Andora

Andora ist ein Fantasy-MMORPG mit einer isometrischen Spielwelt. Der Client wird mit Godot 3.5 entwickelt und ist von Anfang an auf eine ressourcenschonende Darstellung ausgelegt. Primäre Referenzplattform ist der Raspberry Pi 4 (8 GB). Der Server läuft auf einem separaten x86-Linux-System.

## Technik

* **Client:** Godot 3.5 (GLES2), optimiert für den Raspberry Pi
* **Server:** Node.js 20 + TypeScript
* **Datenbank:** MariaDB 10
* **Netzwerk:** WebSocket, serverautoritativ, 10-Hz-World-Tick
* **Lokalisierung:** Deutsch und Englisch von Anfang an, später erweiterbar
* **KI:** optionale Anbindung lokaler KI-Dienste für dafür vorgesehene Spielsysteme

## Verzeichnisse

* `src/` – Server-Dienste: `realm/` (Node.js-/TypeScript-Server), `api/` (Go-API-/Security-Service), `login/` und `coordinator/` (Struktur vorgesehen, noch ohne Code)
* `client/` – Godot-Client
* `shared/` – gemeinsames Netzwerkprotokoll und Definitionen
* `i18n/` – Übersetzungen und Lokalisierung
* `monitor/` – Monitoring- und Admin-Werkzeuge
* `deploy/` – Deployment-Konfiguration und systemd-Vorlagen
* `docs/` – Architektur-, System- und Spieldesign-Dokumentation

## Serverarchitektur

Andora trennt unterschiedliche Verantwortungsbereiche bewusst voneinander.

Die persistente Datenhaltung ist in mehrere logische Bereiche gegliedert:

* `auth` – Accounts, Authentifizierung, Sessions und Serverautorisierung
* `character` – persistente Charakterdaten
* `world_data` – statische und versionierte Weltdefinitionen
* `realm_state_<realm>` – persistenter Zustand eines einzelnen Realms

Der Realm-/World-Server besitzt keinen direkten Zugriff auf die Auth-Datenbank. Authentifizierungsfunktionen werden über eine getrennte Auth/API-Grenze bereitgestellt.

## Start des Clients

Das Projekt im Verzeichnis `client/` mit Godot 3.5 öffnen und ausführen.

Die Lokalisierung wird clientseitig über das dafür vorgesehene i18n-System bereitgestellt.

## Start des Servers

```bash
cd src/realm
cp config.env.example config.env
npm install
npm run dev
```

`config.env` enthält die lokale Serverkonfiguration und darf keine produktiven Zugangsdaten in das Git-Repository übertragen.

## Monitoring & Admin-Panel

Das Monitoring-System ist ausschließlich für Administration und Betrieb vorgesehen und nicht Bestandteil des Spielerclients.

```bash
cd monitor
node server.js
```

Der Gameserver stellt Monitoring- und Health-Endpunkte bereit. Das separate Admin-Panel kann diese Informationen für Diagnose und Serverbetrieb darstellen.

Produktionsbezogene Konfigurationen und Vorlagen befinden sich unter `deploy/`.

Weitere technische Details befinden sich in der Dokumentation unter `docs/`.

## Entwicklungsprinzip

Andora wird serverautoritativ entwickelt. Der Server bestimmt den verbindlichen Zustand der Spielwelt; der Client stellt diesen Zustand dar und verarbeitet die Benutzereingaben.

Neue Systeme werden modular entwickelt und sollen vorhandene Komponenten wiederverwenden, anstatt parallele Implementierungen aufzubauen.

## Lizenz

MIT
