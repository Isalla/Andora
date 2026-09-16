# Andora – Runtime-State-Lifecycle und Cleanup

## 1. Status

**Dokumentation (Strategie, noch nicht vollständig als allgemeiner Mechanismus implementiert).**

Diese Datei definiert den Lebenszyklus von **ausschließlich zur Laufzeit benötigten Zuständen** im Realm-Server (`src/realm-rs`).

Sie ergänzt die Persistenzstrategie (`Player_Persistenz.md`) um die bewusst getrennte Frage: **Wann wird ein Runtime-Objekt aus den besitzenden World-/Runtime-Strukturen entfernt und sein Speicher dadurch freigegeben?**

Autoritativ bleiben die bestehenden Regeln:

* `Player_Persistenz.md` – Dirty-State, periodischer Player-Save, finaler Disconnect-/Shutdown-Save
* `Datenbank_Architektur.md` – Datenbankpersistenz (`realm_state_<realm>`)
* `Deployment_Betriebsarchitektur.md` – insbesondere §7 Realm-Shutdown im Update-Ablauf
* `inventory_system.md`, `Quest-System.md` – systembezogene Persistenz-/Lifecycle-Regeln der jeweiligen Systeme

---

## 2. Grundprinzip

Rust besitzt keinen klassischen Garbage Collector.

Speicher wird automatisch freigegeben, sobald ein Objekt nicht mehr besessen/referenziert wird.

Deshalb wird für Andora **kein allgemeiner Garbage Collector** gebaut.

Stattdessen gilt:

> Nicht mehr benötigte Runtime-Objekte werden gezielt aus den besitzenden World-/Runtime-Strukturen entfernt. Danach kann Rust deren Speicher automatisch freigeben.

---

## 3. Persistenz ist nicht Cleanup

Ein erfolgreich persistierter Zustand wird **nicht** allein deshalb aus dem RAM entfernt.

Beispiel:

> Ein eingeloggter Spieler bleibt nach einem periodischen Player-Save weiterhin im autoritativen Realm-RAM.

**Persistenz:**

```text
RAM-Zustand
→ MariaDB
```

**Cleanup:**

```text
Runtime-Objekt wird nicht mehr benötigt
→ aus besitzender Runtime-Struktur entfernen
→ Rust kann Speicher freigeben
```

Beide Vorgänge sind getrennte Konzepte und dürfen nicht vermischt werden:

* Persistenz bedeutet **nicht**, dass der RAM-Zustand fällt.
* Cleanup bedeutet **nicht**, dass ohne persistierten Zustand entfernt wird (Abschnitt 9).

---

## 4. Cleanup-Kandidaten

Systeme mit temporären oder begrenzt gültigen Runtime-Zuständen müssen einen definierten Cleanup-/Removal-Lebenszyklus besitzen.

Dazu können beispielsweise gehören:

* vollständig getrennte Player-Sessions
* geschlossene Netzwerk-Sessions
* despawnte NPCs und Monster
* abgelaufene Loot-Objekte/Truhen
* temporäre Drops
* abgelaufene Buffs/Debuffs
* nicht mehr benötigte Cooldown-Zustände
* beendete Combat-/Aggro-Zustände
* abgeschlossene temporäre Events
* geschlossene Dungeon-/Instanzzustände
* temporäre Quest-Runtime-Daten
* nicht mehr benötigte Lua-/Script-Runtime-Zustände
* temporäre Caches

Diese Liste definiert **keine** Aussage darüber, dass alle genannten Systeme bereits existieren.

Es werden **keine** neuen Gameplay-Systeme allein für Cleanup erfunden.

---

## 5. Player-Disconnect

Beim Disconnect gilt konzeptionell:

```text
1. finalen persistenzpflichtigen Spielerzustand speichern
2. Verbindung/Session beenden
3. nicht mehr benötigten Player-/Session-Runtime-State aus den
   entsprechenden Realm-Strukturen entfernen
4. dadurch nicht mehr referenzierten Speicher freigeben lassen
```

Die konkrete Reihenfolge wird bei der Implementierung gegen die bestehende Disconnect-Architektur geprüft (`net.rs` Disconnect-Pfad plus `world::disconnect_player`, der Spieler aus `World.players` entfernt und `World.by_conn` bereinigt).

Kein Zustand wird entfernt, der für einen noch laufenden finalen Save benötigt wird (siehe `Player_Persistenz.md` Abschnitt 11: der Disconnect-Save läuft über denselben zentralen Player-Persistenzpfad wie der periodische Save).

---

## 6. Periodischer Cleanup

Der Realm kann für geeignete Systeme einen periodischen Cleanup-Tick besitzen.

Dieser Cleanup ist:

* **kein** allgemeiner RAM-Scanner
* **kein** Garbage Collector

Er verarbeitet ausschließlich Systeme, deren Lifecycle-Regeln bestimmen, dass bestimmte Runtime-Objekte nicht mehr benötigt werden.

Das Cleanup-Intervall soll **konfigurierbar** sein (konsistent zur bestehenden Umgebungsvariablen-Konvention, analog zu Intervall-Keys wie `NPC_PERSIST_INTERVAL_MS` in `config.rs`; der konkrete Key wird beim Coding-Auftrag festgelegt).

In diesem Doku-Auftrag wird **kein konkretes Default-Intervall** festgelegt.

Erst anhand der tatsächlichen Serverarchitektur und späterer Messwerte soll bestimmt werden, welche Systeme:

* sofort beim Lifecycle-Ende entfernt werden
* periodisch bereinigt werden
* oder beides als Sicherheitsmechanismus verwenden

---

## 7. Kein blindes Full-Scan-Cleanup

Es wird nicht festgelegt, dass der Realm regelmäßig sämtliche World-Datenstrukturen vollständig durchsuchen muss.

Bevorzugt werden **Lifecycle-basierte Entfernungen**.

Beispiel:

```text
NPC despawnt
→ aus aktiver NPC-Struktur entfernen
```

statt:

```text
alle X Minuten jeden jemals erzeugten NPC durchsuchen
```

Periodische Sweeps dürfen als Sicherheits-/Cleanup-Mechanismus verwendet werden, wenn sie für ein konkretes System sinnvoll sind.

---

## 8. Unbegrenzt wachsende Strukturen

Runtime-Collections, Maps, Queues, Caches oder History-Strukturen dürfen **nicht unbeabsichtigt unbegrenzt wachsen**.

Für langlebige Realm-Prozesse muss bei solchen Strukturen geklärt sein:

* wodurch Einträge entstehen
* wann sie nicht mehr benötigt werden
* wodurch sie entfernt werden
* ob eine Größen-/Zeitgrenze erforderlich ist

Es werden **keine willkürlichen Limits** in diesem Doku-Auftrag erfunden.

Die bestehenden World-Strukturen (`world.rs`: `World.players`, `World.npcs`, `World.loot_drops`, `World.by_conn`, `World.closers`, zusätzlich Gruppen-/Lua-/Quest-Runtime-Strukturen) werden im späteren Implementierungsaudit (Abschnitt 12) einzeln geprüft; vorhandene Removal-/Timeout-Mechanismen (z. B. Loot-Despawn über `loot_tick`, Gruppen-Reconnect-Frist über `group::tick`) bleiben dabei unverändert gültig.

---

## 9. Dirty-State und Cleanup

Ein persistenzpflichtiger Dirty-State darf durch Cleanup **nicht verloren gehen**.

> Vor dem Entfernen eines persistenzpflichtigen Runtime-Zustands muss dessen erforderliche finale Persistenz erfolgreich behandelt worden sein.

Temporäre, ausdrücklich **nicht** persistente Zustände benötigen dagegen keinen DB-Write nur aufgrund ihres Cleanup.

Abgrenzung zu `Player_Persistenz.md`:

* Dirty-State betrifft die Frage „**was** muss wann nach MariaDB".
* Cleanup betrifft die Frage „**wann** wird ein RAM-Objekt nicht mehr benötigt".
* Beides muss so aufeinander abgestimmt sein, dass kein persistenzpflichtiger Zustand verloren geht.

---

## 10. Graceful Shutdown und Cleanup

Beim Graceful Shutdown hat die **Sicherung persistenzpflichtiger Zustände Vorrang** vor dem Freigeben ihrer Runtime-Repräsentationen.

Konzeptionell:

```text
Gameplay stoppen/einfrieren
→ benötigte persistente Zustände final speichern
→ Runtime-Strukturen kontrolliert abbauen
→ DB-/Serverressourcen schließen
→ Prozess beenden
```

Cleanup darf einen notwendigen finalen Save nicht verhindern.

Reihenfolge und Final-Save-Pflicht sind in `Player_Persistenz.md` Abschnitt 12 festgelegt (keine neuen Spieler aufnehmen → Gameplay stoppen → konsistente Zustände erfassen → final speichern → DB-Ressourcen schließen → beenden), inklusive Einordnung in den Realm-Update-Ablauf (`Deployment_Betriebsarchitektur.md` §7). Der kontrollierte Abbau der Runtime-Strukturen erfolgt danach.

---

## 11. Memory-Monitoring

Der Realm soll später hinsichtlich seines Runtime-Speichers beobachtbar sein.

Sinnvolle Metriken können umfassen:

* Prozess-RAM
* Anzahl aktiver Spieler
* Anzahl aktiver NPCs/Monster
* Anzahl temporärer Runtime-Objekte
* Größen relevanter Caches/Queues
* Cleanup-Dauer
* Anzahl entfernter Objekte pro Cleanup

Es werden **nur Metriken für tatsächlich vorhandene Systeme** implementiert.

Es wird **keine Dashboard-UI** in diesem Auftrag entworfen. Die vorhandene Monitoring-Schnittstelle (`monitoring_web_panel.md`, `/status`-Metriken in `health.rs`) ist der natürliche Anzeige-Ort; konkrete Metrikfelder legt der Coding-Auftrag fest.

---

## 12. Späterer Implementierungsaudit

Vor Implementierung eines allgemeinen Cleanup-Mechanismus muss die **bestehende Realm-Runtime** gelesen werden.

Dabei wird insbesondere geprüft:

* langlebige HashMaps/Collections
* Player-/Session-Lifecycle
* NPC-/Monster-Lifecycle
* Combat-/Aggro-State
* Loot-/temporäre World-Objekte
* Quest-Runtime-State
* Lua-Runtime-State
* Caches
* Queues/History-Strukturen
* sonstige Collections mit potenziellem unbegrenztem Wachstum

Für jede relevante Struktur wird berichtet:

1. Besitzer
2. wodurch Einträge entstehen
3. bestehender Removal-Pfad
4. ob Einträge zuverlässig entfernt werden
5. ob Wachstum begrenzt ist
6. ob Cleanup fehlt
7. ob sofortiges Lifecycle-Cleanup oder periodischer Sweep sinnvoller ist

Dieser Audit ist ein **späterer, separater Auftrag**.

In diesem Doku-Auftrag wurde **kein Rust-Code** geändert.