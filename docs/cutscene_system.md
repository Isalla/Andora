# Cutscene System Documentation

## Übersicht
Das Cutscene-System ermöglicht es, Story-Entwicklungen und Quest-Ereignisse durch Visualisierungen zu vertiefen. Cutscenes können sowohl in Quests als auch in besonderen Events verwendet werden.

## Funktionen

### 1. Automatische Cutscenes bei Gebietsbetritt
- Wenn Spieler bestimmte Bereiche betreten, können automatisch Cutscenes gestartet werden
- Verknüpfung zwischen Spielerevents und Cutscenes

### 2. Quest-basierte Cutscenes
- Verfolgung von Quest-Fortschritt
- Automatische Cutscene-Ausführung bei Schlüsselereignissen
- Integration mit Quest-System

### 3. Boss-Kampf-Cutscenes
- Vor und nach dem Kampf
- Fazit der Schlacht
- Belohnungs-Events mit Visualisierung

## Implementierung

### Cutscene-Klasse
```gdscript
extends Node
class_name Cutscene

# Cutscene-Eigenschaften
var cutscene_name: String = ""
var duration: float = 0.0
var is_active: bool = false
var triggers: Array[Variant] = [] # Quests, Bereichswechsel, Events

# Funktionen
func play() -> void:
    """Startet die Cutscene"""
    
func stop() -> void:
    """Beendet die Cutscene"""
    
func pause() -> void:
    """Pausiert die Cutscene"""
    
func resume() -> void:
    """Fortsetzen der Cutscene"""
```

### BossManager-Erweiterung
```gdscript
func trigger_cutscene(scene_name: String, on_exit: bool = false) -> void:
    """Triggert eine Cutscene basierend auf den Parametern"""
    
func register_cutscene_trigger(trigger_type: String, trigger_data: Dictionary) -> void:
    """Registriert einen Trigger für eine Cutscene"""

# Beispiel zur Integration in RaidBoss
func trigger_victory_cutscene():
    """Startet die Sieges-Cutscene"""
    
func trigger_defeat_cutscene():
    """Startet die Niederlage-Cutscene"""
```

## Anwendungsfälle

1. **Quests bei Gebietsbetritt**
   - Spieler tritt in ein bestimmtes Gebiet ein
   - Automatisch Story-Cutscene wird gestartet

2. **Boss-Kampf-Events**
   - Vor dem Kampf: Erscheinung der Boss-Kraft
   - Nach dem Kampf: Sieges- oder Niederlage-Cutscene

3. **Spezielle Events**
   - Besondere Ereignisse
   - Story-Ereignisse
   - Character-Entwicklungen