# Raid Boss Class for MMORPG

extends Boss

# Raid boss specific properties
class_name RaidBoss

var raid_instance_id: String = ""
var difficulty: int = 1 # 1-5 scale
var max_difficulty: int = 5
var boss_type: String = "raid" 
var spawn_location: Vector2 = Vector2.ZERO
var spawn_timer: int = 0
var is_spawned: bool = false

# Raid-specific stats modifier
var difficulty_multiplier: float = 1.0
var raid_rewards: Array[Dictionary] = []

# Boss phase changes based on health
var phases: Array[Dictionary] = []
var current_phase: int = 0

func _ready():
    """Initialize raid boss"""
    super._ready()
    setup_phases()
    setup_raid_rewards()

func setup_phases():
    """Setup boss phases based on difficulty"""
    # Clear any existing phases
    phases.clear()
    
    if difficulty == 1:
        # Easy phase (single)
        phases = [
            {
                "name": "Phase 1",
                "health_threshold": 1.0,
                "speed_multiplier": 1.0,
                "attack_multiplier": 1.0,
                "special_attacks": ["Feuerball"]
            }
        ]
    elif difficulty == 2:
        # Medium
        phases = [
            {
                "name": "Phase 1",
                "health_threshold": 0.75,
                "speed_multiplier": 1.2,
                "attack_multiplier": 1.2,
                "special_attacks": ["Feuerball"]
            },
            {
                "name": "Phase 2",
                "health_threshold": 0.33,
                "speed_multiplier": 1.5,
                "attack_multiplier": 1.5,
                "special_attacks": ["Feuerball", "Sturmangriff"]
            }
        ]
    else:
        # Hard/Expert
        phases = [
            {
                "name": "Phase 1",
                "health_threshold": 0.75,
                "speed_multiplier": 1.3,
                "attack_multiplier": 1.3,
                "special_attacks": ["Feuerball"]
            },
            {
                "name": "Phase 2",
                "health_threshold": 0.5,
                "speed_multiplier": 1.6,
                "attack_multiplier": 1.6,
                "special_attacks": ["Feuerball", "Sturmangriff"]
            },
            {
                "name": "Phase 3",
                "health_threshold": 0.25,
                "speed_multiplier": 2.0,
                "attack_multiplier": 2.0,
                "special_attacks": ["Feuerball", "Sturmangriff", "Todesfluch"]
            }
        ]

func setup_raid_rewards():
    """Setup raid rewards based on difficulty"""
    raid_rewards = []
    
    if difficulty == 1:
        raid_rewards = [
            {"item": "Eisenhelm", "quantity": 1},
            {"item": "Eisenrüstung", "quantity": 1},
            {"item": "Eisenschwert", "quantity": 1}
        ]
    elif difficulty == 2:
        raid_rewards = [
            {"item": "Elfenhandschuhe", "quantity": 1},
            {"item": "Diamantstiefel", "quantity": 1},
            {"item": "Feuerstab", "quantity": 1}
        ]
    elif difficulty >= 3:
        raid_rewards = [
            {"item": "Drachenarmbrust", "quantity": 1},
            {"item": "Ewiges Schwert", "quantity": 1},
            {"item": "Göttlicher Robe", "quantity": 1}
        ]

func spawn_boss(location: Vector2):
    """Spawn boss at location"""
    spawn_location = location
    position = location
    is_spawned = true
    
    # Set difficulty based stats
    set_difficulty_stats(difficulty)
    
    # Send message
    send_combat_message("Ein mächtiger Boss erscheint!", 2)

func set_difficulty_stats(level: int):
    """Set boss stats based on difficulty level"""
    difficulty_multiplier = 1.0 + (level * 0.2)
    
    # Adjust health, attack, defense stats
    max_health = int(1000 * difficulty_multiplier)
    health = max_health
    attack_damage = int(50 * difficulty_multiplier)
    defense = int(20 * difficulty_multiplier)

func _process(delta: float):
    """Process boss updates"""
    if is_spawned and is_combat_active:
        super.update(delta)
        
        # Phase management
        check_phase_change()

func update(delta: float):
    """Update raid boss every frame"""
    if is_spawned and is_combat_active:
        super.update(delta)

func take_damage(damage: int, damage_type: String = "physical") -> bool:
    """Take damage with phase progression"""
    # Apply damage reduction
    var actual_damage = max(1, damage - defense)
    
    health -= actual_damage
    
    if health <= 0:
        health = 0
        return true  # Boss defeated
        
    # Check for phase change
    check_phase_change() 
    
    return false

func check_phase_change():
    """Check and handle phase changes"""
    if phases.size() > 0 and current_phase < phases.size():
        var current_threshold = phases[current_phase].health_threshold
        
        if get_health_percentage() <= current_threshold:
            # Move to next phase
            current_phase += 1
            
            # Apply phase effects
            apply_phase_effect(phases[current_phase - 1])
            
            # Announce phase change
            send_combat_message("Ich bin nicht mehr dasselbe Ding!", 1)
    
    # Call parent class phase change
    super.check_phase_change()

func apply_phase_effect(phase_data: Dictionary):
    """Apply effects of boss phase"""
    if phase_data.has("speed_multiplier"):
        speed *= phase_data.speed_multiplier
        
    if phase_data.has("attack_multiplier"):
        attack_damage = int(attack_damage * phase_data.attack_multiplier)
        
    # Special attacks could also be introduced here
    if phase_data.has("special_attacks"):
        for attack_name in phase_data.special_attacks:
            send_combat_message("Neue攻击 erschienen!", 1)

# Boss-specific combat behavior
func start_combat(players: Array[CharacterBody2D]):
    """Start raid boss combat"""
    super.start_combat(players)
    
    # Play spawn sound or animation if needed
    # animate_spawn()  # could be implemented
    
    # Send announcement
    send_combat_message("Ein mächtiger Räuber erscheint!", 2)
    
func end_combat(victory: bool = false):
    """End raid boss combat"""
    super.end_combat(victory)
    
    if victory:
        # Give raid rewards
        give_raid_rewards()
        
        # Trigger victory cutscene
        trigger_victory_cutscene()
    else:
        # Trigger defeat cutscene if needed
        trigger_defeat_cutscene()

func give_raid_rewards():
    """Give rewards to players in combat"""
    for player_ref in players_in_range:
        var player = player_ref.get_ref()
        if player != null and player is CharacterBody2D:
            # Would integrate with your inventory system
            print("Boss gewährt Belohnungen...")
            
            # Send message to player about rewards
            send_combat_message("Erhaltene Belohnungen von " + boss_name, 1)
            
            # Process each reward
            for reward in raid_rewards:
                var item = reward.item
                var quantity = reward.quantity
                print("Belohnung: " + item + " x" + str(quantity))

func set_raid_instance_id(id: String):
    """Set the unique ID for this raid instance"""
    raid_instance_id = id

func get_boss_info() -> Dictionary:
    """Get boss information for UI or logs"""
    return {
        "name": boss_name,
        "level": level,
        "health": health,
        "max_health": max_health,
        "boss_type": boss_type,
        "difficulty": difficulty,
        "phase": current_phase + 1,
        "is_active": is_active
    }

# Utility methods for raid management
func reset_stats():
    """Reset boss stats for new instance"""
    health = max_health
    current_phase = 0
    speed = 1.0
    attack_damage = 50
    defense = 20

func is_ready_to_fight() -> bool:
    """Check if raid boss is ready to fight"""
    return is_spawned and is_active

func prepare_for_combat():
    """Prepare boss for combat"""
    spawn_timer = 0  # Reset any spawn timer