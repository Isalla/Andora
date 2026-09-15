# Telemetrie und Analyse

## Status

**Konzept / Architekturziel.**

Dieses Dokument beschreibt die geplante Telemetrie-/Analyse-Schicht von Andora als **Architekturziel**. Es wird **kein Code** beschrieben, der bereits existiert. Es wird **keine technische Festlegung** getroffen, die erst in der Implementierungsphase entschieden werden muss (siehe Abschnitt 6).

Dieses Dokument umfasst die **allgemeine Server-Telemetrie** (nicht nur die Lua-Schicht): Worker-/Realm-bezogene Laufzeitdaten, Queue-Zustände, Incident-/Recovery-Logs und die spätere KI-gestützte Analyse.

---

## 1. Grundprinzip

Telemetrie ist eine **passive Beobachtungsschicht**:

* Sie erfasst serverseitig **diagnoserelevante Laufzeitdaten** (u. a. zu den in `Lua-Scripting-System.md` Abschnitte 16–19 beschriebenen Worker-Pool-, Watchdog-, Failover- und Monitoring-Punkten).
* Sie **beobachtet und dokumentiert**, ohne Spielregeln oder Realm-Zustand zu ändern.
* Rust bleibt für Spielregeln und Zustand autoritativ; die Telemetrie hat darauf **keinen** Einfluss.

---

## 2. Wichtig: Kein automatischer „Schuldiger", keine automatische Entschädigung

> **Prinzip:** Es wird **kein automatischer „Schuldiger"** bestimmt und **nichts automatisch entschädigt** (siehe auch `Lua-Scripting-System.md` Abschnitt 17 und die dortige Abgrenzung zur KI).

* Die Telemetrie liefert **Diagnosekontext und Beobachtungsdaten**, aber **kein automatisches Urteil**.
* Eine spätere KI-Ursachenanalyse **markiert** Korrelationen und Auffälligkeiten (siehe Abschnitt 3), bestimmt aber **nicht** automatisch einen Verantwortlichen und **löst nichts automatisch aus**.
* Es wird **weder automatisch entschuldigt noch automatisch entschädigt** – weder Spieler noch Systeme.

---

## 3. KI-gestützte Analyse: markieren, nicht entscheiden

Später kann eine KI statt einer sofortigen Ursachensuche **Korrelationen und Auffälligkeiten markieren** (siehe `Lua-Scripting-System.md` Abschnitt 17).

* Die KI **markiert** und **korreliert** Auffälligkeiten; sie bestimmt **keinen automatischen „Schuldigen"** und ergreift **keine automatische Maßnahme** (Abschnitt 2).
* Korrelationen sind **Hinweise für die menschliche/r Fach-Analyse**, kein Ersatz für die Ursachenklärung.
* Die konkrete Analyse-/KI-Pipeline ist in Abschnitt 6 als offen markiert.

---

## 4. Incident-/Recovery-Log

* Für Vorfälle wird ein **Incident-/Recovery-Log** geführt (siehe `Lua-Scripting-System.md` Abschnitt 17 „Diagnosekontext" und Abschnitt 18 „Failover/Recovery").
* Es wird dokumentiert: betroffene Systeme (z. B. Worker-Pool, Realm, Raid-Instanz), beobachtete Fehler, **Diagnosekontext** und der Abschluss des Recovery-/Failover-Ablaufs.
* Ein abgeschlossenes Incident-/Recovery-Log bleibt als **dauerhafte Beobachtung** erhalten (siehe `Lua-Scripting-System.md` Abschnitt 19).

---

## 5. Monitoring-Anbindung

Die Telemetrie ergänzt die bestehende Monitoring-Infrastruktur:

* **`monitoring_web_panel.md`** – Laufende Status-/Live-Beobachtung (Ports 3001–3003, `/status`, `/players`).
* **`Lua-Scripting-System.md` Abschnitt 19** – Monitoring des Worker-Pools; Queue-Tiefe/-Latenz ist **monitoringpflichtig** (siehe dort Abschnitt 12 zu QoS-Klassen).

Die Telemetrie liefert daraus die **dauerhafte, historische Analyse-Ebene** (Retention/Format siehe Abschnitt 6).

---

## 6. Noch NICHT festlegen bzw. erfinden

Folgende Entscheidungen erfolgen in der Implementierungsphase und werden hier bewusst **nicht** getroffen:

* konkretes Log-Format / Datenbankformat für Telemetrie-Daten
* Retentionsdauer und -aufbewahrungsregeln
* konkrete KI-Analysepipeline / Modellwahl
* konkrete Markierungs- und Korrelationslogik (Abschnitt 3)
* automatische Reaktions- oder Entschädigungslogik (bleibt ausgeschlossen, Abschnitt 2)
* Netzwerkprotokolländerungen für Telemetrie-Übertragung

> Hinweis: Diese Punkte sind bewusst offen und dürfen in diesem Dokument **nicht erfunden** werden.

---

## 7. Beziehung zu bestehenden Systemen

| System | Dokument | Bezug zur Telemetrie |
|---|---|---|
| Worker-Pool / Failover / Watchdog | `Lua-Scripting-System.md` (Abschnitte 16–19) | Telemetrie erfasst die dort beschriebenen Laufzeit- und Incident-Daten |
| Monitoring-Web-Panel | `monitoring_web_panel.md` | Live-Überblick; Telemetrie ergänzt die historische Analyse-Ebene |
| Realm-/Betriebsarchitektur | `Deployment_Betriebsarchitektur.md` | Telemetrie ist Teil der Beobachtung der fünf Serverdienste |
| KI-Architektur | `coordinator_service.md` (Coordinator) | Analyse-/Markierungslogik später angebunden; keine automatische Maßnahme |
