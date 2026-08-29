# Cutscene Manager for MMORPG

extends Node

# Manages cutscenes during boss fights and raid encounters
class_name CutsceneManager

# List of available cutscenes
var cutscenes: Dictionary = {}
var active_cutscene: String = ""
var is_playing_cutscene: bool = false

# Player reference
var player_ref = null

# Scene transition effects
var scene_transitions: Array[String] = ["fade_in", "fade_out", "zoom_in", "zoom_out"]

func _ready():
    """Initialize cutscene manager"""
    initialize_cutscenes()
    
func initialize_cutscenes():
    """Initialize available cutscenes"""
    # Setup default cutscenes for boss encounters
    cutscenes["boss_defeat"] = {
        "name": "Boss Niederlage",
        "description": "Boss besiegt und Belohnungen verteilt",
        "sequence": [
            {"type": "camera_zoom", "duration": 2.0, "zoom_factor": 1.5},
            {"type": "fade_out", "duration": 1.0},
            {"type": "wait", "duration": 1.0},
            {"type": "text_message", "content": "Der Boss wurde besiegt!", "duration": 3.0},
            {"type": "fade_in", "duration": 1.0}
        ],
        "events": ["rewards_given", "player_stats_updated", "cutscene_completed"]
    }
    
    cutscenes["boss_victory"] = {
        "name": "Boss Sieg",
        "description": "Boss gewinnt und Spieler werden besiegt",
        "sequence": [
            {"type": "camera_shake", "duration": 2.0, "intensity": 0.5},
            {"type": "fade_out", "duration": 1.0},
            {"type": "wait", "duration": 1.0},
            {"type": "text_message", "content": "Der Boss hat uns besiegt...", "duration": 3.0},
            {"type": "fade_in", "duration": 1.0}
        ],
        "events": ["player_defeated", "game_over", "cutscene_completed"]
    }
    
    cutscenes["raid_start"] = {
        "name": "Raid Start",
        "description": "Raid Beginn mit eingehenden Boss",
        "sequence": [
            {"type": "fade_out", "duration": 0.5},
            {"type": "wait", "duration": 0.5},
            {"type": "camera_zoom", "duration": 1.5, "zoom_factor": 2.0},
            {"type": "text_message", "content": "Ein mächtiger Boss erscheint!", "duration": 3.0},
            {"type": "fade_in", "duration": 0.5}
        ],
        "events": ["boss_spawned", "combat_started", "cutscene_completed"]
    }

func play_cutscene(cutscene_name: String):
    """Play a specific cutscene"""
    if !cutscenes.has(cutscene_name):
        print("Cutscene nicht gefunden: " + cutscene_name)
        return
        
    if is_playing_cutscene:
        print("Es läuft bereits ein Cutscene. Warte auf Beendigung...")
        return
    
    active_cutscene = cutscene_name
    is_playing_cutscene = true
    
    # Trigger the cutscene with a timer-based processing
    call_deferred("_run_cutscene", cutscene_name)
    
    print("Starte Cutscene: " + cutscene_name)

func _run_cutscene(cutscene_name: String):
    """Execute cutscene sequence"""
    var cutscene = cutscenes[cutscene_name]
    
    if !cutscene.has("sequence"):
        print("Ungültige Cutscene: " + cutscene_name)
        is_playing_cutscene = false
        return
    
    # Process cutscene sequence
    for step in cutscene.sequence:
        match step.type:
            "camera_zoom":
                execute_camera_zoom(step)
            "camera_shake":
                execute_camera_shake(step)
            "fade_out", "fade_in":
                execute_fade_transition(step)
            "text_message":
                execute_text_message(step)
            "wait":
                wait_for_duration(step.duration)
                
        # Add a small delay between steps
        await get_tree().create_timer(0.1).timeout

func execute_camera_zoom(data: Dictionary):
    """Execute camera zoom effect"""
    print("Camera Zoom: Faktor " + str(data.zoom_factor))
    # This would integrate with Godot's camera system
    
func execute_camera_shake(data: Dictionary):
    """Execute camera shake effect"""
    print("Camera Shake: Stärke " + str(data.intensity))
    # This would integrate with Godot's camera system
    
func execute_fade_transition(data: Dictionary):
    """Execute fade transition effect"""
    var direction = data.type
    var duration = data.duration
    print("Fade " + direction + ": Dauer " + str(duration) + " Sekunden")
    # This would integrate with Godot's UI or scene transitions
    
func execute_text_message(data: Dictionary):
    """Execute text message during cutscene"""
    var content = data.content
    var duration = data.duration
    print("Textnachricht: " + content)
    
    # Send to any UI element that displays messages
    emit_signal("text_displayed", content, duration)

func wait_for_duration(duration: float):
    """Wait for specified duration"""
    if duration > 0:
        await get_tree().create_timer(duration).timeout

func stop_cutscene():
    """Stop the currently playing cutscene"""
    is_playing_cutscene = false
    active_cutscene = ""
    print("Cutscene gestoppt")

func set_player_reference(player: CharacterBody2D):
    """Set reference to main player for cutscenes"""
    player_ref = weakref(player)

func get_active_cutscene() -> String:
    """Get name of currently playing cutscene"""
    return active_cutscene

func is_cutscene_playing() -> bool:
    """Check if a cutscene is currently playing"""
    return is_playing_cutscene

func add_cutscene(name: String, cutscene_data: Dictionary):
    """Add a new custom cutscene to the manager"""
    cutscenes[name] = cutscene_data
    
func remove_cutscene(name: String):
    """Remove a cutscene from the manager"""
    if cutscenes.has(name):
        cutscenes.erase(name)

# Signals for communication
signal text_displayed(message: String, duration: float)