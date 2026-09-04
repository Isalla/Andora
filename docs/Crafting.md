Andora – Item Quality & Crafting Overcap

Allgemeines Quality-System

Quality ist eine gemeinsame Basiseigenschaft qualitätsfähiger Items.
Sie gilt nicht nur für Equipment, sondern auch für Crafting-Materialien, Rohstoffe, Verbrauchsgegenstände usw.
Aktuelle Quality-Stufen:
1 – Gray / Poor
2 – Green / Common
3 – Blue / Uncommon
4 – Yellow / Rare
5 – Orange / Epic
6 – Purple / Legendary

Loot Quality Range

Lootquellen können eine minimale und maximale Quality besitzen.
Eine Master Chest garantiert mindestens ein Legendary.
Weitere Items einer Master Chest liegen innerhalb der definierten Quality Range.
Vorgesehene Master-Range: Rare → Legendary.
Dadurch können neben dem Legendary beispielsweise Rare/Epic Crafting-Materialien oder andere Items enthalten sein.
Nicht jeder Gegenstand einer hochwertigen Truhe muss Legendary sein.

Crafting
Die Qualität eines hergestellten Gegenstands kann unter anderem beeinflusst werden durch:

Rezept
Materialqualität
Handwerker-Skill
Werkzeug
Werkstatt
besondere Zutaten
weitere Crafting-Modifikatoren

Legendary Materialien erhöhen damit die Voraussetzungen für ein außergewöhnliches Ergebnis, garantieren aber nicht automatisch einen außergewöhnlichen Overcap.

Normale Qualitätsgrenze

0–100 %

100 % stellt regulär das bestmögliche Crafting-Ergebnis dar.

Extrem seltener Overcap

Bei optimalen Voraussetzungen kann ein Gegenstand 100 % erreichen.
Es besteht eine extrem seltene Chance von 0,0001 %, dass die 100-%-Grenze überschritten werden darf.
Diese Chance öffnet nur den Overcap.
Sie bestimmt nicht unmittelbar den endgültigen Wert.
Wie weit das Ergebnis über 100 % liegt, wird weiterhin durch die Crafting-Berechnung bestimmt.

Normales Hardcap

105 %

Kein regulär hergestellter Gegenstand darf diesen Wert überschreiten.

Event-Hardcap

Bei besonderen Events kann die erlaubte Obergrenze temporär angehoben werden:

Normal:  maximal 105 %
Event:   maximal 110 %

110 % ist die vorgesehene absolute technische Obergrenze. Auch fehlerhafte Content-/Lua-Werte dürfen sie nicht überschreiten.

Masterwork
Ein Overcap erzeugt keine neue Quality-Farbe. Ein solches Item bleibt beispielsweise:

quality       = legendary
quality_score = 103.7
masterwork    = true

Ein Event-Gegenstand könnte entsprechend beispielsweise 108.4 % erreichen.

Balancing-/Sicherheitsregel

Overcap darf niemals zu unkontrollierter oder exponentieller Stat-Skalierung führen.

Der Quality Score darf daher nicht einfach sämtliche Itemwerte ungeprüft multiplizieren. Die jeweiligen Item-/Crafting-Regeln bestimmen, welche Eigenschaften vom Overcap profitieren.

So bleibt ein 104,8 % Legendary außergewöhnlich wertvoll, ohne im PvP plötzlich jeden Gegner mit einem Treffer umzuhauen.

Und ganz wichtig für die spätere Implementierung:

Lua darf Crafting- und Eventregeln definieren. Der Realm-Server (Rust) erzwingt jedoch das absolute 110-%-Hardcap unabhängig von den Contentdaten



