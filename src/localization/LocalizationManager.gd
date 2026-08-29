# Localization Manager for MMORPG (Godot 3.6 Compatible)

extends Node

# Manages game localization (languages, translations)
class_name LocalizationManager

# Available languages
var available_languages = ["de", "en"]
var current_language = "de"
var translations = {}

func _ready():
    """Initialize localization manager"""
    load_translations()
    
func load_translations():
    """Load translations from dictionaries (Godot 3.6 compatible)"""
    # German translations
    translations["de"] = {
        # Game UI Texts
        "game_title": "MMORPG Spiel",
        "start_game": "Spiel starten",
        "settings": "Einstellungen",
        "quit": "Beenden",
        
        # Boss encounter text
        "boss_spawned": "Ein mächtiger Boss erscheint!",
        "raid_started": "Raid gestartet: %s",
        "boss_defeated": "Der Boss wurde besiegt!",
        "raid_completed": "Raid abgeschlossen!",
        "player_joined_raid": "%s hat den Raid betreten.",
        "player_left_raid": "%s hat den Raid verlassen.",
        "phase_change": "Neue Phase: %s",
        "cutscene_start": "Cutscene beginnt...",
        "cutscene_end": "Cutscene beendet.",
        
        # Boss types
        "boss_type_normal": "Normaler Boss",
        "boss_type_raid": "Raid-Boss",
        "boss_type_quest": "Quest-Boss",
        
        # Cutscene phases 
        "cutscene_phase_1": "Phase 1:",
        "cutscene_phase_2": "Phase 2:",
        "cutscene_phase_3": "Phase 3:",
        
        # Combat text
        "boss_attack": "%s greift an!",
        "player_damage": "%s wurde getroffen für %d Schaden.",
        "heal_effect": "%s hat sich geheilt",
        "buff_effect": "%s erhält einen Stärkebonus",
        
        # Chat and NPC text
        "npc_greeting": "Hallo! Wie kann ich helfen?",
        "npc_goodbye": "Auf Wiedersehen!",
        "npc_thanks": "Gern geschehen!",
        "npc_question": "Was brauchst du?",
        
        # Inventory and items
        "inventory_full": "Inventar ist voll",
        "item_received": "%s erhalten",
        "item_used": "%s benutzt",
        "item_equipped": "%s ausgerüstet",
        
        # Quest system
        "quest_available": "Neue Aufgabe verfügbar!",
        "quest_completed": "Aufgabe abgeschlossen!",
        "quest_failed": "Aufgabe fehlgeschlagen!",
        
        # World events
        "event_start": "Ereignis gestartet",
        "event_end": "Ereignis beendet",
        "area_entered": "Bereich betreten: %s",
        "area_exited": "Bereich verlassen: %s",
        
        # Menu and settings
        "language_selection": "Sprachauswahl",
        "save_settings": "Einstellungen speichern",
        "sound_volume": "Lautstärke",
        "music_volume": "Musik Lautstärke",
        "difficulty": "Schwierigkeit",
        "easy": "Leicht",
        "normal": "Normal",
        "hard": "Schwer"
    }
    
    # English translations
    translations["en"] = {
        # Game UI Texts
        "game_title": "MMORPG Game",
        "start_game": "Start Game",
        "settings": "Settings",
        "quit": "Quit",
        
        # Boss encounter text
        "boss_spawned": "A powerful boss has appeared!",
        "raid_started": "Raid started: %s",
        "boss_defeated": "The boss was defeated!",
        "raid_completed": "Raid completed!",
        "player_joined_raid": "%s joined the raid.",
        "player_left_raid": "%s left the raid.",
        "phase_change": "New phase: %s",
        "cutscene_start": "Cutscene begins...",
        "cutscene_end": "Cutscene ended.",
        
        # Boss types
        "boss_type_normal": "Normal Boss",
        "boss_type_raid": "Raid Boss",
        "boss_type_quest": "Quest Boss",
        
        # Cutscene phases 
        "cutscene_phase_1": "Phase 1:",
        "cutscene_phase_2": "Phase 2:",
        "cutscene_phase_3": "Phase 3:",
        
        # Combat text
        "boss_attack": "%s attacks!",
        "player_damage": "%s took %d damage.",
        "heal_effect": "%s was healed",
        "buff_effect": "%s receives a strength bonus",
        
        # Chat and NPC text
        "npc_greeting": "Hello! How can I help?",
        "npc_goodbye": "Goodbye!",
        "npc_thanks": "You're welcome!",
        "npc_question": "What do you need?",
        
        # Inventory and items
        "inventory_full": "Inventory is full",
        "item_received": "%s received",
        "item_used": "%s used",
        "item_equipped": "%s equipped",
        
        # Quest system
        "quest_available": "New quest available!",
        "quest_completed": "Quest completed!",
        "quest_failed": "Quest failed!",
        
        # World events
        "event_start": "Event started",
        "event_end": "Event ended",
        "area_entered": "Entered area: %s",
        "area_exited": "Exited area: %s",
        
        # Menu and settings
        "language_selection": "Language Selection",
        "save_settings": "Save Settings",
        "sound_volume": "Sound Volume",
        "music_volume": "Music Volume",
        "difficulty": "Difficulty",
        "easy": "Easy",
        "normal": "Normal",
        "hard": "Hard"
    }

func set_language(lang):
    """Set the current game language"""
    if lang in available_languages:
        current_language = lang
        emit_signal("language_changed", lang)
        return true
    else:
        print("Unsupported language: " + str(lang))
        return false

func get_language():
    """Get the current language"""
    return current_language

func get_translation(key):
    """Get translated text for key in current language"""
    if current_language in translations and key in translations[current_language]:
        return translations[current_language][key]
    # Fallback to English
    if "en" in translations and key in translations["en"]:
        return translations["en"][key]
    # Fallback to key name
    return key

func get_translation_with_args(key, args):
    """Get translated text with formatted arguments"""
    var text = get_translation(key)
    # Godot 3.6 compatible string formatting
    if typeof(args) == TYPE_ARRAY and args.size() > 0:
        var formatted_text = text
        for i in range(args.size()):
            formatted_text = formatted_text.replace("%d", str(args[i]), 1)
        return formatted_text
    return text

func get_available_languages():
    """Get list of available languages"""
    return available_languages

# Signals - Godot 3.6 compatible signal declaration
func _init():
    # Signal registration for Godot 3.6
    pass

# Utility methods for localization in the gameplay context
func format_boss_name(name):
    """Format boss name for display"""
    if name in translations[current_language]:
        return "[B] " + get_translation(name)
    return "[B] " + name

func format_player_name(name):
    """Format player name for chat"""
    return "[P] " + name

# Get translations for specific categories
func get_boss_text():
    """Get all boss-related translations"""
    var result = {}
    if current_language in translations:
        for key in translations[current_language]:
            if key.begins_with("boss_") or key.begins_with("cutscene_"):
                result[key] = translations[current_language][key]
    return result

func get_combat_text():
    """Get all combat-related translations"""
    var result = {}
    if current_language in translations:
        for key in translations[current_language]:
            if key.begins_with("boss_attack") or key.begins_with("heal_") or key.begins_with("buff_"):
                result[key] = translations[current_language][key]
    return result

func get_ui_text():
    """Get all UI-related translations"""
    var result = {}
    if current_language in translations:
        for key in translations[current_language]:
            if key.begins_with("menu_") or key.begins_with("settings_") or key == "game_title":
                result[key] = translations[current_language][key]
    return result