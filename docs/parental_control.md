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

## Optionale Eltern-E-Mail

Die Angabe einer Eltern-E-Mail ist **optional**.

Eine Elternkontrolle kann ohne E-Mail eingerichtet und betrieben werden; die Eltern-E-Mail ist keine Voraussetzung für den Elternzugang.

Die Eltern-E-Mail gehört zur separaten Elternkontrolle und nicht zum normalen Account des Kindes. Sie wird unabhängig von den Accountdaten des Kindes gespeichert.

### Benachrichtigungen

Ist eine Eltern-E-Mail hinterlegt, werden die Eltern über Änderungen an den Einstellungen der Elternkontrolle informiert.

Auch bei sicherheitskritischen Änderungen wird eine Benachrichtigung versendet. Dazu zählen insbesondere:

* eine Änderung der Eltern-PIN,
* eine Änderung der Eltern-E-Mail,
* das Entfernen der Eltern-E-Mail,
* das vollständige Entfernen der Elternkontrolle.

Wird die Eltern-E-Mail geändert oder entfernt, erhält die bisherige E-Mail-Adresse eine Benachrichtigung.

Wird die Elternkontrolle vollständig entfernt, erhält die bisher hinterlegte Eltern-E-Mail eine letzte Benachrichtigung.

### Inhalt der Benachrichtigung

Eine Änderungsbenachrichtigung soll nachvollziehbar machen, was geändert wurde. Sie enthält insbesondere:

* welche Einstellung geändert wurde,
* den alten Wert,
* den neuen Wert,
* den Zeitpunkt der Änderung.

Bei einer Änderung der Eltern-PIN enthält die Benachrichtigung keine PIN.

### Keine Geheimnisse per E-Mail

Per E-Mail dürfen keine PINs, Tokens oder andere Geheimnisse versendet werden.

Die Eltern-E-Mail dient ausschließlich der Information über Änderungen an der Elternkontrolle.

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

## BLOCKED und Beginn eines Puffers

Ist der aktuelle Tag bereits beim Login BLOCKED, wird der Spieleinstieg verweigert.

Wird ein bereits eingeloggter Spieler durch eine Änderung der Elternregeln auf BLOCKED gesetzt, beginnt der reguläre Puffer.

Für den Puffer gelten die bereits definierten 15 Minuten Sonntag–Donnerstag bzw. 30 Minuten Freitag–Samstag.

Der Puffer ist ausschließlich eine Auslaufzeit der bereits bestehenden Sitzung.

Endet die Sitzung während des Puffers – beispielsweise durch freiwilliges Beenden, Client-Absturz, Disconnect oder Verbindungsverlust –, ist kein erneuter Login während dieses Puffers möglich.

Nach Ablauf des Puffers erfolgt der erzwungene Logout, falls die Sitzung noch besteht.

Diese Regel zum fehlenden Re-Login gilt entsprechend auch beim normalen Ende eines TIME_WINDOW.

Ein Logout oder Disconnect während des Puffers darf niemals einen neuen Puffer oder zusätzliche Spielzeit erzeugen.

Der Puffer gehört zur bestehenden Sitzung und stellt keine zusätzliche Loginberechtigung dar. Wird die bestehende Sitzung während des Puffers beendet oder unterbrochen, ist kein erneuter Spieleinstieg möglich. Insbesondere darf ein Reconnect keinen neuen Puffer starten oder die verbleibende Spielzeit zurücksetzen.

**WIRKUNG EINES VERWEIGERTEN EINSTIEGS**

Wird ein Spieleinstieg verweigert — insbesondere weil der Tag bereits beim Login `BLOCKED` ist oder weil die Elternkontrolle beim Einstieg nicht verfügbar ist —, zählt dieser fehlgeschlagene Einstieg fachlich **nicht** als zustande gekommener Realm-Login.

VERBINDLICH gilt deshalb:

* Der Einstieg wird nicht durch die Elternkontrolle blockiert und der Charakter dennoch teilweise in den Realm aufgenommen; die Ablehnung bleibt vollständig und fail-closed.
* Die Offline-Zeit des Charakters wird dabei **nicht** verbraucht und dem Rested-Pool wird **nichts** gutgeschrieben. Der gespeicherte Logout-Zeitpunkt und der Offline-Zeitraum bleiben für den nächsten erfolgreichen Einstieg erhalten (siehe `Erfahrung_und_Progressionssystem.md`, Abschnitt 12.6, und `Player_Persistenz.md`, Abschnitt 11).
* Die Elternkontrolle verbraucht durch eine verweigerte Anmeldung **keine** Spielzeit und erzeugt **keinen** neuen Puffer; die bestehende Pufferregel (kein Re-Login während des Puffers) bleibt davon unberührt.
* Ob und wie die technische Umsetzung die erhaltene Offline-Zeit sicherstellt, ist nicht Gegenstand dieses Dokuments; der dazu offene sicherheitstechnische Befund steht in `docs/Security.md` unter `P-32`.

---

## Temporäre Session-Ausnahmen

Das Ingame-Elternpanel ist bewusst eingeschränkt und ersetzt nicht das vollständige Elternpanel.

Nach Eingabe des Eltern-PINs dürfen Eltern dort:

* die aktuelle Spielzeit einmal pro Kalendertag um genau eine Stunde verlängern,
* einzelne durch die Elternkontrolle regelbare Mechanismen temporär freischalten.

Die einstündige Verlängerung:

* kann pro Account nur einmal pro Kalendertag verwendet werden,
* wird serverseitig gespeichert,
* richtet sich nach der für die Elternkontrolle maßgeblichen Realm-/Server-Zeitzone,
* kann nicht durch Logout, Reconnect, Client-Neustart oder Gerätewechsel erneut verfügbar gemacht werden,
* verändert keine permanenten Wochen- oder Sonderregeln.

Das Ingame-Elternpanel darf keine permanenten Zeitpläne oder sonstigen dauerhaften Elternregeln verändern.

Temporäre Mechanismus-Freischaltungen gelten nur für die aktuelle Sitzung und verfallen mit deren Ende.

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

## Berechtigungen in Sonderzeiträumen

Sonderzeiträume dürfen grundsätzlich alle Mechanismen abweichend festlegen, die überhaupt durch die Elternkontrolle regelbar sind.

* Sie dürfen keine Funktionen verändern, die nicht Bestandteil der Elternkontrolle sind.
* Während eines aktiven Sonderzeitraums gelten dessen Zeit- und Berechtigungsregeln anstelle der entsprechenden normalen Regeln.
* Nach Ende des Sonderzeitraums gelten automatisch wieder die normalen Regeln.
* Da aktive Sonderzeiträume nicht überlappen dürfen, ist keine zusätzliche Prioritätslogik zwischen mehreren gleichzeitig aktiven Sonderzeiträumen erforderlich.

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
- parent_email (optional)
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
* Handel,
* Auktionshaus,
* Gildenbeitritt,
* Käufe (ausschließlich Ingame-Käufe mit Ingame-Währung),
* weitere soziale Funktionen.

Spielbezogene Echtgeldfunktionen (Kauf von Inhalt, Fortschritt, Währung oder sonstigen Vorteilen gegen Echtgeld) existieren in Andora nicht und sind als Thema der Elternkontrolle nicht vorgesehen (verbindlich in `Monetarisierung_und_Donations.md`). Die spätere, auf der offiziellen Andora-Website geplante freiwillige Donation-Möglichkeit ist eine nicht-spielbezogene Unterstützungsleistung und gehört ebenfalls nicht zu den Regeln der Elternkontrolle.

Diese Funktionen gehören noch nicht zwingend zur ersten Implementierung.

Die Architektur soll jedoch verhindern, dass solche Regeln später ausschließlich clientseitig umgesetzt werden müssen.

---

## Voice in der Elternkontrolle

Das Voice-System selbst gehört nicht zum aktuellen Releaseumfang und wird erst später implementiert.

Die Voice-Berechtigung ist jedoch bereits heute Bestandteil der Elternkontrolle.

Die Elternkontrolle muss daher bereits für die Berechtigung von Spieler-Voice vorsehen.

* Die elterliche Voice-Sperre betrifft die Sprachkommunikation zwischen Spielern (Spieler-zu-Spieler-Voice).
* Sprachinteraktion mit NPCs/KI sowie Sprachbefehle für Begleiter/Söldner sind davon getrennt und bleiben grundsätzlich verfügbar.
* Die Details des späteren Voice-Systems stehen in [voice_system.md](./voice_system.md).

---

## Sicherheitsgrundsatz

Die Elternkontrolle darf nicht davon abhängen, wie technisch versiert Eltern oder Kinder sind.

> **Sicherheit muss auch dann funktionieren, wenn der Spieler nichts von IT-Sicherheit versteht.**

Der Client stellt die Bedienoberfläche bereit.

Die verbindliche Entscheidung und Durchsetzung erfolgt auf dem Server.
