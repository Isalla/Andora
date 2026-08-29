# Godot Character System

## Player Character Script (Character.gd)

```gdscript
extends CharacterBody2D

# Character attributes
var name = "Helen"
var race = "Mensch"  # Default race
var level = 1
var max_level = 40

# Stats
var health = 100
var max_health = 100
var mana = 50
var max_mana = 50
var strength = 10
var agility = 10
var intelligence = 10
var charisma = 10

# Experience
var experience = 0
var exp_to_level = 100

# Inventory
var inventory = {}
var equipped_items = {}

func _ready():
    # Initialize character with default values
    update_stats()

func update_stats():
    # Update stats based on level and race
    match race:
        "Mensch":
            strength += 5
            agility += 5
            intelligence += 5
        "Elf":
            intelligence += 10
            agility += 3
        "Andorer":
            strength += 8
            health += 20
        "Luzilla":
            agility += 10
            intelligence += 3
        "Mandalonier":
            strength += 12
            health += 15
    # Level-up calculations
    if level > max_level:
        level = max_level

func gain_experience(amount):
    experience += amount
    
    # Check for level up
    if experience >= exp_to_level:
        level_up()

func level_up():
    if level < max_level:
        level += 1
        experience -= exp_to_level
        exp_to_level = int(exp_to_level * 1.5)
        
        # Increase stats on level up
        max_health += 10
        health = max_health
        max_mana += 5
        mana = max_mana
        
        print(name + " reached level " + str(level))

func get_damage():
    return strength + equipped_items.get("weapon", {"damage": 0}).damage

func get_defense():
    return agility + equipped_items.get("armor", {"defense": 0}).defense

# Get character info for UI
func get_character_info():
    return {
        "name": name,
        "race": race,
        "level": level,
        "health": health,
        "max_health": max_health,
        "mana": mana,
        "max_mana": max_mana,
        "strength": strength,
        "agility": agility,
        "intelligence": intelligence,
        "experience": experience,
        "exp_to_level": exp_to_level
    }
```

## Race Selection System

```gdscript
# RaceSelection.gd
extends Node

var available_races = ["Mensch", "Elf", "Andorer", "Luzilla", "Mandalonier"]

func get_race_info(race_name):
    match race_name:
        "Mensch":
            return {
                "name": "Mensch",
                "description": "Ein allgemeiner Mensch mit ausgeglichener Entwicklung.",
                "stats": {
                    "strength": 10,
                    "agility": 10,
                    "intelligence": 10
                },
                "special_ability": "Gleichgewicht"
            }
        "Elf":
            return {
                "name": "Elf",
                "description": "Ein weiser Elf mit hoher Intelligenz.",
                "stats": {
                    "strength": 5,
                    "agility": 8,
                    "intelligence": 15
                },
                "special_ability": "Magisches Talent"
            }
        "Andorer":
            return {
                "name": "Andorer",
                "description": "Ein Starker Hundewesen mit großem Wachstumspotenzial.",
                "stats": {
                    "strength": 15,
                    "agility": 5,
                    "intelligence": 5
                },
                "special_ability": "Rauhpelz"
            }
        "Luzilla":
            return {
                "name": "Luzilla",
                "description": "Ein geschicktes Katzenwesen mit schnellen Reaktionen.",
                "stats": {
                    "strength": 5,
                    "agility": 15,
                    "intelligence": 8
                },
                "special_ability": "Katzenreflexe"
            }
        "Mandalonier":
            return {
                "name": "Mandalonier",
                "description": "Kräftig gebaute Wesen mit hohem Wachstumspotenzial.",
                "stats": {
                    "strength": 12,
                    "agility": 5,
                    "intelligence": 8
                },
                "special_ability": "Großes Wachstumspotenzial"
            }
        _:
            return null
```