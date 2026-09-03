# Elternkontrolle

## Überblick

Die Elternkontrolle von Andora ist **accountgebunden und serverseitig erzwungen**.

Sie darf nicht ausschließlich im Client gespeichert oder ausgewertet werden. Dadurch gelten die Regeln unabhängig davon, ob sich das Kind:

* am eigenen Computer,
* an einem anderen Computer,
* bei Freunden,
* über einen Browser,
* oder später über eine andere Andora-Oberfläche

anmeldet.

Grundprinzip:

> **Eltern bedienen – der Server entscheidet – der Account trägt die Regeln.**

Die Eltern müssen dafür keinen normalen Andora-Spielaccount besitzen.

---

## Elternzugang

Im Andora-Client befindet sich ein Bereich:

**Elternkontrolle**

Dort können Eltern für einen bestehenden Kinderaccount einen Elternzugang einrichten.

Der Elternzugang dient ausschließlich zur Verwaltung der Elternkontrolle und ist kein Spielaccount.

Für den Zugriff wird mindestens eine Eltern-PIN eingerichtet.

Die PIN darf nicht im Klartext gespeichert werden.

Die Eltern-PIN dient nur zur Autorisierung von Änderungen. Sie ersetzt nicht die serverseitige Durchsetzung der Regeln.

---

## Accountbindung

Ein Account erhält einen serverseitigen Status, ob eine Elternkontrolle aktiv ist.

Beispiel:

```text
parental_control_enabled = true
```

Die eigentlichen Regeln werden nicht vollständig in der normalen Account-Tabelle gespeichert, sondern in eigenen Tabellen für die Elternkontrolle.

Dadurch bleibt die Account-Struktur übersichtlich und das System kann später erweitert werden.

---

## Grundregeln

Eltern können für jeden Wochentag ein eigenes tägliches Spielzeitlimit festlegen.

Beispiel:

```text
Montag      60 Minuten
Dienstag    60 Minuten
Mittwoch    60 Minuten
Donnerstag  60 Minuten
Freitag     90 Minuten
Samstag    180 Minuten
Sonntag    120 Minuten
```

Die Spielzeit wird serverseitig erfasst.

Eine Änderung der lokalen Systemzeit oder ein anderer Computer darf keinen Einfluss auf das verfügbare Zeitbudget haben.

---

## 30-Minuten-Warnung

Wenn nur noch 30 Minuten Spielzeit verfügbar sind, muss der Client einen gut sichtbaren Countdown anzeigen.

Beispiel:

```text
Verbleibende Spielzeit heute: 29:43
```

Dieser Countdown bleibt sichtbar und zählt bis zum Ende der verfügbaren Spielzeit herunter.

Zusätzliche deutlichere Hinweise können beispielsweise bei:

* 15 Minuten
* 5 Minuten
* 1 Minute

angezeigt werden.

Die maßgebliche verbleibende Spielzeit kommt vom Server. Der Client stellt sie lediglich dar.

---

## Ablauf der Spielzeit

Ist das tägliche Zeitbudget aufgebraucht, darf der Spieler nicht einfach unbegrenzt weiterspielen.

Der Server entscheidet über das Ende der erlaubten Spielzeit.

Der Client erhält rechtzeitig vorher entsprechende Warnungen.

Später kann zusätzlich definiert werden, wie Andora mit besonderen Situationen umgeht, beispielsweise:

* laufender Kampf,
* Dungeon,
* Raid,
* Handel,
* andere nicht sofort abbrechbare Aktionen.

Eine Verlängerung darf jedoch niemals allein durch den Client vorgenommen werden.

---

## Tagesausnahmen

Eltern können für einzelne Tage eine Ausnahme festlegen.

Das kann beispielsweise sinnvoll sein bei:

* Feiertagen,
* Familienbesuch,
* besonderen Anlässen,
* schulfreien Tagen.

Mögliche Ausnahmen:

```text
+60 Minuten
+120 Minuten
anderes Tageslimit
Limit für diesen Tag aufheben
```

Die Ausnahme gilt ausschließlich für den festgelegten Tag.

Danach gelten automatisch wieder die normalen Regeln.

---

## Ferienregeln

Eltern können eigene Zeiträume definieren, in denen andere Spielzeitregeln gelten.

Andora verwendet dafür keinen automatisch gepflegten Schulferienkalender.

Die Eltern bestimmen selbst:

* Beginn des Zeitraums,
* Ende des Zeitraums,
* tägliche Spielzeit innerhalb dieses Zeitraums.

Beispiel:

```text
Ferienregel

Von: 12.10.2026
Bis: 24.10.2026

Montag–Freitag: 180 Minuten
Samstag:        300 Minuten
Sonntag:        240 Minuten
```

Eltern müssen nicht die gesamten offiziellen Schulferien freigeben.

Sie können beispielsweise bei sechs Wochen Sommerferien nur drei Wochen mit erweiterten Spielzeiten versehen.

Nach Ablauf des eingetragenen Zeitraums gelten automatisch wieder die normalen Wochenregeln.

---

## Priorität der Regeln

Für die Berechnung der erlaubten Spielzeit gilt folgende Reihenfolge:

```text
Normale Wochenregel
        ↓
aktive Ferienregel
        ↓
Tagesausnahme
```

Eine Tagesausnahme hat damit die höchste Priorität.

---

## Serverseitige Durchsetzung

Der Client darf niemals selbst entscheiden, ob die Elternkontrolle aktiv ist oder wie viel Zeit noch verfügbar ist.

Beim Login und während des Spielens prüft der Server den Account.

Beispiel:

```text
Login
↓
Auth/API prüft Account
↓
parental_control_enabled?
↓
Regeln laden
↓
verfügbare Spielzeit bestimmen
↓
Realm setzt Regeln durch
↓
Client zeigt verbleibende Zeit
```

Dadurch kann ein Kind die Elternkontrolle nicht umgehen, indem es:

* den Client neu installiert,
* lokale Dateien löscht,
* einen anderen Computer verwendet,
* beim Freund spielt,
* einen anderen Andora-Client verwendet,
* auf eine Browser-Oberfläche wechselt,
* die lokale Uhr verändert.

---

## Datenstruktur

Die genaue Datenbankstruktur wird bei der Implementierung festgelegt.

Grundsätzlich sollen die Informationen getrennt von den normalen Accountdaten gespeichert werden.

Beispiel:

```text
accounts
- id
- parental_control_enabled
```

Separate Elternkontrolle:

```text
parental_controls
- account_id
- monday_minutes
- tuesday_minutes
- wednesday_minutes
- thursday_minutes
- friday_minutes
- saturday_minutes
- sunday_minutes
- warning_minutes
- enabled
```

Ferienzeiträume:

```text
parental_control_periods
- id
- account_id
- starts_at
- ends_at
- monday_minutes
- tuesday_minutes
- wednesday_minutes
- thursday_minutes
- friday_minutes
- saturday_minutes
- sunday_minutes
```

Tagesausnahmen:

```text
parental_control_exceptions
- id
- account_id
- date
- extra_minutes
- override_minutes
```

Die tatsächliche Datenbankstruktur darf bei der Implementierung verbessert werden, solange die hier definierten Regeln und Sicherheitsgrenzen erhalten bleiben.

---

## Kinderaccount bleibt normaler Account

Ein beaufsichtigter Spieler besitzt keinen speziellen Spielstand, der später ersetzt werden muss.

Charaktere, Inventar, Erfolge, Freunde, Gilde und sonstiger Fortschritt gehören weiterhin zum normalen Andora-Account.

Die Elternkontrolle ist lediglich eine zusätzliche Verwaltungsschicht.

Wenn die Elternkontrolle später nicht mehr benötigt wird, kann sie kontrolliert entfernt werden.

Der Spieler behält dabei vollständig:

* seinen Account,
* seine Charaktere,
* seine Gegenstände,
* seine Erfolge,
* seine sozialen Verbindungen,
* seinen gesamten Spielfortschritt.

Es muss kein neuer Account erstellt werden.

---

## Zukünftige Erweiterungen

Die Elternkontrolle soll später erweiterbar bleiben.

Mögliche zusätzliche Regeln sind:

* Chat-Einschränkungen,
* private Nachrichten,
* Freundesanfragen,
* Voice-Chat,
* Handel,
* Auktionshaus,
* Gildenbeitritt,
* Käufe oder Echtgeldfunktionen,
* weitere soziale Funktionen.

Diese Funktionen gehören noch nicht zwingend zur ersten Implementierung.

Die Architektur soll jedoch verhindern, dass solche Regeln später ausschließlich clientseitig umgesetzt werden müssen.

---

## Sicherheitsgrundsatz

Die Elternkontrolle darf nicht davon abhängen, wie technisch versiert Eltern oder Kinder sind.

> **Sicherheit muss auch dann funktionieren, wenn der Spieler nichts von IT-Sicherheit versteht.**

Der Client stellt die Bedienoberfläche bereit.

Die verbindliche Entscheidung und Durchsetzung erfolgt auf dem Server.
