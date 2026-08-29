# Godot NPC System

## Base NPC Script (NPC.gd)

```gdscript
extends CharacterBody2D

# Basic NPC properties
var npc_id = ""
var name = "Unbekannter NPC"
var npc_type = "civilian"  # civilian, merchant, quest_giver, combatant
var level = 1
var faction = "neutral"

# Stats
var health = 100
var max_health = 100
var strength = 10
var agility = 10
var intelligence = 10

# Dialogue system
var dialogue_tree = {}
var can_talk = true
var is_interacting = false

# AI behavior
var ai_behavior = "wander"  # wander, patrol, guard, follow_player
var patrol_points = []
var current_patrol_point = 0
var idle_time = 0.0
var idle_duration = 3.0

# Quest interactions
var associated_quests = []
var quest_rewards = {}

func _ready():
    # Set NPC properties based on type
    setup_npc_type()

func setup_npc_type():
    """Setup NPC based on its type"""
    match npc_type:
        "merchant":
            name = "Händler " + name
            can_talk = true
        "quest_giver":
            name = "Questgeber " + name
            can_talk = true
        "combatant":
            name = "Kämpfer " + name
            faction = "hostile"
        "civilian":
            name = "Bürger " + name
            faction = "neutral"

func _process(delta):
    """Update NPC behavior"""
    if ai_behavior == "wander":
        wander_behavior(delta)
    elif ai_behavior == "patrol":
        patrol_behavior(delta)

func wander_behavior(delta):
    """Basic wandering AI"""
    # Simple wandering movement
    move_and_slide(Vector2(randf_range(-1.0, 1.0), randf_range(-1.0, 1.0)) * 50 * delta)
    
    # Random stopping and turning
    if randf() < 0.02:
        velocity = Vector2.ZERO

func patrol_behavior(delta):
    """Patrol behavior with waypoints"""
    if patrol_points.size() == 0:
        return
    
    var target_pos = patrol_points[current_patrol_point]
    
    # Move towards current patrol point
    var direction = (target_pos - position).normalized()
    velocity = direction * 50
    
    # Check if reached patrol point
    if position.distance_to(target_pos) < 10:
        current_patrol_point = (current_patrol_point + 1) % patrol_points.size()

func start_interaction():
    """Start interaction with player"""
    is_interacting = true
    print(name + " is now interacting")

func end_interaction():
    """End interaction with player"""
    is_interacting = false
    print(name + " ended interaction")

func talk_to_player():
    """Handle talking to the player"""
    if can_talk:
        start_interaction()
        # Show dialogue window here
        display_dialogue()
        end_interaction()

func display_dialogue():
    """Display NPC's dialogue tree"""
    # In a real implementation, this would open a dialog UI
    print("Dialogue with " + name + ":")
    # This is where your dialogue system would show the dialog

func give_quest(quest_id):
    """Give a quest to the player"""
    if not associated_quests.has(quest_id):
        associated_quests.append(quest_id)
        print(name + " gave quest: " + quest_id)

func take_damage(amount):
    """Handle NPC taking damage"""
    health -= amount
    if health <= 0:
        die()

func die():
    """Handle NPC death"""
    print(name + " died")
    # Drop loot here
    queue_free()

# Get NPC info for UI
func get_npc_info():
    return {
        "id": npc_id,
        "name": name,
        "type": npc_type,
        "level": level,
        "health": health,
        "max_health": max_health,
        "faction": faction,
        "is_interacting": is_interacting
    }
```

## NPC Manager Script (NPCManager.gd)

```gdscript
extends Node

# List of all NPCs in the world
var npcs = {}
var active_dialogue = null

func _ready():
    # Load sample NPCs
    load_sample_npcs()

func load_sample_npcs():
    # Sample merchant NPC
    var merchant = create_npc("merchant_001", "Händler")
    merchant.name = "Bert"
    merchant.npc_type = "merchant"
    
    npcs[merchant.npc_id] = merchant
    
    # Sample quest giver
    var quest_giver = create_npc("quest_giver_001", "Questgeber")
    quest_giver.name = "Elena"
    quest_giver.npc_type = "quest_giver"
    quest_giver.associated_quests = ["intro_001", "monster_001"]
    
    npcs[quest_giver.npc_id] = quest_giver
    
    # Sample combatant
    var combatant = create_npc("combatant_001", "Kämpfer")
    combatant.name = "Räuber"
    combatant.npc_type = "combatant"
    combatant.faction = "hostile"
    combatant.strength = 20
    
    npcs[combatant.npc_id] = combatant

func create_npc(npc_id, name):
    """Create a new NPC instance"""
    var npc = NPC.new()
    npc.npc_id = npc_id
    npc.name = name
    
    # Set default properties
    npc.level = 1
    npc.health = 100
    npc.max_health = 100
    
    return npc

func get_npc(npc_id):
    """Get specific NPC by ID"""
    if npcs.has(npc_id):
        return npcs[npc_id]
    return null

func spawn_npc(npc_type, position):
    """Spawn a new NPC at given position"""
    var npc = create_npc("npc_" + str(Time.get_ticks_msec()), npc_type)
    npc.position = position
    npcs[npc.npc_id] = npc
    
    print("NPC spawned: " + npc.name)
    return npc

func spawn_npc_at_random_location(npc_type):
    """Spawn NPC at a random location in the world"""
    # This would be more complex in a real game with proper world boundaries
    var position = Vector2(randf_range(0, 100), randf_range(0, 100))
    return spawn_npc(npc_type, position)

func get_all_npcs():
    """Get list of all NPCs"""
    return npcs

func get_npc_by_position(position):
    """Find NPC at specific position"""
    for npc_id in npcs:
        var npc = npcs[npc_id]
        if position.distance_to(npc.position) < 50:  # Within 50 units
            return npc
    return null

func handle_npc_interaction(npc_id, interaction_type):
    """Handle interaction with NPC"""
    if not npcs.has(npc_id):
        return false
    
    var npc = npcs[npc_id]
    
    match interaction_type:
        "talk":
            npc.talk_to_player()
            return true
        "trade":
            # Open trading UI
            return true
        "quest":
            # Show available quests
            return true
        _:
            return false

func get_npc_spawns():
    """Get list of NPC spawn points"""
    # This would return predefined spawn locations
    return [
        {"position": Vector2(100, 100), "type": "merchant"},
        {"position": Vector2(300, 200), "type": "quest_giver"},
        {"position": Vector2(500, 400), "type": "combatant"}
    ]
```

## NPC Dialogue Manager (DialogueManager.gd)

```gdscript
extends Node

# Dialogue system for NPCs
var current_dialogue = null
var dialogue_options = []

func _ready():
    # Initialize sample dialogues
    init_sample_dialogues()

func init_sample_dialogues():
    # Sample merchant dialogue
    var merchant_dialogue = {
        "id": "merchant_001",
        "greetings": [
            "Hallo, Abenteurer! Ich habe für dich interessante Waren.",
            "Willkommen bei meinem Laden!",
            "Bist du auf der Suche nach Ausrüstung?"
        ],
        "quests": [
            "Ich brauche jemanden, der meine Waren in die nächste Stadt bringt.",
            "Könntest du vielleicht etwas für mich ernten? Die Scherben sind gefährlich."
        ],
        "farewells": [
            "Bis zum nächsten Mal!",
            "Viel Glück auf deinem Weg!",
            "Komm wieder, wenn du mehr brauchst!"
        ]
    }
    
    # Sample quest giver dialogue
    var quest_giver_dialogue = {
        "id": "quest_giver_001",
        "greetings": [
            "Ah, eine neue Gestalt! Ich bin hier um dir zu helfen!",
            "Freut mich sehr, dich kennenzulernen!"
        ],
        "quests": [
            "Ich habe eine Aufgabe für dich. Besiege 5 Slimes in der Wildnis.",
            "Ein alter Freund von mir benötigt deine Hilfe."
        ],
        "farewells": [
            "Geh mit Mut!",
            "Gute Reise, Kämpfer!"
        ]
    }

func start_dialogue(npc_id, dialogue_type):
    """Start a dialogue with an NPC"""
    # This would load the appropriate dialogue text
    print("Starting dialogue with NPC: " + npc_id)
    
    # Show dialogue UI here
    return true

func get_dialogue_options(npc_id):
    """Get available options for NPC dialogue"""
    # This would be more complex with actual dialogues
    return ["Gespräch beginnen", "Handel treiben", "Quests ansehen"]

func process_dialogue_selection(option_text):
    """Process selection from dialogue options"""
    print("Selected option: " + option_text)
    
    # This would trigger appropriate actions based on choice
```

## Sample NPC Data File (npcs/sample_npcs.json)

```json
{
  "npcs": [
    {
      "id": "merchant_001",
      "name": "Bert",
      "type": "merchant",
      "level": 1,
      "faction": "neutral",
      "position": {"x": 100, "y": 100},
      "dialogue": {
        "greetings": [
          "Hallo, Abenteurer! Ich habe für dich interessante Waren.",
          "Willkommen bei meinem Laden!"
        ]
      }
    },
    {
      "id": "quest_giver_001",
      "name": "Elena",
      "type": "quest_giver",
      "level": 5,
      "faction": "neutral",
      "position": {"x": 300, "y": 200},
      "dialogue": {
        "greetings": [
          "Ah, eine neue Gestalt! Ich bin hier um dir zu helfen!",
          "Freut mich sehr, dich kennenzulernen!"
        ]
      }
    },
    {
      "id": "combatant_001",
      "name": "Räuber",
      "type": "combatant",
      "level": 5,
      "faction": "hostile",
      "position": {"x": 500, "y": 400},
      "dialogue": {
        "greetings": [
          "Hast du das nicht besser in deinem Haus gelassen?",
          "Du machst dir hier keine Sorgen!"
        ]
      }
    }
  ]
}
```