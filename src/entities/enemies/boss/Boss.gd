# Raid Boss System for MMORPG

## Boss.gd - Base Boss Class

```gdscript
extends CharacterBody2D

# Base boss class with raid functionality
class_name Boss

# Basic stats
var health: int = 1000
var max_health: int = 1000
var attack_damage: int = 50
var defense: int = 20
var speed: float = 1.0

# Boss-specific properties
var boss_name: String = "Unbekannter Boss"
var boss_type: String = "normal" # normal, elite, raid, world_boss
var level: int = 50
var is_active: bool = false
var phase: int = 1
var max_phases: int = 3

# Combat messages and taunts
var taunt_messages: Array[String] = []
var victory_messages: Array[String] = []
var defeat_messages: Array[String] = []
var combo_attack_messages: Array[String] = []

# Combat state management
var is_combat_active: bool = false
var players_in_range: Array[WeakRef] = []
var combat_timer: int = 0
var next_taunt_time: int = 0

# Cutscene system
var cutscene_manager = null
var active_cutscenes: Array[String] = []

# Boss abilities and combo attacks
var combo_attacks: Array[Dictionary] = []
var is_casting: bool = false
var casting_time: int = 0
var casting_duration: int = 0

func _ready():
    """Initialize boss"""
    # Setup default taunts
    setup_default_taunts()
    
    # Setup abilities and combos
    setup_abilities()
    
    # Setup cutscene manager (should be connected to main scene)
    cutscene_manager = get_node_or_null("/root/CutsceneManager")
    
    # Connect to main scene for player detection
    connect_to_scene()

func setup_default_taunts():
    """Setup default taunt messages"""
    taunt_messages = [
        "Ihr seid so schwach! Ich kann euch mit einer Hand bekämpfen!",
        "Kommt, ich habe Zeit für euch!",
        "Meine Macht ist unbesiegbar!",
        "Ach, ein neuer Held? Wie niedlich.",
        "Ich werde eure Herzen brechen!",
        "Eure Würde wird von mir zerstört!",
        "Ein letzter Atemzug vor meiner mächtigen Niederlage!",
        "Ihr seid alle verloren!"
    ]
    
    victory_messages = [
        "Ihr habt es nicht geschafft! Ich bin unbesiegbar!",
        "Eure Helden sind wie Fliegen gegen mich!",
        "Ich bin die Unbesiegbarkeit selbst!",
        "Ein Sieg, den ihr verdient habt... in der Niederlage!"
    ]
    
    defeat_messages = [
        "Das ist... nicht möglich!",
        "Nicht... endet so...",
        "Ihr seid... stark...",
        "Eure Macht... überrascht mich..."
    ]
    
    combo_attack_messages = [
        "Seht euch das an!",
        "Zu schwach!",
        "Könnt ihr es verkraften?",
        "Diese Attacke hat euren Körper zerstört!",
        "Euer Blut wird meine Kraft verstärken!",
        "Schwach! So schwach!"
    ]

func setup_abilities():
    """Setup boss abilities and combo attacks"""
    combo_attacks = [
        {
            "name": "Feuerball",
            "damage": 150,
            "description": "Verzehrt den Feind mit Flammen",
            "cooldown": 30,
            "casting_time": 2
        },
        {
            "name": "Sturmangriff",
            "damage": 200,
            "description": "Hagelt über die Spieler",
            "cooldown": 45,
            "casting_time": 3
        },
        {
            "name": "Todesfluch",
            "damage": 300,
            "description": "Ein Fluch, der die Herzen zerrissen",
            "cooldown": 60,
            "casting_time": 4
        }
    ]

func connect_to_scene():
    """Connect to main game scene for player detection and events"""
    pass

func start_combat(players: Array[CharacterBody2D]):
    """Start boss combat with given players"""
    is_combat_active = true
    is_active = true
    
    # Register players in combat
    players_in_range.clear()
    for player in players:
        if player != null:
            players_in_range.append(weakref(player))
    
    # Send initial message to players
    send_combat_message("Neue Herausforderung aufgetreten!", 1)
    
    # Start combat timer
    combat_timer = 0
    
func end_combat(victory: bool = false):
    """End boss combat"""
    is_combat_active = false
    is_active = false
    
    if victory:
        send_combat_message(get_random_victory_message(), 2)
        trigger_defeat_cutscene()
    else:
        send_combat_message(get_random_defeat_message(), 2)
        trigger_victory_cutscene()

func update(delta: float):
    """Update boss every frame"""
    if is_combat_active:
        combat_timer += delta
        
        # Check for taunts periodically
        if combat_timer > next_taunt_time and players_in_range.size() > 0:
            perform_random_taunt()
            next_taunt_time = combat_timer + 5.0 + randf() * 10.0
    
    # Update casting status if active
    if is_casting and casting_time > 0:
        casting_time -= delta
        if casting_time <= 0:
            finish_casting()

func perform_random_taunt():
    """Perform a random taunt to players"""
    if taunt_messages.size() > 0:
        var message = taunt_messages[randi() % taunt_messages.size()]
        send_combat_message(message, 0)
        
        # Also trigger a random combo attack occasionally
        if randf() > 0.7:  # 30% chance for combo attack
            perform_combo_attack()

func send_combat_message(message: String, message_type: int = 0):
    """Send combat message to all players in range"""
    for player_ref in players_in_range:
        var player = player_ref.get_ref()
        if player != null and player is CharacterBody2D:
            # Could integrate with your UI system or HUD
            print("[Boss " + boss_name + "] " + message)
            
            # If using a chat system, call it here:
            # player.add_to_chat(message)

func get_random_taunt():
    """Get random taunt message"""
    if taunt_messages.size() > 0:
        return taunt_messages[randi() % taunt_messages.size()]
    return "Ich bin unbesiegbar!"

func get_random_victory_message():
    """Get random victory message"""
    if victory_messages.size() > 0:
        return victory_messages[randi() % victory_messages.size()]
    return "Ihr seid nicht stark genug!"

func get_random_defeat_message():
    """Get random defeat message"""
    if defeat_messages.size() > 0:
        return defeat_messages[randi() % defeat_messages.size()]
    return "Ich bin noch nicht besiegt!"

func perform_combo_attack():
    """Perform a boss combo attack"""
    var available_attacks = []
    
    for attack in combo_attacks:
        # Check if attack is on cooldown
        if !is_attack_on_cooldown(attack):
            available_attacks.append(attack)
    
    if available_attacks.size() > 0:
        var attack = available_attacks[randi() % available_attacks.size()]
        send_combat_message(get_random_combo_attack_message(), 1)
        
        # Start casting
        start_casting_attack(attack)

func start_casting_attack(attack_data: Dictionary):
    """Start casting an attack"""
    is_casting = true
    casting_time = attack_data.casting_time
    casting_duration = attack_data.casting_time
    
    # Send casting message
    send_combat_message("Vorbereitung auf " + attack_data.name + "!", 1)
    
    # Send visual cues here if using UI or effects
    
func finish_casting():
    """Finish casting and execute the attack"""
    is_casting = false
    
    # Execute the actual attack here - would need to call methods that affect 
    # player health based on casting_duration and attack_data
    execute_attack()

func execute_attack():
    """Execute the current boss attack"""
    # This is where you would implement actual damage logic to players
    pass

func is_attack_on_cooldown(attack_data: Dictionary) -> bool:
    """Check if an attack is on cooldown"""
    # This would need more complex logic for individual attacks
    return false

func get_health_percentage() -> float:
    """Get current health percentage"""
    if max_health > 0:
        return float(health) / float(max_health)
    return 1.0
    
func take_damage(damage: int, damage_type: String = "physical") -> bool:
    """Take damage from player attack"""
    # Apply damage reduction based on defense
    var actual_damage = max(1, damage - defense)
    
    health -= actual_damage
    
    if health <= 0:
        health = 0
        return true  # Boss is defeated
        
    return false

func check_phase_change():
    """Check if boss should change phase"""
    var health_percent = get_health_percentage()
    
    if health_percent < 0.66 and phase == 1:
        phase = 2
        send_combat_message("Ich werde stärker!", 1)
    elif health_percent < 0.33 and phase == 2:
        phase = 3
        send_combat_message("Meine Kraft ist unglaublich! Ich bin unsterblich!", 1)

func trigger_cutscene(cutscene_name: String):
    """Trigger a cutscene related to this boss"""
    if cutscene_manager:
        cutscene_manager.play_cutscene(cutscene_name)
        active_cutscenes.append(cutscene_name)

func trigger_defeat_cutscene():
    """Trigger defeat cutscene"""
    trigger_cutscene("boss_defeat")

func trigger_victory_cutscene():
    """Trigger victory cutscene"""
    trigger_cutscene("boss_victory")
    
func on_player_enter(player: CharacterBody2D):
    """Handle player entering boss detection zone"""
    if !is_combat_active:
        start_combat([player])

func on_player_exit(player: CharacterBody2D):
    """Handle player leaving boss detection zone"""
    pass