# Godot Crafting System

## Crafting Recipe Script (CraftingRecipe.gd)

```gdscript
extends Resource

# Crafting recipe structure
@export var recipe_id = ""
@export var name = ""
@export var description = ""
@export var required_level = 1
@export var crafting_time = 1.0 # in seconds
@export var materials_required = {}
@export var result_item = ""
@export var quantity_result = 1
@export var skills_required = {}

func _ready():
    pass

# Check if player can craft this recipe
func can_craft(player_stats, player_inventory):
    # Check level requirement
    if player_stats.level < required_level:
        return false
    
    # Check if player has all materials
    for material, amount in materials_required:
        if not player_inventory.has(material) or player_inventory[material] < amount:
            return false
    
    # Check skill requirements
    for skill, level in skills_required:
        if not player_stats.skills.has(skill) or player_stats.skills[skill] < level:
            return false
    
    return true

# Consume materials and create item
func craft(player_inventory):
    # Deduct materials from inventory
    for material, amount in materials_required:
        player_inventory[material] -= amount
    
    # Return crafted item info
    return {
        "item": result_item,
        "quantity": quantity_result
    }

# Get recipe information for UI
func get_recipe_info():
    return {
        "id": recipe_id,
        "name": name,
        "description": description,
        "required_level": required_level,
        "materials_required": materials_required,
        "result_item": result_item,
        "quantity_result": quantity_result
    }
```

## Crafting Manager Script (CraftingManager.gd)

```gdscript
extends Node

# List of all available recipes
var recipes = {}
var active_crafting = null

func _ready():
    # Load sample recipes
    load_sample_recipes()

func load_sample_recipes():
    # Sample recipe: Wooden Sword
    var wooden_sword_recipe = CraftingRecipe.new()
    wooden_sword_recipe.recipe_id = "sword_wood_001"
    wooden_sword_recipe.name = "Holzschwert"
    wooden_sword_recipe.description = "Ein einfaches Holzschwert"
    wooden_sword_recipe.required_level = 5
    wooden_sword_recipe.crafting_time = 2.0
    wooden_sword_recipe.materials_required = {
        "wood": 10,
        "iron": 2
    }
    wooden_sword_recipe.result_item = "wooden_sword"
    wooden_sword_recipe.quantity_result = 1
    wooden_sword_recipe.skills_required = {
        "crafting": 3
    }
    
    recipes[wooden_sword_recipe.recipe_id] = wooden_sword_recipe
    
    # Sample recipe: Wooden Shield
    var wooden_shield_recipe = CraftingRecipe.new()
    wooden_shield_recipe.recipe_id = "shield_wood_001"
    wooden_shield_recipe.name = "Holzschild"
    wooden_shield_recipe.description = "Ein einfacher Holzschild"
    wooden_shield_recipe.required_level = 3
    wooden_shield_recipe.crafting_time = 3.0
    wooden_shield_recipe.materials_required = {
        "wood": 15,
        "iron": 1
    }
    wooden_shield_recipe.result_item = "wooden_shield"
    wooden_shield_recipe.quantity_result = 1
    wooden_shield_recipe.skills_required = {
        "crafting": 2
    }
    
    recipes[wooden_shield_recipe.recipe_id] = wooden_shield_recipe
    
    # Sample recipe: Health Potion
    var health_potion_recipe = CraftingRecipe.new()
    health_potion_recipe.recipe_id = "potion_health_001"
    health_potion_recipe.name = "Gesundheitstrank"
    health_potion_recipe.description = "Ein Trank zur Regeneration von Gesundheit"
    health_potion_recipe.required_level = 1
    health_potion_recipe.crafting_time = 1.0
    health_potion_recipe.materials_required = {
        "herb": 3,
        "water": 1
    }
    health_potion_recipe.result_item = "health_potion"
    health_potion_recipe.quantity_result = 3
    health_potion_recipe.skills_required = {
        "alchemy": 1
    }
    
    recipes[health_potion_recipe.recipe_id] = health_potion_recipe

func start_crafting(recipe_id, player_stats, player_inventory):
    """Start crafting process"""
    if not recipes.has(recipe_id):
        return false
    
    var recipe = recipes[recipe_id]
    
    # Check if player can craft
    if not recipe.can_craft(player_stats, player_inventory):
        return false
    
    # Start crafting animation and time
    active_crafting = {
        "recipe": recipe,
        "progress": 0.0,
        "duration": recipe.crafting_time
    }
    
    print("Crafting started: " + recipe.name)
    return true

func update_crafting(delta):
    """Update crafting progress"""
    if active_crafting != null:
        active_crafting.progress += delta
        
        if active_crafting.progress >= active_crafting.duration:
            complete_crafting()
            return true
    
    return false

func complete_crafting():
    """Complete the crafting process"""
    if active_crafting != null:
        var recipe = active_crafting.recipe
        print("Crafting completed: " + recipe.name)
        
        # Actually craft the item (this would update inventory in real implementation)
        var result = recipe.craft(player_inventory)
        active_crafting = null
        
        return result
    
    return null

func get_available_recipes():
    """Get all available recipes for crafting"""
    var available = []
    for recipe_id in recipes:
        available.append(recipes[recipe_id])
    return available

func get_recipe_info(recipe_id):
    """Get detailed information about a recipe"""
    if recipes.has(recipe_id):
        return recipes[recipe_id].get_recipe_info()
    return null

func can_craft(recipe_id, player_stats, player_inventory):
    """Check if a specific recipe can be crafted by player"""
    if recipes.has(recipe_id):
        return recipes[recipe_id].can_craft(player_stats, player_inventory)
    return false
```

## Crafting Station Script (CraftingStation.gd)

```gdscript
extends Node2D

# Crafting station type
var station_type = "workbench"

# Available recipes at this crafting station
var available_recipes = []

func _ready():
    # Load recipes based on station type
    load_station_recipes()

func load_station_recipes():
    """Load recipes appropriate for this crafting station"""
    match station_type:
        "workbench":
            # Workbench can craft all basic items
            available_recipes = CraftingManager.get_available_recipes()
        "alchemy_table":
            # Alchemy table only specialized recipes
            pass

func get_crafting_options():
    """Get crafting options available at this station"""
    return {
        "station_type": station_type,
        "recipes": available_recipes,
        "available": true
    }
```

## Sample Materials and Items (items.json)

```json
{
  "items": [
    {
      "id": "wooden_sword",
      "name": "Holzschwert",
      "type": "weapon",
      "damage": 10,
      "durability": 50,
      "value": 20
    },
    {
      "id": "wooden_shield",
      "name": "Holzschild",
      "type": "armor",
      "defense": 8,
      "durability": 70,
      "value": 15
    },
    {
      "id": "health_potion",
      "name": "Gesundheitstrank",
      "type": "consumable",
      "effect": "heal",
      "effect_value": 30,
      "value": 10
    },
    {
      "id": "wood",
      "name": "Holz",
      "type": "material",
      "value": 5
    },
    {
      "id": "iron",
      "name": "Eisen",
      "type": "material",
      "value": 15
    },
    {
      "id": "herb",
      "name": "Kraut",
      "type": "material",
      "value": 3
    }
  ],
  
  "materials": [
    {
      "name": "wood",
      "description": "Holz aus Bäumen",
      "rarity": "common"
    },
    {
      "name": "iron",
      "description": "Eisenerz aus Erden",
      "rarity": "uncommon"
    },
    {
      "name": "herb",
      "description": "Heilpflanzen",
      "rarity": "common"
    }
  ]
}
```