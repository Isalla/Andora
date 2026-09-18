# Serverautorität & Anti-Manipulation V1

**Leitgedanke: Der Client darf lügen. Der Server darf ihm nur nicht glauben.**

V1 ist eine kleine, robuste Grundlage gegen manipulierte Clients — kein
Anti-Cheat-Framework, keine externe Software, keine Kernel-Treiber, kein
Machine Learning, keine Bot-Erkennung, keine automatischen permanenten Banns.

## 1. Grundprinzip (umgesetzt)

Der Client übermittelt **Absichten und Eingaben**. Der Server bestimmt
**Spielzustand und Ergebnisse**. Der Client dient bei relevanten Werten nur
der Darstellung.

Autoritative Quelle ist ausschließlich der serverseitige RAM-Zustand
(`world::Player`, geladen aus `realm_state_<realm>`); persistiert wird über
die bestehende Persistenzarchitektur (Dirty-Flags → Spool → Drain, keine
zusätzlichen DB-Pfade pro Request).

## 2. Bereits serverautoritativ (Bestand, verifiziert — kein Umbau)

| System | Autorität | Nachweis |
|---|---|---|
| HP/Mana/Schaden/Treffer | `combat::combat_tick` würfelt serverseitig (Config + Attribute) | `combat/mod.rs` |
| Tod/Leben | `hp <= 0` → KILL-Broadcast, Entwaffnung aller Angreifer | `combat_tick`, `handle_attack` |
| Fähigkeiten | Mana, Cooldown, gelernt, Reichweite, Caster lebt | `ability::start_ability`, `targeting::validate_caster_status` |
| Bewegung | Speed-Cap 210 m/s, keine Teleports | `world::apply_move` |
| Loot | Claim, Distanz, Aufteilung, Despawn | `loot::attempt_pickup` |
| Inventar/Equipment | Slots, Stacks, Equip-Checks, defekte Items | `inventory.rs`, `handle_hello` |
| EXP/Level/Rested | Kurve, Cap, Differenz, Pool | `progression.rs` |
| Einstieg | Handoff + Session, Account-Gleichheit, fail-closed | `handlers::verify_entry` |
| Chat | Eltern-Gate, 240-Zeichen-Cap | `handle_chat` |

Das Protokoll enthielt bereits **keine** `SET_*`-Nachrichten mit Endwerten
(kein `SET_HP`/`SET_GOLD`/`SET_STRENGTH`, kein Client-Schaden) — es gab also
keine Stelle, an der der Server Clientwerte als Wahrheit übernahm. Die
Handler lesen nur Aktionsfelder (`target_id`, `dir`, `loot_id`, …);
mitgesendete `hp`-/`damage`-/`gold`-Felder werden ignoriert (Test
`manipulated_hp_and_damage_are_ignored`).

## 3. Neu in V1 (`src/realm-rs/src/security.rs`)

### 3.1 Attributpunkte: Aktion statt Endwert

Neues Protokoll (IDs synchron in `src/realm-rs/src/protocol.rs` und
`shared/protocol.gd`):

```text
C2S 20 SPEND_ATTRIBUTE  {attribute: "strength"}   (nur die Aktion)
S2C 21 ATTRIBUTE_RESULT {ok, attribute, <alle 7 Werte>, free_attr_points}
```

`security::spend_attribute_point` (aufgerufen von
`handlers::handle_spend_attribute`):

1. Charakter aus serverseitigem RAM-Zustand,
2. `free_attr_points > 0` prüfen (sonst `no_attribute_points`),
3. Schlüssel gegen Allowlist prüfen (sonst `unknown_attribute`),
4. Attribut um genau **+1** erhöhen, Punkt abziehen,
5. Max-Ressourcen neu berechnen, Progression dirty markieren
   (Persistenz über Spool, kein Sonderpfad).

### 3.2 Auktionskauf: Validierung aus Serverwerten

Der Client sendet nur `AUCTION_BUY {auction_id}`.
`security::validate_auction_buy` prüft aus **Serverwerten**:
Existenz → Aktivität → Preisgültigkeit → Selbstkauf-Verbot →
**serverseitiges** Gold. Client-Goldanzeige ist irrelevant
(243 serverseitig schlägt 500er-Kauf trotz angezeigter 99.999 fehl).

**V1-Stand:** Es existiert noch kein Auktionshaus-State im Realm (kein
AH-Modul, keine Auction-Tabellen — Design in
`docs/Auktionshaus und Marktplatz`). `handle_auction_buy` lehnt daher
aktuell **jeden** Kauf fail-closed ab (`auction_unknown`, kein
Gold-/Item-Transfer). Die reine Validierungslogik ist unit-getestet; sobald
AH-State existiert, wird dort der Lookup angeschlossen (markierte
Aufrufstelle in `handle_auction_buy`) — kein Protokollumbau nötig.
Gleiches Anschlussprinzip gilt später für Handel, Crafting und Sammeln
(Aktion → Server prüft → Server führt aus).

### 3.3 Frühe Netzwerkprüfung (`net.rs`)

Reihenfolge pro Paket (billig → teuer):

```text
Paketgröße (vor dem Parsen) → JSON-Format → Typ-Whitelist → Session
→ Sequenz (nur vermerkt) → Rate Limit → Game Logic → ggf. Datenbank
```

- Übergröße/unparsbar/unbekannt: verwerfen + Auffälligkeit zählen.
- Ohne Session (außer HELLO): verwerfen (`no_session`).
- Rate Limit überschritten: verwerfen (`rate_limited`); bei massiver/
  wiederholter Überschreitung (`SEC_DISCONNECT_AFTER_VIOLATIONS`):
  Verbindung trennen — **kein permanenter Bann**.
- Sequenz: Duplikate/Out-of-Order durch Lag werden nur vermerkt, nie als
  Cheat gewertet oder abgelehnt.

### 3.4 Rate Limiting (gestaffelt)

| Kategorie | Typen | Default/Sekunde | Env |
|---|---|---|---|
| Bewegung | MOVE | 30 | `SEC_MOVE_PER_SEC` |
| Interaktiv | CHAT, HEARTBEAT, PARENTAL | 10 | `SEC_INTERACTIVE_PER_SEC` |
| Kampf | ATTACK, ABILITY, PICKUP | 10 | `SEC_COMBAT_PER_SEC` |
| Selten | Attribute, Auktion, Gruppe, NPC, Unbekannt | 5 | `SEC_RARE_PER_SEC` |

Sliding-Window (1000 ms) je Verbindung und Kategorie, reine RAM-Operation
(`ConnGuard`). 500 Spend-Requests in 1 s → 5 passieren, 495 werden vor
jeder Logik/DB verworfen. Unbekannte Typen fallen fail-closed in Selten.

### 3.5 Tod und Instanz-Isolation

- Toter Angreifer (`hp <= 0`): `handle_attack` verwirft (`attacker_dead`),
  `combat_tick` entwaffnet zusätzlich; `start_ability` lehnt mit
  `caster_dead` ab. Der Client kann weiter senden — der Zustand ändert
  sich nicht.
- Kein Instanzabbruch: Nur die ungültige Aktion wird verworfen; der Raid/
  Dungeon läuft für alle anderen normal weiter.

### 3.6 Keine Cheat-Verurteilung, kein Bannsystem

Ungültig ≠ Cheat (Lag, Duplikate, Client-/Serverfehler möglich). V1 lehnt
nur ab, zählt Auffälligkeiten (`sec-reject`-Logs) und trennt bei
massiver/wiederholter Überschreitung die Verbindung. Keine automatischen
permanenten Banns (V2-Entscheidung bei Live-Bedarf, vgl.
`docs/v1_v2_leitlinie.md`).

### 3.7 Logging

`sec-reject`-Zeilen (Level WARN): Zeitpunkt, Connection, Charakter,
Session, Request-Typ, Ablehnungsgrund, Wiederholungsanzahl, kompakter
Serverzustand (`hp/level/exp/idia/free_attr`). Keine großen Datenmengen.
Basis für eine spätere GM-/Anti-Cheat-Auswertung (nicht Teil von V1).

### 3.8 Abgrenzungen (bewusst nicht V1)

- Netzwerkverschlüsselung/Integrität ≠ Spielvalidierung: Ein korrekt
  übertragenes Paket ist keine vertrauenswürdige Spielaktion. Keine
  Krypto-Eigenbauten; zstd nur Kompression größerer Übertragungen, keine
  Kompression kleiner Echtzeitpakete, kein Schutzmechanismus.
- DB: Client hat nie direkten DB-Zugriff; Charakter-/Sessiondaten aus dem
  RAM-Zustand nutzen, dann über Spool persistieren (kein Request → DB-Query).
- Handel/Auktionshaus-State/Crafting/Sammeln/Quests: Quest-Fortschritt ist
  bereits serverautoritativ (`quest.rs`, atomarer Abschluss); AH-State,
  Handel, Crafting und Sammeln existieren noch nicht — keine
  Platzhaltersysteme gebaut, Anschluss dokumentiert (§3.2).

## 4. Konfiguration (`SEC_*` in `config.env`, Beispiel in `config.env.example`)

`SEC_MAX_FRAME_BYTES` (65536), `SEC_MOVE_PER_SEC` (30),
`SEC_INTERACTIVE_PER_SEC` (10), `SEC_COMBAT_PER_SEC` (10),
`SEC_RARE_PER_SEC` (5), `SEC_DISCONNECT_AFTER_VIOLATIONS` (50).

## 5. Tests (`security.rs`, 16 Tests — alle 8 Pflichtfälle)

1. Manipulierter Attributwert (`strength=999`) → nur +1 ab Serverwert;
   unbekannte Attribute (`gold`, `strength999`) → Ablehnung ohne Mutation.
2. Spend ohne Punkte → `no_attribute_points`, keine Mutation, kein Dirty.
3. 243 Gold vs. 500er-Auktion → `insufficient_gold`; 200er → freigegeben.
4. Unbekannte/inaktive/Selbstkauf-Auktion, Preis ≤ 0 → abgelehnt.
5. `hp=140000`/`damage=50000` in MOVE/ATTACK → ignoriert, Server-HP 3567.
6. Toter Angreifer: ATTACK verworfen (kein Combat-State, Boss unverletzt),
   ABILITY-Caster tot → abgelehnt.
7. 500 Rare-Requests/s → nur 5 passieren; Bewegungslimit > Selten-Limit;
   wiederholte Überschreitung → Disconnect (kein Bann).
8. Übergröße/unbekannt/ohne Session → Drop vor jeder teuren
   Verarbeitung (Zähler für DB/Kampf/Inventar/Welt/KI bleibt 0).

Gesamt: `cargo test` im Realm — 415 Tests, 0 Fehler.

## 6. Grenzen von V1 (V2-Kandidaten bei Live-Bedarf)

- AH-/Handels-/Crafting-/Sammel-State fehlt noch ( fail-closed-Stub ).
- Kein persistentes Auffälligkeitsprofil über Sessions hinweg (nur Logs).
- Rate Limits sind feste Defaults, nicht adaptiv; keine IP-Ebene
  (siehe `docs/automatische_ip-sperre.md` für die separate IP-Schicht).
- Keine Replay-/Bot-Verhaltensanalyse, keine GM-Oberfläche.
