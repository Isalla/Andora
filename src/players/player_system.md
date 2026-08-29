# Godot Player and Character Management System

## Player Character Script (PlayerCharacter.gd)

```gdscript
extends CharacterBody2D

# Basic character properties
var player_id = ""
var username = "Anonymous"
var character_name = "Hero"
var level = 1
var experience = 0
var gold = 0

# Stats
var health = 100
var max_health = 100
var mana = 50
var max_mana = 50
var strength = 10
var agility = 10
var intelligence = 10
var constitution = 10

# Skills and abilities
var skills = {
    "crafting": 1,
    "alchemy": 1,
    "swordsmanship": 1,
    "archery": 1,
    "magic": 1
}

# Inventory
var inventory = {}
var equipped_items = {
    "weapon": null,
    "armor": null,
    "accessory": null
}

# Location data
var position = Vector2.ZERO
var world_id = ""
var zone = ""

# Status effects
var status_effects = []

# Character state
var is_alive = true
var is_moving = false
var is_attacking = false
var is_in_combat = false

func _ready():
    # Initialize character stats based on level
    update_stats()

func update_stats():
    """Update character stats based on level and attributes"""
    max_health = 100 + (constitution * 5)
    health = max_health
    
    max_mana = 50 + (intelligence * 3)
    mana = max_mana

func move_to_position(target_position):
    """Move character to target position"""
    position = target_position
    is_moving = true
    
    # In a real game, this would animate movement
    print("Moving to: " + str(position))

func take_damage(amount):
    """Take damage from attacks"""
    health -= amount
    if health <= 0:
        health = 0
        die()
    
    return health

func heal(amount):
    """Heal character"""
    health += amount
    if health > max_health:
        health = max_health
    
    return health

func gain_experience(exp_amount):
    """Gain experience points"""
    print("Gained " + str(exp_amount) + " experience")
    experience += exp_amount
    
    # Check for level up
    if experience >= level * 100:
        level_up()

func level_up():
    """Level up the character"""
    level += 1
    print("Levelled up to " + str(level))
    
    # Increase stats
    strength += 2
    agility += 1
    intelligence += 1
    constitution += 2
    
    update_stats()

func add_gold(amount):
    """Add gold to player's inventory"""
    gold += amount
    print("Gold increased to: " + str(gold))

func give_experience(amount):
    """Give experience to the character"""
    gain_experience(amount)

func die():
    """Handle character death"""
    is_alive = false
    print(character_name + " has died")
    
    # Respawn logic would go here

func equip_item(item_id, slot):
    """Equip an item to a specific slot"""
    if inventory.has(item_id) and inventory[item_id].count > 0:
        if equipped_items[slot] != null:
            # Unequip current item
            unequip_item(slot)
        
        equipped_items[slot] = item_id
        inventory[item_id].count -= 1
        
        print("Equipped: " + item_id + " in slot: " + slot)

func unequip_item(slot):
    """Unequip an item from a slot"""
    if equipped_items[slot] != null:
        var item_id = equipped_items[slot]
        equipped_items[slot] = null
        add_to_inventory(item_id, 1)
        print("Unequipped: " + item_id)

func add_to_inventory(item_id, count):
    """Add items to inventory"""
    if !inventory.has(item_id):
        inventory[item_id] = {"count": 0}
    
    inventory[item_id].count += count
    print("Added " + str(count) + " of " + item_id + " to inventory")

func remove_from_inventory(item_id, count):
    """Remove items from inventory"""
    if inventory.has(item_id):
        if inventory[item_id].count >= count:
            inventory[item_id].count -= count
            if inventory[item_id].count <= 0:
                inventory.erase(item_id)
            return true
    return false

func get_character_info():
    """Get character information for UI display"""
    return {
        "id": player_id,
        "username": username,
        "character_name": character_name,
        "level": level,
        "health": health,
        "max_health": max_health,
        "mana": mana,
        "max_mana": max_mana,
        "experience": experience,
        "gold": gold,
        "skills": skills,
        "inventory_size": inventory.size(),
        "equipped_items": equipped_items,
        "position": position,
        "alive": is_alive
    }
```

## Player Manager Script (PlayerManager.gd)

```gdscript
extends Node

# List of all players in the game
var players = {}
var online_players = []
var player_data = {}

func _ready():
    # Initialize base data for players
    load_player_templates()

func load_player_templates():
    """Load base player templates"""
    # Sample player data for creating new characters
    player_data = {
        "warrior": {
            "stats": {
                "strength": 20,
                "agility": 10,
                "intelligence": 5,
                "constitution": 15
            },
            "skills": [
                {"name": "swordsmanship", "level": 1},
                {"name": "shield_bash", "level": 1}
            ],
            "starting_items": ["wooden_sword", "leather_armor"]
        },
        "mage": {
            "stats": {
                "strength": 5,
                "agility": 10,
                "intelligence": 20,
                "constitution": 10
            },
            "skills": [
                {"name": "fireball", "level": 1},
                {"name": "magic_resistance", "level": 1}
            ],
            "starting_items": ["wooden_staff", "robe"]
        },
        "archer": {
            "stats": {
                "strength": 8,
                "agility": 20,
                "intelligence": 10,
                "constitution": 12
            },
            "skills": [
                {"name": "aim", "level": 1},
                {"name": "poison_arrow", "level": 1}
            ],
            "starting_items": ["wooden_bow", "leather_armor"]
        }
    }

func create_player(player_id, username, character_name, character_type):
    """Create a new player character"""
    var player = PlayerCharacter.new()
    player.player_id = player_id
    player.username = username
    player.character_name = character_name
    
    # Apply character type template
    apply_character_template(player, character_type)
    
    players[player_id] = player
    online_players.append(player_id)
    
    print("Created new player: " + player.name)
    return player

func apply_character_template(player, char_type):
    """Apply a character type template to the new player"""
    if player_data.has(char_type):
        var template = player_data[char_type]
        player.strength = template.stats.strength
        player.agility = template.stats.agility
        player.intelligence = template.stats.intelligence
        player.constitution = template.stats.constitution
        
        # Give starting items
        for item in template.starting_items:
            player.add_to_inventory(item, 1)
        
        update_stats(player)
    else:
        # Default stats if template doesn't exist
        player.strength = 10
        player.agility = 10
        player.intelligence = 10
        player.constitution = 10
        update_stats(player)

func update_stats(player):
    """Update a character's stats based on attributes"""
    player.update_stats()

func get_player(player_id):
    """Get specific player by ID"""
    if players.has(player_id):
        return players[player_id]
    return null

func add_player_to_online(player_id):
    """Add player to online list"""
    if !online_players.has(player_id):
        online_players.append(player_id)
        print("Player " + player_id + " is now online")

func remove_player_from_online(player_id):
    """Remove player from online list"""
    if online_players.has(player_id):
        online_players.erase(player_id)
        print("Player " + player_id + " is now offline")

func save_player_data(player_id):
    """Save player data to database"""
    var player = get_player(player_id)
    if player:
        # In a real implementation, this would write to a database
        print("Saved player data for: " + player.username)
        
        # This is where you'd typically serialize the player object
        return {
            "player_id": player.player_id,
            "username": player.username,
            "character_name": player.character_name,
            "level": player.level,
            "experience": player.experience,
            "gold": player.gold,
            "health": player.health,
            "inventory": player.inventory,
            "skills": player.skills
        }

func load_player_data(player_id):
    """Load player data from database"""
    # This would retrieve saved character data
    print("Loaded player data for: " + player_id)
    
    # In a real implementation, this would fetch from database
    return null

func get_online_players():
    """Get list of currently online players"""
    var online = []
    for player_id in online_players:
        if players.has(player_id):
            online.append(players[player_id])
    return online

func get_player_stats(player_id):
    """Get stats of a specific player"""
    var player = get_player(player_id)
    if player:
        return player.get_character_info()
    return null
```

## Character Customization System (CharacterCustomization.gd)

```gdscript
extends Node

# Character customization data
var character_templates = {
    "race": [
        {"name": "Human", "stats": {"strength": 10, "agility": 10, "intelligence": 10}},
        {"name": "Elf", "stats": {"strength": 8, "agility": 12, "intelligence": 15}},
        {"name": "Dwarf", "stats": {"strength": 15, "agility": 8, "intelligence": 10}}
    ],
    
    "class": [
        {"name": "Warrior", "skills": ["swordsmanship", "shield_bash"]},
        {"name": "Mage", "skills": ["fireball", "magic_resistance"]},
        {"name": "Archer", "skills": ["aim", "poison_arrow"]}
    ],
    
    "appearance": {
        "hair_styles": ["short", "long", "bald", "braided"],
        "hair_colors": ["black", "brown", "blonde", "red"],
        "skin_colors": ["light", "medium", "dark"],
        "face_shapes": ["round", "oval", "square"]
    }
}

func customize_character(player_id, customization_data):
    """Apply character customizations"""
    var player = PlayerManager.get_player(player_id)
    
    if player:
        # Apply stats based on race
        if customization_data.race:
            apply_race_stats(player, customization_data.race)
        
        # Apply class skills
        if customization_data.class:
            apply_class_skills(player, customization_data.class)
        
        # Apply appearance settings
        player.appearance = customization_data.appearance
        
        print("Character customized: " + player.character_name)
        return true
    
    return false

func apply_race_stats(player, race_name):
    """Apply race-based stat bonuses"""
    for race in character_templates.race:
        if race.name == race_name:
            player.strength += race.stats.strength
            player.agility += race.stats.agility
            player.intelligence += race.stats.intelligence
            break

func apply_class_skills(player, class_name):
    """Apply class-based skills"""
    for char_class in character_templates.class:
        if char_class.name == class_name:
            for skill in char_class.skills:
                if !player.skills.has(skill):
                    player.skills[skill] = 1
            break
```

## Game Stats and Progression (GameProgression.gd)

```gdscript
extends Node

# System for tracking game progression
var achievements = []
var quests_completed = []
var items_collected = []
var zones_explored = []

func _ready():
    # Initialize progression system
    load_achievement_templates()

func load_achievement_templates():
    """Load available achievements"""
    var achievement_templates = [
        {"id": "first_kill", "name": "Erster Tod", "description": "Töte deinen ersten Gegner"},
        {"id": "level_10", "name": "Level 10", "description": "Erreiche Level 10"},
        {"id": "collect_100_items", "name": "Sammler", "description": "Sammle 100 Gegenstände"},
        {"id": "explore_5_zones", "name": "Wanderer", "description": "Erkunde 5 Zonen"}
    ]
    
    achievements = achievement_templates

func gain_achievement(achievement_id):
    """Grant player an achievement"""
    if !quests_completed.has(achievement_id):
        quests_completed.append(achievement_id)
        print("Achievement unlocked: " + achievement_id)
        return true
    
    return false

func complete_quest(quest_id, player):
    """Complete a quest for a player"""
    # In a real implementation:
    # 1. Check if quest can be completed
    # 2. Give rewards
    # 3. Update progress
    print("Quest completed: " + quest_id)
    
    # Award XP and gold
    var rewards = get_quest_rewards(quest_id)
    player.give_experience(rewards.experience)
    player.add_gold(rewards.gold)

func get_quest_rewards(quest_id):
    """Get rewards for completing a specific quest"""
    # Simplified - in a real game, these would be defined per quest
    var reward_map = {
        "intro_001": {"experience": 100, "gold": 50},
        "monster_001": {"experience": 200, "gold": 100}
    }
    
    if reward_map.has(quest_id):
        return reward_map[quest_id]
    else:
        return {"experience": 0, "gold": 0}

func update_zone_exploration(player, zone_id):
    """Update zone exploration tracking"""
    if !zones_explored.has(zone_id):
        zones_explored.append(zone_id)
        player.give_experience(10)  # Small XP reward
        print("New zone explored: " + zone_id)

func get_player_progression_info(player_id):
    """Get progression info for a player"""
    var player = PlayerManager.get_player(player_id)
    
    return {
        "total_quests": quests_completed.size(),
        "completed_quests": quests_completed,
        "achievements": achievements.size(),
        "zones_explored": zones_explored.size(),
        "level_progress": {
            "current": player.level,
            "experience": player.experience,
            "next_level": player.level * 100
        }
    }
```

## Player Stats Data File (players/stats.json)

```json
{
  "races": [
    {
      "name": "Human",
      "stats": {
        "strength": 10,
        "agility": 10,
        "intelligence": 10,
        "constitution": 10
      },
      "description": "Ein ausgewogenener Rasse mit keiner besonderen Stärke"
    },
    {
      "name": "Elf",
      "stats": {
        "strength": 8,
        "agility": 12,
        "intelligence": 15,
        "constitution": 8
      },
      "description": "Steife und schnelle Schattenkämpfer mit magischen Fähigkeiten"
    },
    {
      "name": "Dwarf",
      "stats": {
        "strength": 15,
        "agility": 8,
        "intelligence": 10,
        "constitution": 12
      },
      "description": "Robuste Krieger mit hoher Widerstandsfähigkeit"
    }
  ],
  
  "classes": [
    {
      "name": "Warrior",
      "skills": ["Schildstoß", "Kampftraining"],
      "primary_attribute": "strength",
      "role": "tank",
      "description": "Ein starker Kämpfer, der sich auf Nahkampf konzentriert"
    },
    {
      "name": "Mage",
      "skills": ["Feuerball", "Magische Resistenz"],
      "primary_attribute": "intelligence",
      "role": "caster",
      "description": "Ein Zauberer mit starken Magiefähigkeiten"
    },
    {
      "name": "Archer",
      "skills": ["Zielgenauigkeit", "Giftspitze"],
      "primary_attribute": "agility",
      "role": "ranged",
      "description": "Ein präziser Schütze mit Fernkampfkenntnissen"
    }
  ],
  
  "achievements": [
    {
      "id": "first_kill",
      "name": "Erster Tod",
      "description": "Töte deinen ersten Gegner",
      "points": 10
    },
    {
      "id": "level_10",
      "name": "Level 10",
      "description": "Erreiche Level 10",
      "points": 50
    },
    {
      "id": "collect_100_items",
      "name": "Sammler",
      "description": "Sammle 100 Gegenstände",
      "points": 100
    }
  ]
}
```