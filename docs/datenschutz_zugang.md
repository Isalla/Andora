# Datenschutz / Zugang

Da IP-, Account-, Chat- und Moderationsdaten sicherheits- und teilweise
personenbezogene Informationen enthalten können:

---

## Zugriff rollenbasiert begrenzen

Nur erforderliche Daten anzeigen.

---

## Securitydaten nicht unnötig dem gesamten Supportteam zugänglich machen

Supportteam benötigt nur Informationen, die zur Bearbeitung des
konkreten Falls erforderlich sind.

---

## sicherheitsrelevante Aktionen nachvollziehbar protokollieren

Alle sicherheitsrelevanten Aktionen sind nachvollziehbar zu protokollieren.

---

## Aufbewahrungskonzepte

Aufbewahrungskonzepte vorsehen.

---

## Keine konkreten gesetzlichen Aufbewahrungsfristen

Erfinden. Wenn die bestehende Dokumentation hierzu bereits Regeln enthält,
diese beachten und nicht widersprechen.

---

## Connection-Takeover-Logs

Für Logs authentifizierter Connection-Takeovers gilt:

- Zweck ist ausschließlich die Account- und Serversicherheit sowie die Untersuchung konkreter Sicherheitsvorfälle.
- Spielertracking, Werbung und eine allgemeine Spielerverhaltensbewertung sind ausdrücklich ausgeschlossen.
- Zugriff auf diese Logs erhalten nur berechtigte Administratoren.
- Roh-IP-Adressen und mit Accounts verknüpfte IP-Daten aus diesen Takeover-Ereignissen werden nach 14 Tagen automatisch gelöscht.
- Vollständige Session-IDs, Handoff-Tokens, Passwörter und andere Zugangsdaten dürfen nicht protokolliert werden.
- Der Ereignisname lautet `authenticated_connection_takeover`.
- Erfasst werden Zeitstempel, Account-ID, Charakter-ID, alte und neue `conn_id`, alte und neue Quell-IP sowie der Zustand der alten Verbindung.
- Ein einzelner Takeover ist ein normales INFO-Ereignis.
- Auffällige Wiederholungen dürfen einen WARN-/Alarmhinweis erzeugen, führen aber nicht automatisch zu einer Sanktion.
- Die Frist von 14 Tagen gilt ausschließlich für die hier beschriebenen Connection-Takeover-Daten. Daraus folgt keine allgemeine IP-Aufbewahrungsfrist.
