# Godot Building System

## House Building Script (HouseBuilder.gd)

```gdscript
extends Node

# Available house designs
var house_templates = {
    "simple_house": {
        "name": "Einfaches Haus",
        "cost": 100,
        "size": {"width": 5, "height": 4},
        "materials_needed": {
            "wood": 20,
            "stone": 10,
            "iron": 5
        },
        "description": "Ein einfaches Holzhaus"
    },
    "castle": {
        "name": "Burg",
        "cost": 1000,
        "size": {"width": 10, "height": 8},
        "materials_needed": {
            "stone": 50,
            "iron": 30,
            "wood": 20
        },
        "description": "Eine stabile Burg"
    },
    "cottage": {
        "name": "Hütte",
        "cost": 200,
        "size": {"width": 4, "height": 3},
        "materials_needed": {
            "wood": 15,
            "stone": 5
        },
        "description": "Eine kleine Hütte"
    }
}

# Player's owned houses
var player_houses = {}

func _ready():
    pass

func can_afford_house(player_inventory, house_template):
    """Check if player has enough materials"""
    for material, needed_amount in house_template.materials_needed:
        if not player_inventory.has(material) or player_inventory[material] < needed_amount:
            return false
    return true

func build_house(player_id, house_type, position):
    """Build a house at given position"""
    if not house_templates.has(house_type):
        print("Invalid house type")
        return false
    
    var template = house_templates[house_type]
    
    # Check if player can afford it
    if not can_afford_house(get_player_inventory(player_id), template):
        print("Not enough materials to build house")
        return false
    
    # Deduct materials
    deduct_materials(player_id, template.materials_needed)
    
    # Create the house
    var house = {
        "id": generate_house_id(),
        "type": house_type,
        "position": position,
        "size": template.size,
        "owner": player_id,
        "name": template.name,
        "built_at": Time.get_ticks_msec()
    }
    
    player_houses[house["id"]] = house
    print("House built successfully: " + house["name"])
    return true

func generate_house_id():
    """Generate a unique ID for house"""
    return str(Time.get_ticks_msec()) + "_" + str(randi() % 1000)

func get_player_inventory(player_id):
    """Get player's inventory (to be implemented based on your inventory system)"""
    # This would connect to your character or inventory system
    return {}

func deduct_materials(player_id, materials_needed):
    """Deduct materials from player's inventory (to be implemented)"""
    # This would update the player's inventory
    pass

func get_owned_houses(player_id):
    """Get all houses owned by a player"""
    var owned = []
    for house_id in player_houses:
        if player_houses[house_id]["owner"] == player_id:
            owned.append(player_houses[house_id])
    return owned

func get_house_info(house_id):
    """Get detailed information about a specific house"""
    if player_houses.has(house_id):
        return player_houses[house_id]
    return null

func upgrade_house(house_id, upgrade_type):
    """Upgrade a house with new features"""
    if not player_houses.has(house_id):
        return false
    
    # This would be expanded based on upgrade system
    var house = player_houses[house_id]
    print("Upgrading house " + house["name"])
    
    return true
```

## Building Manager Script (BuildingManager.gd)

```gdscript
extends Node

# Building system for MMORPG
var building_mode = false
var selected_building_type = ""
var build_preview = null
var current_build_position = Vector2.ZERO

# Available building tools
var building_tools = {
    "house": "Hausbau",
    "farm": "Farm",
    "workshop": "Werkstatt"
}

func _ready():
    pass

func enter_building_mode(building_type):
    """Enter building mode with specified building type"""
    building_mode = true
    selected_building_type = building_type
    print("Building mode activated for: " + building_type)

func exit_building_mode():
    """Exit building mode"""
    building_mode = false
    selected_building_type = ""
    print("Building mode deactivated")

func set_build_position(position):
    """Set the position where building will be placed"""
    current_build_position = position

func can_place_building(position, build_type):
    """Check if building can be placed at given position"""
    # Check if position is valid
    if position.x < 0 or position.y < 0:
        return false
    
    # Check for existing buildings in area
    if check_overlapping_buildings(position, build_type):
        return false
    
    # More validation checks can go here
    return true

func check_overlapping_buildings(position, build_type):
    """Check if new building would overlap with existing ones"""
    # This would check against all existing buildings
    return false

func get_building_preview(build_type):
    """Get preview for building placement"""
    # Return visual representation of where building will be placed
    return null
    
func complete_build():
    """Complete the building process"""
    if building_mode and selected_building_type:
        print("Building completed at position: " + str(current_build_position))
        exit_building_mode()
        return true
    return false

func get_available_houses():
    """Get list of all available house designs"""
    return HouseBuilder.house_templates
```

## Sample Building Areas (building_areas.gd)

```gdscript
extends Node

# Define different buildable areas in the world
var buildable_areas = {
    "town_center": {
        "name": "Stadtzentrum",
        "description": "Großes Gebiet für Hauptgebäude",
        "size": {"width": 50, "height": 50},
        "requirements": {"level": 10}
    },
    "residential": {
        "name": "Wohngebiet",
        "description": "Area für private Häuser",
        "size": {"width": 20, "height": 20},
        "requirements": {"level": 5}
    },
    "industrial": {
        "name": "Industriegebiet",
        "description": "Area für Werkhallen und Fabriken",
        "size": {"width": 30, "height": 30},
        "requirements": {"level": 15}
    }
}

func get_area_info(area_id):
    """Get information about a specific building area"""
    if buildable_areas.has(area_id):
        return buildable_areas[area_id]
    return null

func can_build_in_area(player_level, area_id):
    """Check if player can build in an area based on requirements"""
    if buildable_areas.has(area_id):
        var area = buildable_areas[area_id]
        if area.requirements.level <= player_level:
            return true
    return false
```