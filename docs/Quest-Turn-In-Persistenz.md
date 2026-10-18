# Questabgabe und Belohnungen – notwendige Vertragsfreigabe

## Status

**Freigabe GRANTED und durchgängig implementiert (Productions-Pfad, offline-geprüft).**
`IMPLEMENT_DURABLE_QUEST_COMPLETION` realizes the here described contract: the
existing Spool-/Drain-Pfad is extended with a single-character `quest_completion`
order and the additive Receipt-Table `character_quest_commits` (Migration 025,
created + registered, NOT executed against a real DB). The mutating NPC turn-in
(`quest_turn_in`) now completes a fulfilled Killquest with the prepared EXP-/Idia
rewards; no `quest_turn_in_unavailable` block remains.

Grundlage of the released contract (unchanged below):

## Warum der vorhandene Vertrag nicht ausreicht

1. `persist::PersistSnapshot` enthält einen **vollständigen** Charakterstand,
   aber keine Questzustände. `persist::write_snapshot_fields` schreibt unter
   anderem Position, Progression, Idia, Ressourcen, Attribute und Inventar;
   `apply_snapshot_to_db` schreibt Lifecycle und Revision dazu.
2. `Spool` überspringt Snapshots bei gleicher DB-Revision und archiviert sie
   bei höherer DB-Revision. Ein partieller Direktwrite von EXP/Idia/Inventar
   mit Revisionssprung könnte dadurch noch nicht gedrainete Position,
   Ressourcen, Attribute oder Lifecycle-Veränderungen eines Vollsnapshots
   verlieren. `persist_revision + 1` allein ist kein konsistenter Vollstand.
3. `guarded_complete_quest` schützt nur ACTIVE→COMPLETED. Nach einem verlorenen
   COMMIT-Ergebnis bedeutet ein späteres Guard-Ergebnis von null Zeilen nur,
   dass die Quest nicht mehr ACTIVE ist. Es beweist weder die Identität des
   Abschlussversuchs noch dessen Belohnung und berechtigt nicht zum erneuten
   Anwenden eines alten RAM-Entwurfs.
4. Ein RAM-only-Abschlussauftrag reicht nach Prozessabbruch nicht zur sicheren
   Wiederaufnahme. Die unveränderliche Belohnungsplanung samt Instanz-UUIDs
   braucht einen dauerhaften Nachweis, bevor ihr Ergebnis bestätigt wird.

Der abgebrochene eigene Zwischenentwurf mit partiellen Direktwrites und
ungesichertem Revisionssprung wurde daher entfernt. Der bestehende, bislang
nicht an NPC-Abgabe gebundene `complete_for_character`-Kern bleibt Bestand;
er wird nicht als Reward-/Spool-/Crash-Nachweis ausgegeben.

## Kleinster vorgeschlagener Vertrag

Den vorhandenen **Spool-/Drain-Pfad** um einen eincharakterigen
`quest_completion`-Vorgang erweitern, analog zum vorhandenen, mit Commit-Beleg
abgesicherten Spielerhandel. Keine parallele Belohnungs-Persistenzpipeline.

### Unveränderlicher Auftrag

- technische Commit-ID, `character_id`, `quest_id`, Basis- und Zielrevision;
- vollständiger resultierender `PersistSnapshot` einschließlich Lifecycle;
- resultierender COMPLETED-Questzustand;
- kanonischer Payload-/Identitätsbeleg, einschließlich einmal festgelegter
  Iteminstanz-UUIDs, falls später Itembelohnungen freigegeben werden;
- alle Reward-/Level-up-Berechnungen genau einmal aus dem geschützten
  serverseitigen Zustand; keine Neuberechnung beim Retry.

Vorhandene normale und Trade-Publications des Charakters müssen vor der
Planung geklärt sein. Basisrevisionen dürfen nicht übersprungen werden.
Die Veröffentlichung nutzt die bestehende atomare Datei-/Sync-Grenze,
Pending-Registry und Recovery-Reihenfolge.

### Gemeinsame DB-Transaktion

Unter dem bestehenden Charakter-Gate und ohne World-Lock:

1. Charakterrevision und Questzeile sperrend lesen.
2. Existierenden Abschlussbeleg auf **denselben** Payload prüfen. Identischer
   Beleg liefert `already_applied`, abweichender Payload einen Konflikt.
3. Basisrevision prüfen; fehlende Vorgänger abwarten, keine Revision erraten.
4. Vollsnapshot über den gemeinsamen Writer einschließlich Lifecycle schreiben.
5. ACTIVE→COMPLETED mit Beleg und Zielrevision in derselben Transaktion setzen.
6. COMMIT. Nach Antwortverlust denselben Auftrag anhand des Belegs klären.

**Vorgeschlagene Belegablage:** eine additive Receipt-Tabelle
`character_quest_commits` nach dem vorhandenen Trade-Muster, mit Commit-ID,
Charakter-/Quest-ID und kanonischem Auftrag. Für diesen nicht wiederholbaren
Minimalumfang muss auch das Paar Charakter/Quest eindeutig sein. Der Beleg
wird in derselben Transaktion wie Vollsnapshot und COMPLETED geschrieben und
bei technischer Wiederholung inhaltlich geprüft. Bestehende Quest-State-
Writes dürfen einen bewiesenen Abschluss nicht zurücksetzen. Der Beleg wird
nicht in ein JSON-Feld eingebaut, das ein älterer Questencoder unbemerkt wieder
überschreiben könnte. Noch keine Migration erstellt oder registriert.

### RAM, Timeout und Shutdown

- Kein Erfolg vor gemeinsam gesichertem Abschluss/Reward **und** RAM-Übernahme.
- Bei unklarem Ergebnis bleibt derselbe Auftrag offen. Ein späterer Fehler vor
  COMMIT beseitigt die ursprüngliche Unklarheit nicht.
- Charakterbezogene Mutations-/Save-/Login-Gates respektieren die offene
  Publication; unabhängige Charaktere bleiben sicherbar.
- 2-s-I/O-Muster der Annahme wiederverwenden; Gate-Warten und gesamter
  Shutdown-Recovery-Pass erhalten eine tatsächliche Deadline. Timeout ist kein
  Rollback-Beweis. Ein Transaction-Drop wird nicht als Commit-Wirkung bewertet.
- Abbruch vor RAM-Übernahme lässt den dauerhaften Auftrag/Beleg wiederaufnehmbar.
  Neustart-Recovery verarbeitet denselben Auftrag vor Charakterfreigabe.

## Unabhängig vorbereiteter V1-Inhalt

- `turn_in`: optionale, validierte stabile NPC-Content-ID, unabhängig von `giver`.
  §27.22 bestimmt keinen Fallback. Bis zu einer fachlichen Festlegung gilt:
  ohne explizites `turn_in` keine NPC-Abgabevorschau.
- `rewards = { exp = N, idia = M }`: nichtnegative Ganzzahlen bis i64::MAX;
  fehlende Felder bedeuten null. Keine erfundenen Mengen oder Balancewerte.
  EXP ist für `Progression::grant_exp` vorgesehen (kein Rested-Bonus).
- Andere Reward-Felder, einschließlich `xp`, `gold`, `items`, Auswahlbelohnungen,
  Ruf und Titel, werden ausdrücklich abgewiesen. Das langfristige Beispiel in
  `quests_stories.md` ist kein unmittelbar ladbarer V1-Vertrag.
- Itembelohnungen und damit echte Inventarvoll-/UUID-Wiederaufnahmetests sind
  noch nicht umgesetzt. Es werden keine Auswahlregeln erfunden.

## Zur Freigabe erforderlich

Zustimmung zur Erweiterung des bestehenden Spool-/Drain-Vertrags um den oben
beschriebenen vollständigen `quest_completion`-Auftrag mit dauerhaftem
Identitätsbeleg in der additiven Tabelle `character_quest_commits`.
Ein optional gewünschter `giver`-Fallback muss
ebenfalls ausdrücklich festgelegt werden; explizites `turn_in` genügt sonst.
