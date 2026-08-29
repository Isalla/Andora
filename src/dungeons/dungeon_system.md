# Godot Dungeon and Battle System

## Dungeon Script (Dungeon.gd)

```gdscript
extends Node

# Dungeon structure
var dungeon_id = ""
var name = "Unbekannter Dungeon"
var level_requirement = 1
var difficulty = "normal"  # easy, normal, hard, extreme
var size = {"width": 100, "height": 100}
var theme = "undead"  # forest, cave, castle, undead, dragon

# Rooms in the dungeon
var rooms = []
var connections = []

# Enemies in the dungeon
var enemy_spawns = []
var boss_spawn = null

# Rewards for completing dungeon
var rewards = {
    "experience": 0,
    "gold": 0,
    "items": []
}

# Dungeon status
var is_active = false
var is_completed = false
var current_room = 0
var visited_rooms = []

func _ready():
    # Initialize basic dungeon structure
    setup_dungeon()

func setup_dungeon():
    """Setup the dungeon with rooms and connections"""
    generate_rooms()
    connect_rooms()
    setup_enemies()
    setup_rewards()

func generate_rooms():
    """Generate random rooms for the dungeon"""
    # Create starting room
    var start_room = {
        "id": "room_start_001",
        "type": "start",
        "position": {"x": 50, "y": 50},
        "size": {"width": 20, "height": 20}
    }
    rooms.append(start_room)
    
    # Create some regular rooms
    for i in range(5):
        var room = {
            "id": "room_" + str(i),
            "type": "regular",
            "position": {"x": randf_range(10, 90), "y": randf_range(10, 90)},
            "size": {"width": 15, "height": 15}
        }
        rooms.append(room)
    
    # Create ending room
    var end_room = {
        "id": "room_end_001",
        "type": "end",
        "position": {"x": 80, "y": 80},
        "size": {"width": 20, "height": 20}
    }
    rooms.append(end_room)

func connect_rooms():
    """Connect rooms with corridors"""
    # Simple connection between first and last room
    connections.append({
        "from": "room_start_001",
        "to": "room_0"
    })
    
    connections.append({
        "from": "room_0",
        "to": "room_1"
    })
    
    connections.append({
        "from": "room_4",
        "to": "room_end_001"
    })

func setup_enemies():
    """Setup enemy spawns in the dungeon"""
    # Spawn points for different enemy types
    enemy_spawns = [
        {"room": "room_0", "type": "slime", "count": 3, "level": 1},
        {"room": "room_1", "type": "orc", "count": 2, "level": 3},
        {"room": "room_2", "type": "skeleton", "count": 5, "level": 2},
        {"room": "room_end_001", "type": "boss", "count": 1, "level": 10}
    ]

func setup_rewards():
    """Setup rewards for completing dungeon"""
    rewards = {
        "experience": 200,
        "gold": 100,
        "items": ["health_potion", "wooden_sword"]
    }

func enter_dungeon():
    """Player enters the dungeon"""
    is_active = true
    print("Entered dungeon: " + name)
    
    # Set player to starting room
    current_room = 0
    visited_rooms.append(0)

func exit_dungeon():
    """Player exits the dungeon"""
    is_active = false
    print("Exited dungeon: " + name)

func complete_dungeon(player):
    """Complete the dungeon and give rewards"""
    if not is_completed:
        is_completed = true
        player.give_experience(rewards.experience)
        player.add_gold(rewards.gold)
        
        # Give items
        for item in rewards.items:
            player.inventory.add_item(item)
            
        print("Dungeon completed! Rewards: " + str(rewards))

func get_current_room():
    """Get the room the player is currently in"""
    if rooms.size() > current_room:
        return rooms[current_room]
    return null

func get_room(room_id):
    """Get info about a specific room"""
    for room in rooms:
        if room.id == room_id:
            return room
    return null

func move_to_room(room_index):
    """Move player to a different room"""
    if room_index < rooms.size() and room_index >= 0:
        current_room = room_index
        if not visited_rooms.has(room_index):
            visited_rooms.append(room_index)
        print("Moved to room: " + str(room_index))
        return true
    return false

func get_dungeon_info():
    """Get dungeon information for UI"""
    return {
        "id": dungeon_id,
        "name": name,
        "level_requirement": level_requirement,
        "difficulty": difficulty,
        "rooms_count": rooms.size(),
        "active": is_active,
        "completed": is_completed
    }
```

## Battle System Script (BattleSystem.gd)

```gdscript
extends Node

# Battle state
var is_battle_active = false
var battle_type = "normal"  # normal, dungeon, boss
var current_enemy = null
var player_team = []
var enemy_team = []

# Battle settings
var turn_speed = 1.0  # seconds per turn
var current_turn = 0
var total_turns = 0

func _ready():
    pass

func start_battle(enemy_type, player_stats):
    """Start a new battle"""
    is_battle_active = true
    
    # Create enemy based on type
    var enemy = create_enemy(enemy_type, player_stats.level)
    current_enemy = enemy
    
    print("Battle started against: " + enemy.name)
    
    # Setup battle teams
    player_team = [player_stats]  # Simplified - would be actual party
    enemy_team = [enemy]
    
    # Start first turn
    process_turn()

func create_enemy(enemy_type, level):
    """Create an enemy with appropriate stats"""
    var enemy_info = {
        "name": "Monstrosität",
        "type": enemy_type,
        "level": level,
        "health": 50 + (level * 10),
        "max_health": 50 + (level * 10),
        "strength": 5 + (level * 2),
        "agility": 3 + level,
        "intelligence": 2 + level
    }
    
    # Adjust stats based on enemy type
    match enemy_type:
        "slime":
            enemy_info.name = "Schleim"
            enemy_info.health = 30 + (level * 5)
            enemy_info.strength = 3 + (level * 1)
        "orc":
            enemy_info.name = "Ork"
            enemy_info.health = 80 + (level * 15)
            enemy_info.strength = 8 + (level * 3)
        "skeleton":
            enemy_info.name = "Skelett"
            enemy_info.health = 60 + (level * 12)
            enemy_info.agility = 5 + level
        "boss":
            enemy_info.name = "Boss"
            enemy_info.health = 200 + (level * 30)
            enemy_info.strength = 15 + (level * 4)
            
    return enemy_info

func process_turn():
    """Process one turn of battle"""
    if not is_battle_active or !player_team or !enemy_team:
        return
    
    # Simple AI for enemy
    if current_turn % 2 == 0:  # Player turn
        print("Player's Turn")
    else:  # Enemy turn
        if current_enemy and current_enemy.health > 0:
            print(current_enemy.name + " attacks!")
            # Player takes damage (simplified)
            player_team[0].health -= current_enemy.strength
    
    total_turns += 1
    current_turn += 1

func player_attack():
    """Player performs an attack"""
    if is_battle_active and current_enemy and current_enemy.health > 0:
        # Basic attack
        var damage = randi_range(5, 15) + (player_team[0].strength / 2)
        current_enemy.health -= damage
        
        print("Player attacks for " + str(damage) + " damage!")
        
        # Check if enemy is defeated
        if current_enemy.health <= 0:
            end_battle(true)
            return

func player_use_item(item_name):
    """Player uses an item during battle"""
    if is_battle_active and current_enemy:
        match item_name:
            "health_potion":
                player_team[0].health += 30
                print("Used health potion")
            _:
                print("Unknown item used")

func end_battle(player_won):
    """End the battle"""
    is_battle_active = false
    
    if player_won:
        print("Player won the battle!")
        # Give rewards
    else:
        print("Player lost the battle...")
        # Handle defeat

func get_battle_info():
    """Get battle information for UI"""
    return {
        "active": is_battle_active,
        "turn": current_turn,
        "player_health": player_team[0].health if player_team.size() > 0 else 0,
        "enemy_health": current_enemy.health if current_enemy else 0,
        "enemy_name": current_enemy.name if current_enemy else ""
    }
```

## Dungeon Manager Script (DungeonManager.gd)

```gdscript
extends Node

# List of available dungeons
var dungeons = {}
var active_dungeon = null
var player_in_dungeon = false

func _ready():
    # Load sample dungeons
    load_sample_dungeons()

func load_sample_dungeons():
    # Sample dungeon
    var forest_dungeon = Dungeon.new()
    forest_dungeon.dungeon_id = "forest_001"
    forest_dungeon.name = "Dschungelhöhle"
    forest_dungeon.level_requirement = 5
    forest_dungeon.difficulty = "normal"
    forest_dungeon.theme = "forest"
    
    dungeons[forest_dungeon.dungeon_id] = forest_dungeon
    
    # Another sample dungeon
    var cave_dungeon = Dungeon.new()
    cave_dungeon.dungeon_id = "cave_001"
    cave_dungeon.name = "Alte Höhle"
    cave_dungeon.level_requirement = 10
    cave_dungeon.difficulty = "hard"
    cave_dungeon.theme = "cave"
    
    dungeons[cave_dungeon.dungeon_id] = cave_dungeon

func enter_dungeon(dungeon_id, player):
    """Player enters a dungeon"""
    if not dungeons.has(dungeon_id):
        return false
    
    var dungeon = dungeons[dungeon_id]
    
    # Check requirements
    if player.level < dungeon.level_requirement:
        print("You're not strong enough to enter this dungeon!")
        return false
    
    active_dungeon = dungeon
    active_dungeon.enter_dungeon()
    player_in_dungeon = true
    
    print("Entering " + dungeon.name)
    return true

func exit_dungeon():
    """Player exits the current dungeon"""
    if active_dungeon:
        active_dungeon.exit_dungeon()
        active_dungeon = null
        player_in_dungeon = false
        print("Exited dungeon")

func complete_current_dungeon(player):
    """Complete the current dungeon"""
    if active_dungeon and player_in_dungeon:
        active_dungeon.complete_dungeon(player)
        exit_dungeon()
        return true
    return false

func get_available_dungeons(player_level):
    """Get list of dungeons player can enter based on their level"""
    var available = []
    for dungeon_id in dungeons:
        var dungeon = dungeons[dungeon_id]
        if player_level >= dungeon.level_requirement:
            available.append(dungeon)
    return available

func get_dungeon_info(dungeon_id):
    """Get detailed information about a dungeon"""
    if dungeons.has(dungeon_id):
        return dungeons[dungeon_id].get_dungeon_info()
    return null

func spawn_boss_enemy(boss_type, position):
    """Spawn a boss enemy in battle"""
    var boss = BattleSystem.create_enemy(boss_type, 10)  # Level 10 boss
    print("Boss spawned: " + boss.name)
    return boss
```

## Battle Manager Script (BattleManager.gd)

```gdscript
extends Node

# Global battle management
var current_battle = null
var battle_log = []
var battle_settings = {
    "turn_speed": 1.0,
    "allow_flee": true,
    "battle_type": "normal"
}

func _ready():
    pass

func start_random_encounter(player_stats):
    """Start a random enemy encounter"""
    # Simplified - in reality this would determine what kind of encounter
    var enemy_types = ["slime", "orc", "skeleton"]
    var enemy_type = enemy_types[randi() % enemy_types.size()]
    
    BattleSystem.start_battle(enemy_type, player_stats)

func start_dungeon_battle(dungeon_id):
    """Start battle in a dungeon context"""
    # This would be tied to specific rooms or events
    var enemy_type = "orc"  # Simplified
    
    BattleSystem.start_battle(enemy_type, null)
    
    print("Started battle in dungeon: " + dungeon_id)

func process_battle_update(delta):
    """Update ongoing battle"""
    if BattleSystem.is_battle_active:
        # This would be where we process turn timing
        pass

func get_battle_status():
    """Get current battle status"""
    return BattleSystem.get_battle_info()

func log_battle_event(event_text):
    """Log an event in the battle"""
    battle_log.append({
        "timestamp": Time.get_ticks_msec(),
        "event": event_text
    })
```

## Sample Dungeon Data File (dungeons/sample_dungeons.json)

```json
{
  "dungeons": [
    {
      "id": "forest_001",
      "name": "Dschungelhöhle",
      "level_requirement": 5,
      "difficulty": "normal",
      "theme": "forest",
      "size": {"width": 100, "height": 100},
      "rooms": 7,
      "enemies": [
        {"type": "slime", "count": 3},
        {"type": "orc", "count": 2}
      ],
      "rewards": {
        "experience": 200,
        "gold": 100,
        "items": ["health_potion"]
      }
    },
    {
      "id": "cave_001",
      "name": "Alte Höhle",
      "level_requirement": 10,
      "difficulty": "hard",
      "theme": "cave",
      "size": {"width": 150, "height": 150},
      "rooms": 12,
      "enemies": [
        {"type": "skeleton", "count": 5},
        {"type": "orc", "count": 3}
      ],
      "rewards": {
        "experience": 500,
        "gold": 250,
        "items": ["iron_sword", "health_potion"]
      }
    }
  ]
}
```