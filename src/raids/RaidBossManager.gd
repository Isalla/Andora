# Raid Boss Manager with Localization Support

extends Node

# Manages raid instances, boss spawns, and player interactions with localization
class_name RaidBossManager

# Raid instances database
var raid_instances: Dictionary = {}
var active_raids: Array[String] = []
var boss_spawned: bool = false

# Player reference
var players_in_raid: Array[WeakRef] = []

# Boss information
var current_boss_ref = null
var current_boss_type: String = ""
var current_difficulty: int = 1

# Raid configuration
var max_raids_per_day: int = 10
var raid_cooldown_time: int = 300 # 5 minutes in seconds
var active_raid_timer: int = 0

# Localization manager reference
var localization_manager = null

func _ready():
    """Initialize raid manager with localization"""
    initialize_raids()
    # Try to get the localization manager from scene
    localization_manager = get_node_or_null("/root/LocalizationManager")
    if localization_manager == null:
        # Fallback to creating a new one (for standalone use)
        localization_manager = preload("res://src/localization/LocalizationManager.tscn").instantiate()
        add_child(localization_manager)

func initialize_raids():
    """Initialize boss types and difficulty settings"""
    # Configure raid rewards
    var raid_rewards = {
        "normal": [
            {"item": "Eisenhelm", "quantity": 1, "chance": 0.8},
            {"item": "Eisenrüstung", "quantity": 1, "chance": 0.7},
            {"item": "Eisenschwert", "quantity": 1, "chance": 0.6}
        ],
        "elite": [
            {"item": "Elfenhandschuhe", "quantity": 1, "chance": 0.7},
            {"item": "Diamantstiefel", "quantity": 1, "chance": 0.6},
            {"item": "Magische Klinge", "quantity": 1, "chance": 0.5}
        ],
        "raid": [
            {"item": "Drachenhelm", "quantity": 1, "chance": 0.9},
            {"item": "Drachenrüstung", "quantity": 1, "chance": 0.8},
            {"item": "Drachenschild", "quantity": 1, "chance": 0.7}
        ]
    }
    
    # Store rewards in instance
    $RaidRewards = raid_rewards

func start_raid(boss_name: String, difficulty: int = 1, spawn_location: Vector2 = Vector2.ZERO):
    """Start a new raid instance"""
    # Create unique raid ID
    var raid_id = generate_raid_id()
    
    # Setup raid instance data
    raid_instances[raid_id] = {
        "id": raid_id,
        "boss_name": boss_name,
        "difficulty": difficulty,
        "started_at": Time.get_ticks_msec(),
        "status": "active",
        "spawn_location": spawn_location,
        "players": [],
        "current_phase": 0
    }
    
    active_raids.append(raid_id)
    current_boss_type = boss_name
    current_difficulty = difficulty
    
    # Create and spawn the raid boss
    var boss = create_raid_boss(boss_name, difficulty)
    
    if boss != null:
        boss.spawn_boss(spawn_location)
        current_boss_ref = weakref(boss)
        
        # Register with cutscene manager for events
        var cutscene_manager = get_node_or_null("/root/CutsceneManager")
        if cutscene_manager:
            boss.cutscene_manager = cutscene_manager
        
        # Send message to players using localization
        announce_raid_start(raid_id, boss)
        
        # Start the boss cutscene sequence
        start_boss_cutscenes(boss)
        
        return raid_id
    
    return null

func create_raid_boss(boss_name: String, difficulty: int) -> RaidBoss:
    """Create a new raid boss instance"""
    var boss = preload("res://src/entities/enemies/boss/RaidBoss.tscn").instantiate()
    
    # Set boss properties
    boss.boss_name = boss_name
    boss.difficulty = difficulty
    
    # Add to scene
    add_child(boss)
    
    return boss

func generate_raid_id() -> String:
    """Generate a unique raid ID"""
    var timestamp = Time.get_ticks_msec()
    var random = randi() % 10000
    return "raid_" + str(timestamp) + "_" + str(random)

func get_localized_text(key: String, args: Array = []) -> String:
    """Get localized text with optional arguments"""
    if localization_manager != null:
        if args.size() > 0:
            return localization_manager.get_translation_with_args(key, args)
        else:
            return localization_manager.get_translation(key)
    # Fallback to simple text
    return key

func announce_raid_start(raid_id: String, boss: RaidBoss):
    """Announce raid start to players with localized messages"""
    var message = get_localized_text("boss_spawned")
    push_notification(message, 5)

func end_raid(raid_id: String, victory: bool = true):
    """End a raid instance and distribute rewards"""
    if !raid_instances.has(raid_id):
        print("Raid nicht gefunden: " + raid_id)
        return
    
    # Update raid status
    var raid_info = raid_instances[raid_id]
    raid_info.status = "completed"
    raid_info.completed_at = Time.get_ticks_msec()
    
    # Check if this was the active raid
    if active_raids.has(raid_id):
        active_raids.erase(raid_id)
    
    # Give rewards if player was victorious
    if victory and raid_info.has("players") and raid_info.players.size() > 0:
        give_raid_rewards(raid_id, boss_difficulty_to_reward_type(raid_info.difficulty))
    
    # Cleanup
    cleanup_raid_instance(raid_id)

func cleanup_raid_instance(raid_id: String):
    """Clean up raid instance"""
    if raid_instances.has(raid_id):
        raid_instances.erase(raid_id)
    
    # Remove boss from scene if needed
    if current_boss_ref != null:
        var boss = current_boss_ref.get_ref()
        if boss != null and boss.is_instance_valid(boss):
            # Remove boss from scene
            boss.queue_free()
        current_boss_ref = null

func give_raid_rewards(raid_id: String, reward_type: String):
    """Give rewards to players who participated in the raid"""
    var raid_info = raid_instances[raid_id]
    
    if !raid_info.has("players"):
        return
    
    # Iterate through all players who participated
    for player_ref in raid_info.players:
        var player = player_ref.get_ref()
        if player != null:
            give_player_rewards(player, reward_type)

func give_player_rewards(player: CharacterBody2D, reward_type: String):
    """Give rewards to a specific player"""
    # This would integrate with your actual inventory system
    print("Belohnungen für Spieler gegeben")

func get_player_id(player: CharacterBody2D) -> String:
    """Get unique ID for a player (simplified - you'd implement based on your needs)"""
    return str(player.get_instance_id())

func join_raid(raid_id: String, player: CharacterBody2D):
    """Allow player to join an active raid"""
    if !raid_instances.has(raid_id):
        print("Raid nicht gefunden: " + raid_id)
        return false
    
    if raid_instances[raid_id].status != "active":
        print("Raid ist nicht aktiv")
        return false
    
    # Check if player is already in the raid
    for existing_player_ref in raid_instances[raid_id].players:
        var existing_player = existing_player_ref.get_ref()
        if existing_player == player:
            return true  # Already joined
    
    # Add player to raid
    raid_instances[raid_id].players.append(weakref(player))
    
    # Send message to player about joining using localization
    var message = get_localized_text("player_joined_raid", [player.name])
    player.emit_signal("raid_joined", raid_id)
    
    return true

func leave_raid(raid_id: String, player: CharacterBody2D):
    """Allow player to leave a raid"""
    if !raid_instances.has(raid_id):
        return
    
    var players = raid_instances[raid_id].players
    for i in range(players.size()):
        var player_ref = players[i]
        var existing_player = player_ref.get_ref()
        if existing_player == player:
            players.remove_at(i)
            break

func check_raid_status():
    """Check status of all active raids"""
    var current_time = Time.get_ticks_msec()
    
    # Remove expired raids
    for raid_id in raid_instances.keys():
        var raid_info = raid_instances[raid_id]
        if raid_info.status == "active":
            var elapsed_time = current_time - raid_info.started_at
            
            # End raids that have been ongoing too long (5 minutes)
            if elapsed_time > 300000:  # 5 minutes
                end_raid(raid_id, false)

func push_notification(message: String, duration: float):
    """Push a notification to all players in raid"""
    for player_ref in players_in_raid:
        var player = player_ref.get_ref()
        if player != null and has_method("add_to_chat"):
            # Send message to player's chat system
            player.add_to_chat(message)

func get_active_raids() -> Array[String]:
    """Get list of active raid IDs"""
    return active_raids

func get_raid_info(raid_id: String) -> Dictionary:
    """Get information for a specific raid"""
    if raid_instances.has(raid_id):
        return raid_instances[raid_id]
    return {}

func boss_difficulty_to_reward_type(difficulty: int) -> String:
    """Convert boss difficulty to reward type"""
    if difficulty <= 2:
        return "normal"
    elif difficulty <= 4:
        return "elite"
    else:
        return "raid"

func is_raid_active() -> bool:
    """Check if any raids are currently active"""
    return active_raids.size() > 0

# Public API to communicate with raid bosses
func boss_defeated():
    """Called when current boss is defeated"""
    if current_boss_ref != null:
        var boss = current_boss_ref.get_ref()
        if boss != null:
            end_raid(boss.raid_instance_id, true)
            
func boss_victory():
    """Called when boss wins the raid"""
    if current_boss_ref != null:
        var boss = current_boss_ref.get_ref()
        if boss != null:
            end_raid(boss.raid_instance_id, false)

# Utility functions
func get_boss_info() -> Dictionary:
    """Get information about currently active boss"""
    if current_boss_ref != null:
        var boss = current_boss_ref.get_ref()
        if boss != null:
            return boss.get_boss_info()
    return {}

func update_active_raid_timer(delta: float):
    """Update raid timer (call from _process)"""
    active_raid_timer += delta

# Cutscene handling for boss encounters
func start_boss_cutscenes(boss: RaidBoss):
    """Start cutscenes for different types of bosses"""
    if !boss.is_instance_valid(boss):
        return
    
    # Boss type specific cutscenes  
    var cutscene_manager = get_node_or_null("/root/CutsceneManager")
    
    if cutscene_manager:
        # Regular cutscene for normal boss (one cutscene)
        # Raid cutscene - two cutscenes for raid bosses
        if boss.boss_type == "raid":
            cutscene_manager.play_cutscene("raid_boss_intro")
        else:
            cutscene_manager.play_cutscene("boss_intro")

# NPC Chat System Integration
func npc_receive_chat_message(player_name: String, message: String, npc_ref = null):
    """NPC receives and responds to chat messages"""
    
    # Basic response logic to prevent flooding
    if randf() < 0.3:  # 30% chance to respond
        var response = generate_npc_response(message)
        
        if response != "":
            # Send response to player's chat using localization
            print("[NPC] " + response)
            
            # Send to appropriate communication system
            if npc_ref != null:
                var npc = npc_ref.get_ref()
                if npc != null:
                    # Use a limit on how many responses can happen in quick succession 
                    pass  # Implement your specific messaging system here

func generate_npc_response(message: String) -> String:
    """Generate a context-aware response to player chat"""
    
    # Simple response logic based on message content
    var lower_message = message.to_lower()
    
    # Response logic with anti-flooding considerations
    if "hallo" in lower_message or "hello" in lower_message:
        return get_localized_text("npc_greeting")
    elif "danke" in lower_message or "thank" in lower_message:
        return get_localized_text("npc_thanks")
    elif "guten tag" in lower_message or "good day" in lower_message:
        return "Guten Tag, mein Freund!"
    elif "wo ist" in lower_message or "where is" in lower_message:
        return "Ich weiß nicht, wo das ist."
    elif "hilfe" in lower_message or "help" in lower_message:
        return get_localized_text("npc_question")
    else:
        # More generic responses to avoid chat flooding
        var random_responses = [
            "Das klingt interessant.",
            "Ich verstehe.",
            "Interessant...",
            "Das ist eine gute Frage.",
            "Vielen Dank für die Information."
        ]
        
        return random_responses[randi() % random_responses.size()]