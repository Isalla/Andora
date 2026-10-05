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

Zur Zahlen- und Arraygrenze des MOVE-Pfads (`handle_move`,
`handlers.rs:634-672`): Der Client liefert eine Richtung, keine Endposition.
`dir` wird nur gelesen, wenn es ein Array mit **mindestens zwei** Elementen
ist (`handlers.rs:636`); sonst greifen `x`/`y` mit Standard `0.0`. Falsch
typisierte Elemente ergeben `0.0` (`as_f64().unwrap_or(0.0)`). Nach der
Wegnormalisierung begrenzt `apply_move` den Schritt auf 210 m/s ×
Tickintervall (`world.rs:679-687`).

Belegte Randfälle am echten Parse-/Handlerpfad (Tests in `net.rs`, keine
Sonderregel im Produktionscode):

- Sehr große, aber endlich darstellbare Werte (`dir:[1e308,0]`) → begrenzte,
  endliche Bewegung (`x = 21.0` bei 100-ms-Tick). Kein Rechenüberlauf.
- Zahlen außerhalb des f64-Bereichs (`1e400`) → `serde_json` weist sie mit
  „number out of range" ab; der Frame scheitert bereits an der
  Frame-Deserialisierung und wird am Parse-Gate verworfen. Damit ist `±∞`
  über den Empfangspfad **nicht erreichbar**.
- Endliche Einzelwerte, deren Betragsquadratsumme überläuft
  (`dir:[1.7e308,1.7e308]`, `hypot = ∞`) → kontrolliertes No-Op, Position
  bleibt endlich bei `(0,0)`.

`world::apply_move` wäre für `dx = ±∞` nicht endlich (`∞/∞ = NaN`). Das ist
über den Empfangspfad nicht erreichbar; der tragende Nachweis ist die
**Parser-Grenze**, nicht die Robustheit von `apply_move`. Der Sicherheits-
bzw. Nicht-Befund ist ausdrücklich **kein** Beweis einer allgemeinen
Bewegungsvalidierung — Positionsauthorität bleibt allein `apply_move`.

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

**Tatsächliche Reihenfolge** einer C2S-Nachricht (billig → teuer), wie im
Produktionscode belegt:

```text
Paketgröße (vor dem Parsen) → JSON-Format → Typ-Whitelist
→ Sequenzbeobachtung (nur vermerkt, kein Gate)
→ Session-Gate → Rate-Gate → Dispatch → typabhängige Fachprüfung
→ ggf. Game Logic / Datenbank
```

**Verteilung auf die Funktionen (wichtig für jede Code- und Doku-Aussage):**

| Stufe | Ort | Funktion |
|---|---|---|
| Paketgröße | `read_loop` (`net.rs:582`) | `security::frame_too_large` |
| JSON-Format | `read_loop` (`net.rs:592`) | `serde_json::from_str` |
| Typ-Whitelist | `read_loop` (`net.rs:602`) | `security::is_known_c2s` |
| Sequenzbeobachtung | `dispatch` (`net.rs:831`) | `ConnGuard::note_seq` |
| Session + Rate Limit | `dispatch` (`net.rs:836`) | `security::gate_frame` |

`security::gate_frame` prüft **ausschließlich Session und Rate Limit**, in
dieser Reihenfolge. Größe, Parse und Whitelist liegen **beim Aufrufer**
(`read_loop`); sie sind **nicht** Teil von `gate_frame`. Der Name der
Funktion allein belegt also **keine** vollständige Abdeckung.

- Übergröße/unparsbar/unbekannt: verwerfen + Auffälligkeit zählen
  (`net.rs:583`, `:595`, `:603`). Jede dieser drei Stufen zählt über
  `ConnGuard::violations` genau eine Auffälligkeit; das ist der vorhandene
  Diagnosewert, über den sich die Ablehnungsstufe eindeutig zuordnen lässt.
- Ohne Session (außer HELLO): verwerfen (`no_session`).
- Rate Limit überschritten: verwerfen (`rate_limited`); bei massiver/
  wiederholter Überschreitung (`SEC_DISCONNECT_AFTER_VIOLATIONS`):
  Verbindung trennen — **kein permanenter Bann**.
- Sequenz: Duplikate/Out-of-Order durch Lag werden nur vermerkt, nie als
  Cheat gewertet oder abgelehnt. Die Sequenzstufe ist **kein** Gate und
  entscheidet nichts (siehe §3.3.1).

**Größenbegrenzung — Anwendung gegen Transport:** `SEC_MAX_FRAME_BYTES`
(Standard 65536) ist ein **Anwendungslimit**: Es greift im `read_loop` vor
dem JSON-Parsen und begrenzt Parse sowie Spiellogik. Es ist **keine**
Transportpuffergrenze — zum Zeitpunkt der Prüfung hat die WebSocket-Schicht
den Frame bereits angenommen und im Speicher gehalten. Das Projekt setzt
**kein** eigenes `WebSocketConfig`/`max_message_size`
(`tokio_tungstenite::accept_async` mit Standardkonfiguration,
`net.rs:503`); die wirksame Transportgrenze ist damit die
tungstenite-Vorgabe, nicht `SEC_MAX_FRAME_BYTES`.

**Nachweis:** Die Gate-Reihenfolge am echten Empfangspfad ist durch Tests
über den Produktions-`read_loop` belegt (`net.rs`:
`read_loop_drops_oversize_text_before_handler_effect`,
`read_loop_drops_invalid_json_text`,
`read_loop_drops_unknown_message_type`,
`read_loop_drops_valid_known_message_without_session`,
`read_loop_delivers_valid_known_message_to_handler`,
`read_loop_keeps_reading_after_single_rejected_frames`,
`read_loop_stops_at_existing_disconnect_threshold`).

#### 3.3.1 Verbindlicher Vertrag: `seq` ist Korrelation (normativ)

**Entscheidung:** `seq` ist ein **Korrelationsobjekt**. Es bestimmt weder
Zulässigkeit noch Zeitpunkt einer Spielaktion und ist **kein** Berechtigungs-
oder Idempotenzschlüssel. Der Client übermittelt Absichten; der Server prüft
**unabhängig von `seq`**, ob und wann diese ausgeführt werden dürfen. Es wird
**keine** Sequenz-Ablehnung, **keine** Strafregel und **keine** Clientpflicht
eingeführt.

**Beobachtung, nicht Entscheidung** (`src/realm-rs/src/security.rs:229-236`):

- `ConnGuard::note_seq` berechnet nur, ob ein Wert über dem bisherigen Höchstwert
  liegt, und schreibt den **Rückgabewert nicht in eine Entscheidung**: der
  einzige Aufrufer verwirft ihn (`src/realm-rs/src/net.rs:831`).
- `ConnGuard.last_seq` ist damit der **höchste bisher gesehene** `seq`-Wert
  dieser Verbindung, **nicht** der zuletzt gesehene: Er wird nur bei
  `seq > last_seq` fortgeschrieben (`src/realm-rs/src/security.rs:230-233`) und
  läuft bei älteren oder doppelten Werten **nicht** zurück. Der Begleitzustand
  `seen_any_seq` unterscheidet „noch keine Sequenz gesehen" von „Wert 0".
- Die Beobachtung liegt **nach** den Vorprüfungen des Read-Loops (Paketgröße,
  JSON-Format, Typ-Whitelist: `src/realm-rs/src/net.rs:582,592,602`) und
  **vor** dem Session- und Rate-Gate (`:836`). Übergrößene, unparsbare und
  unbekannte Typen werden deshalb **nicht** sequenzbeobachtet; Frames, die das
  Session- oder Rate-Gate verwirft, **werden** es.
- `ConnGuard` ist **verbindungslokal** und wird je Verbindung neu erzeugt
  (`src/realm-rs/src/net.rs:509`). Eine neue Verbindung beginnt ohne
  Sequenzhistorie.

**Was daraus folgt (ausdrücklich):**

- Gleiche, ältere, negative oder fehlende `seq` werden von der Sequenzbeobachtung **nicht abgelehnt**; alle übrigen Prüfungen (Whitelist, Session, Rate-Limits, Zustands- und Zielvalidierung im jeweiligen Handler) gelten unverändert weiter.
- Es findet **keine** sequenzbasierte Deduplizierung und **keine** Ergebniswiederholung statt: `seq` löst weder eine Wiederholung einer vorherigen Antwort aus noch verhindert sie eine erneute Ausführung derselben Absicht.
- **Rate-Limits sind kein Idempotenznachweis.** Sie begrenzen die Häufigkeit je Verbindung und Kategorie (`SEC_*_PER_SEC`), nicht die Einmaligkeit einer Operation.
- Der sequentielle Read-Loop verarbeitet die Frames einer Verbindung **der Reihe nach** (`src/realm-rs/src/net.rs:566-615`), verhindert aber **nicht**, dass der Client dieselbe Nachricht **zweimal sendet**.
- **Gleicher Inhalt mit neuer `seq`** und **Wiederholung über eine neue Verbindung** werden durch `seq` **nicht** geschützt. Maßgeblich ist in beiden Fällen allein die Servervalidierung des jeweiligen Handlers.
- Für zustandsändernde Operationen bleibt deshalb die **fachliche** Absicherung maßgeblich, nicht die Nachrichtennummer. Beispiel: Der veröffentlichte Fix `b121766` schützt den Angriffstakt (`docs/Kampfsystem.md` §3.1) über den **RAM-gebundenen** Zeitpunkt des zuletzt ausgeführten Schlags — unabhängig von `seq`, gleicher oder neuer Nummer, gleichem oder anderem Ziel. Das gilt an den Zustand dieses Player-Objekts gebunden und ist **keine** allgemeine Reconnect- und **keine** allgemeine Idempotenzgarantie.
- **Korrelationsfeld ist `Frame.seq`.** Für `MOVE` nennt der Typkommentar zusätzlich ein Payload-Feld `seq` (`src/realm-rs/src/protocol.rs:12`); der Handler wertet dieses **nicht** aus und liest ausschließlich `dir`/`x`/`y` (`src/realm-rs/src/handlers.rs:635-640`). Der Kommentar bleibt als Bestandsbeschreibung unverändert; verbindlich ist allein das Feld `Frame.seq` im Rahmen der Nachricht.

**Nachweisgrenzen (keine Annahmen als Tatsachen):** Im Repository existiert
**kein** Clientcode, der `seq` erzeugt oder erhöht (`shared/protocol.gd:24`
bietet nur den Helfer `encode`, ohne einen einzigen Aufrufer), und **keine**
Auswertung von `ack_seq`. Ein Quittungsfeld wird ausschließlich für
`HEARTBEAT` erzeugt (`src/realm-rs/src/handlers.rs:1393`, `SYNC {ack_seq}`);
`WELCOME`, `PARENTAL_RESULT`, `ATTRIBUTE_RESULT` und die System-Chat-Antwort
spiegeln die Nummer nur im S2C-Rahmen (`:583, :891, :1276, :1303, :1362`),
übrige S2C-Frames senden `seq = 0`. Ein tatsächliches Zähler-, Reset- oder
Retry-Verhalten des Clienten ist damit **nicht belegbar** und wird hier
ausdrücklich nicht festgelegt; getrennte Clientzähler je Nachrichtentyp werden
nicht als inkompatibel bewertet, solange der Server `seq` nicht zur
Zulässigkeitsentscheidung verwendet.

### 3.4 Rate Limiting (gestaffelt)

| Kategorie | Typen | Default/Sekunde | Env |
|---|---|---|---|
| Bewegung | MOVE | 30 | `SEC_MOVE_PER_SEC` |
| Interaktiv | CHAT, HEARTBEAT, PARENTAL | 10 | `SEC_INTERACTIVE_PER_SEC` |
| Kampf | ATTACK, ABILITY, PICKUP | 10 | `SEC_COMBAT_PER_SEC` |
| Selten | Attribute, Auktion, Gruppe, NPC, Unbekannt | 5 | `SEC_RARE_PER_SEC` |

**Whitelisted, aber nicht implementiert:** `NPC_TALK` (6), `AUCTION_LIST`
(7) und `AUCTION_BID` (8) stehen in `security::is_known_c2s`
(`security.rs:285-309`) und im Protokoll (`protocol.rs:16-18`), besitzen aber
**keinen** Dispatch-Arm in `net.rs:869-930`. Sie passieren Whitelist und
Gate und enden im `other`-Zweig (`net.rs:929`) **ohne Handler und ohne
Zustandswirkung** — bewusst fail-closed. `AUCTION_BUY` (9) hat dagegen einen
Arm und wird fail-closed abgelehnt (`handle_auction_buy`, `handlers.rs:1326`),
weil kein Auktionshaus-State existiert.

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

### 3.5a Angriffstakt: Absicht ist keine Berechtigung

`ATTACK` übermittelt eine Absicht. Maßgeblich für die Waffen-Duration ist der
zuletzt **tatsächlich ausgeführte** Schlag (`Player.last_strike`, nur vom
`combat_tick` geschrieben), nicht das Eintreffen der Absicht. Eine wiederholte
Absicht — gleiche oder neue `seq`, gleiches oder anderes Ziel, nach `stop`
oder nach Zieltod — setzt den Takt nicht zurück und schaltet keinen zusätzlichen
Sofortschlag frei. Der erste Schlag nach der Aktivierung ohne vorherigen Schlag
bleibt Sofortschlag. Die Sequenzstufe der Pipeline bleibt reine Vermerkung
(§3.3); sie ist an der Angriffsverarbeitung nicht beteiligt. Nachweis und
Teilbefund: `docs/Security.md` Abschnitt 4.7; normative Fassung
`docs/Kampfsystem.md` §3.1.

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
