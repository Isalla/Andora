# Godot Inventory and Item Management System

## Base Item Script (Item.gd)

```gdscript
extends Resource

# Base item properties
var item_id = ""
var name = "Unbenannter Gegenstand"
var description = ""
var item_type = "generic"  # weapon, armor, consumable, quest, material
var rarity = "common"  # common, uncommon, rare, epic, legendary
var stackable = false
var max_stack = 1
var value = 0  # Gold value of the item
var weight = 0

# Item stats (if applicable)
var stats = {
    "strength": 0,
    "agility": 0,
    "intelligence": 0,
    "health": 0,
    "mana": 0
}

# Item usage
var use_effect = ""
var use_target = ""  # self, target, area

func _init(item_data = null):
    """Initialize item with data"""
    if item_data:
        for key in item_data:
            if has_method("set_" + key):
                call("set_" + key, item_data[key])
            else:
                set(key, item_data[key])

func use(player):
    """Use the item (override in child classes)"""
    print("Using item: " + name)
    return true

func get_item_info():
    """Get information about this item for UI display"""
    return {
        "id": item_id,
        "name": name,
        "description": description,
        "type": item_type,
        "rarity": rarity,
        "stackable": stackable,
        "value": value,
        "stats": stats
    }

func is_equippable():
    """Check if item can be equipped"""
    return item_type in ["weapon", "armor", "accessory"]

func get_rarity_color():
    """Get color based on rarity for UI display"""
    var colors = {
        "common": "[color=white]",
        "uncommon": "[color=green]",
        "rare": "[color=blue]",
        "epic": "[color=purple]",
        "legendary": "[color=gold]"
    }
    
    return colors.get(rarity, "[color=white]")
```

## Weapon Script (Weapon.gd)

```gdscript
extends Item

# Weapon specific properties
var weapon_type = "sword"  # sword, staff, bow, axe, dagger
var damage_min = 0
var damage_max = 0
var attack_speed = 1.0
var range = 1
var durability = 100
var max_durability = 100

func _init(item_data = null):
    """Initialize weapon with data"""
    item_type = "weapon"
    
    if item_data:
        for key in item_data:
            if has_method("set_" + key):
                call("set_" + key, item_data[key])
            else:
                set(key, item_data[key])

func get_damage():
    """Get damage range for weapon"""
    return {
        "min": damage_min,
        "max": damage_max
    }

func is_damaged():
    """Check if weapon is damaged"""
    return durability < max_durability

func use(player):
    """Use the weapon (in this case, equip it)"""
    if player:
        # Logic for equipping weapon would go here
        print("Equipped weapon: " + name)
        return true
    return false

func degrade():
    """Decrease weapon durability"""
    if durability > 0:
        durability -= 1
        if durability <= 0:
            print("Weapon broke!")
            return true
    return false

func get_weapon_info():
    """Get weapon specific info for UI display"""
    var base_info = get_item_info()
    return base_info.merge({
        "weapon_type": weapon_type,
        "damage": get_damage(),
        "attack_speed": attack_speed,
        "range": range,
        "durability": durability,
        "max_durability": max_durability
    })
```

## Armor Script (Armor.gd)

```gdscript
extends Item

# Armor specific properties
var armor_type = "chest"  # head, chest, legs, feet, shield
var defense = 0
var durability = 100
var max_duraibility = 100

func _init(item_data = null):
    """Initialize armor with data"""
    item_type = "armor"
    
    if item_data:
        for key in item_data:
            if has_method("set_" + key):
                call("set_" + key, item_data[key])
            else:
                set(key, item_data[key])

func use(player):
    """Use the armor (in this case, equip it)"""
    if player:
        # Logic for equipping armor would go here
        print("Equipped armor: " + name)
        return true
    return false

func degrade():
    """Decrease armor durability"""
    if durability > 0:
        durability -= 1
        if durability <= 0:
            print("Armor broken!")
            return true
    return false

func get_armor_info():
    """Get armor specific info for UI display"""
    var base_info = get_item_info()
    return base_info.merge({
        "armor_type": armor_type,
        "defense": defense,
        "durability": durability,
        "max_durability": max_duraibility
    })
```

## Consumable Script (Consumable.gd)

```gdscript
extends Item

# Consumable specific properties
var effect = ""
var effect_value = 0
var duration = 0  # seconds
var target = "player"  # player, party, area

func _init(item_data = null):
    """Initialize consumable with data"""
    item_type = "consumable"
    stackable = true
    
    if item_data:
        for key in item_data:
            if has_method("set_" + key):
                call("set_" + key, item_data[key])
            else:
                set(key, item_data[key])

func use(player):
    """Use the consumable item"""
    if player:
        match effect.lower():
            "heal":
                player.health += effect_value
                print("Used health potion: +" + str(effect_value) + " HP")
            "mana_restore":
                player.mana += effect_value
                print("Used mana potion: +" + str(effect_value) + " MP")
            "stat_boost":
                # Apply stat boost temporary
                player.strength += effect_value
                print("Used stat boost item")
        
        return true
    
    return false

func get_consumable_info():
    """Get consumable specific info for UI display"""
    var base_info = get_item_info()
    return base_info.merge({
        "effect": effect,
        "value": effect_value,
        "duration": duration,
        "target": target
    })
```

## Inventory Manager Script (InventoryManager.gd)

```gdscript
extends Node

# Main inventory system
var player_inventories = {}
var item_database = {}

func _ready():
    """Initialize the inventory system"""
    load_item_database()

func load_item_database():
    """Load all items from data files"""
    # In a real implementation, this would load from JSON files
    
    # Sample item database
    item_database = {
        "health_potion": {
            "name": "Gesundheitstrank",
            "description": "Regeneriert gesunde Lebenspunkte",
            "item_type": "consumable",
            "rarity": "common",
            "value": 20,
            "weight": 1,
            "stackable": true,
            "effect": "heal",
            "effect_value": 50
        },
        
        "mana_potion": {
            "name": "Manapotion",
            "description": "Regeneriert magische Energien",
            "item_type": "consumable",
            "rarity": "common",
            "value": 30,
            "weight": 1,
            "stackable": true,
            "effect": "mana_restore",
            "effect_value": 30
        },
        
        "wooden_sword": {
            "name": "Holzsäbel",
            "description": "Ein einfacher Holzschwert",
            "item_type": "weapon",
            "rarity": "common",
            "value": 50,
            "weight": 3,
            "stackable": false,
            "weapon_type": "sword",
            "damage_min": 10,
            "damage_max": 20
        },
        
        "leather_armor": {
            "name": "Lederrüstung",
            "description": "Mäßige Rüstung aus Leder",
            "item_type": "armor",
            "rarity": "common",
            "value": 100,
            "weight": 5,
            "stackable": false,
            "armor_type": "chest",
            "defense": 15
        }
    }

func create_player_inventory(player_id):
    """Create a new inventory for a player"""
    if !player_inventories.has(player_id):
        var inventory = {
            "items": {},
            "capacity": 30,
            "weight": 0,
            "max_weight": 100
        }
        
        player_inventories[player_id] = inventory
        return inventory
    
    return player_inventories[player_id]

func get_player_inventory(player_id):
    """Get a player's inventory"""
    if player_inventories.has(player_id):
        return player_inventories[player_id]
    
    # If no inventory, create one
    return create_player_inventory(player_id)

func add_item_to_inventory(player_id, item_id, count = 1):
    """Add items to player's inventory"""
    var inventory = get_player_inventory(player_id)
    if !inventory:
        return false
    
    # Check capacity and weight
    if not can_add_item(inventory, item_id, count):
        print("Inventory full or too heavy")
        return false
    
    if !inventory.items.has(item_id):
        inventory.items[item_id] = {
            "count": 0,
            "equipped": false
        }
    
    inventory.items[item_id].count += count
    
    # Update weight
    var item_data = item_database[item_id]
    if item_data:
        inventory.weight += (item_data.weight * count)
    
    print("Added " + str(count) + " of " + item_id + " to inventory")
    return true

func remove_item_from_inventory(player_id, item_id, count = 1):
    """Remove items from player's inventory"""
    var inventory = get_player_inventory(player_id)
    if !inventory or !inventory.items.has(item_id):
        return false
    
    if inventory.items[item_id].count >= count:
        inventory.items[item_id].count -= count
        
        # Update weight
        var item_data = item_database[item_id]
        if item_data:
            inventory.weight -= (item_data.weight * count)
        
        # Remove item if count is 0
        if inventory.items[item_id].count <= 0:
            inventory.items.erase(item_id)
        
        print("Removed " + str(count) + " of " + item_id + " from inventory")
        return true
    
    return false

func can_add_item(inventory, item_id, count = 1):
    """Check if item can be added to inventory"""
    var item_data = item_database[item_id]
    
    if !item_data:
        return false
    
    # Check weight capacity
    var new_weight = inventory.weight + (item_data.weight * count)
    if new_weight > inventory.max_weight:
        return false
    
    # Check stackable items
    if item_data.stackable and inventory.items.has(item_id):
        # Simple check for now
        return true
    
    # Check inventory space
    var total_items = 0
    for item in inventory.items:
        total_items += item.value.count
    
    return (total_items + count) <= inventory.capacity

func equip_item(player_id, slot, item_id):
    """Equip an item to a specific slot"""
    # This would typically call player's equip function
    print("Equipping " + item_id + " in slot: " + slot)
    
    if !player_inventories.has(player_id):
        return false
    
    var inventory = player_inventories[player_id]
    if inventory.items.has(item_id) and inventory.items[item_id].count > 0:
        inventory.items[item_id].equipped = true
        print("Item equipped: " + item_id)
        return true
    
    return false

func unequip_item(player_id, slot, item_id):
    """Unequip an item from a specific slot"""
    if !player_inventories.has(player_id):
        return false
    
    var inventory = player_inventories[player_id]
    if inventory.items.has(item_id) and inventory.items[item_id].equipped:
        inventory.items[item_id].equipped = false
        print("Item unequipped: " + item_id)
        return true
    
    return false

func get_inventory_info(player_id):
    """Get complete inventory information for UI"""
    var inventory = get_player_inventory(player_id)
    
    if !inventory:
        return null
    
    return {
        "items": inventory.items,
        "capacity": inventory.capacity,
        "weight": inventory.weight,
        "max_weight": inventory.max_weight
    }

func use_item(player_id, item_id):
    """Use an item from inventory"""
    var inventory = get_player_inventory(player_id)
    if !inventory or !inventory.items.has(item_id):
        return false
    
    var item_data = item_database[item_id]
    if !item_data:
        return false
    
    match item_data.item_type:
        "consumable":
            # Create consumable and use it
            var consumable = Consumable.new(item_data)
            return consumable.use(null)  # Pass in player to use on
            
        _:
            print("Item cannot be used directly")
            return false

func transfer_item(from_player, to_player, item_id, count = 1):
    """Transfer an item from one player to another"""
    if remove_item_from_inventory(from_player, item_id, count):
        add_item_to_inventory(to_player, item_id, count)
        print("Transferred " + str(count) + " of " + item_id + " from " + from_player + " to " + to_player)
        return true
    
    return false
```

## Item Database System (ItemDatabase.gd)

```gdscript
extends Node

# Item database manager for loading and managing game items
var items = {}
var categories = []

func _ready():
    """Initialize item database"""
    load_base_items()
    load_item_categories()

func load_base_items():
    """Load base items into database"""
    # This would normally load from JSON files or database
    
    # Sample items data
    items = {
        "health_potion": {
            "id": "health_potion",
            "name": "Gesundheitstrank",
            "description": "Regeneriert 50 Lebenspunkte",
            "type": "consumable",
            "rarity": "common",
            "value": 20,
            "weight": 1,
            "stackable": true,
            "effects": {
                "heal": 50
            }
        },
        
        "mana_potion": {
            "id": "mana_potion",
            "name": "Manapotion",
            "description": "Regeneriert 30 Manapunkte",
            "type": "consumable",
            "rarity": "common",
            "value": 30,
            "weight": 1,
            "stackable": true,
            "effects": {
                "mana_restore": 30
            }
        },
        
        "wooden_sword": {
            "id": "wooden_sword",
            "name": "Holzsäbel",
            "description": "Ein einfach gebauter Säbel aus Holz",
            "type": "weapon",
            "rarity": "common",
            "value": 50,
            "weight": 3,
            "stackable": false,
            "stats": {
                "damage_min": 10,
                "damage_max": 20
            }
        },
        
        "leather_armor": {
            "id": "leather_armor",
            "name": "Lederrüstung",
            "description": "Mäßige Rüstung aus Leder",
            "type": "armor",
            "rarity": "common",
            "value": 100,
            "weight": 5,
            "stackable": false,
            "stats": {
                "defense": 15
            }
        },
        
        "iron_sword": {
            "id": "iron_sword",
            "name": "Eisenschwert",
            "description": "Ein stabiles Schwert aus Eisen",
            "type": "weapon",
            "rarity": "uncommon",
            "value": 200,
            "weight": 4,
            "stackable": false,
            "stats": {
                "damage_min": 25,
                "damage_max": 40
            }
        },
        
        "steel_helmet": {
            "id": "steel_helmet",
            "name": "Stahlhelm",
            "description": "Robuster Helm aus Stahl",
            "type": "armor",
            "rarity": "uncommon",
            "value": 150,
            "weight": 3,
            "stackable": false,
            "stats": {
                "defense": 25
            }
        }
    }

func load_item_categories():
    """Load item categories"""
    categories = ["weapon", "armor", "consumable", "quest", "material"]

func get_item(item_id):
    """Get a specific item by ID"""
    if items.has(item_id):
        return items[item_id]
    return null

func get_items_by_type(item_type):
    """Get all items of a certain type"""
    var result = []
    for item_id in items:
        if items[item_id].type == item_type:
            result.append(items[item_id])
    
    return result

func get_items_by_rarity(rarity):
    """Get all items of a certain rarity"""
    var result = []
    for item_id in items:
        if items[item_id].rarity == rarity:
            result.append(items[item_id])
    
    return result

func search_items(query):
    """Search for items by name or description"""
    var results = []
    query = query.to_lower()
    
    for item_id in items:
        var item = items[item_id]
        if item.name.to_lower().find(query) != -1 or item.description.to_lower().find(query) != -1:
            results.append(item)
    
    return results

func get_all_items():
    """Get all items in the database"""
    return items

func get_item_count():
    """Get total number of items"""
    return items.size()

func get_rarity_statistics():
    """Get statistics about item rarities"""
    var stats = {
        "common": 0,
        "uncommon": 0,
        "rare": 0,
        "epic": 0,
        "legendary": 0
    }
    
    for item_id in items:
        var rarity = items[item_id].rarity
        if stats.has(rarity):
            stats[rarity] += 1
    
    return stats
```

## Equipment Manager (EquipmentManager.gd)

```gdscript
extends Node

# Manage player equipment and stats
var equipped_items = {}

func _ready():
    """Initialize equipment manager"""
    pass

func equip_item(player, item_id, slot):
    """Equip an item to a specific slot"""
    if !equipped_items.has(player.player_id):
        equipped_items[player.player_id] = {}
    
    var player_equipment = equipped_items[player.player_id]
    
    # Unequip existing item in slot
    if player_equipment.has(slot) and player_equipment[slot] != null:
        unequip_item(player, slot)
    
    # Check if item can be equipped in this slot
    if !can_equip_item(item_id, slot):
        print("Cannot equip item in slot: " + slot)
        return false
    
    player_equipment[slot] = item_id
    
    # Update player stats
    update_player_stats(player)
    print("Equipped " + item_id + " to " + slot)
    
    return true

func unequip_item(player, slot):
    """Unequip an item from a specific slot"""
    if !equipped_items.has(player.player_id):
        return false
    
    var player_equipment = equipped_items[player.player_id]
    
    if player_equipment.has(slot) and player_equipment[slot] != null:
        var item_id = player_equipment[slot]
        
        # Return the item to inventory
        player.add_to_inventory(item_id, 1)
        
        # Remove from equipment
        player_equipment[slot] = null
        
        # Update player stats
        update_player_stats(player)
        print("Unequipped " + item_id + " from " + slot)
        
        return true
    
    return false

func can_equip_item(item_id, slot):
    """Check if an item can be equipped in a specific slot"""
    var item_data = ItemDatabase.get_item(item_id)
    if !item_data:
        return false
    
    # Check weapon slot requirements
    match slot:
        "weapon":
            return item_data.type == "weapon"
        "armor":
            return item_data.type == "armor"
        "head", "chest", "legs", "feet":
            return item_data.type == "armor" and item_data.stats.armor_type == slot
        _:
            # General check for other slots
            return item_data.type in ["weapon", "armor"]
    
    return false

func update_player_stats(player):
    """Update player stats based on equipped items"""
    var total_strength = 0
    var total_agility = 0
    var total_intelligence = 0
    var total_defense = 0
    
    if equipped_items.has(player.player_id):
        var equipment = equipped_items[player.player_id]
        
        for slot in equipment:
            if equipment[slot]:
                var item_data = ItemDatabase.get_item(equipment[slot])
                if item_data and item_data.stats:
                    if item_data.stats.strength:
                        total_strength += item_data.stats.strength
                    if item_data.stats.agility:
                        total_agility += item_data.stats.agility
                    if item_data.stats.intelligence:
                        total_intelligence += item_data.stats.intelligence
                    if item_data.stats.defense:
                        total_defense += item_data.stats.defense
    
    # Apply bonus effects
    player.strength = player.strength + total_strength
    player.agility = player.agility + total_agility
    player.intelligence = player.intelligence + total_intelligence
    player.constitution = player.constitution + total_defense

func get_equipped_items(player_id):
    """Get list of equipped items for a player"""
    if equipped_items.has(player_id):
        return equipped_items[player_id]
    return {}

func get_equipped_item_info(player_id):
    """Get detailed info about equipped items"""
    var equipment = get_equipped_items(player_id)
    var info = {}
    
    for slot in equipment:
        if equipment[slot]:
            var item_data = ItemDatabase.get_item(equipment[slot])
            info[slot] = item_data
    
    return info
```

## Item Data File (items/base_items.json)

```json
{
  "items": [
    {
      "id": "health_potion",
      "name": "Gesundheitstrank",
      "description": "Regeneriert gesunde Lebenspunkte",
      "type": "consumable",
      "rarity": "common",
      "value": 20,
      "weight": 1,
      "stackable": true,
      "effects": {
        "heal": 50
      }
    },
    {
      "id": "mana_potion",
      "name": "Manapotion",
      "description": "Regeneriert magische Energien",
      "type": "consumable",
      "rarity": "common",
      "value": 30,
      "weight": 1,
      "stackable": true,
      "effects": {
        "mana_restore": 30
      }
    },
    {
      "id": "wooden_sword",
      "name": "Holzsäbel",
      "description": "Ein einfacher Holzschwert",
      "type": "weapon",
      "rarity": "common",
      "value": 50,
      "weight": 3,
      "stackable": false,
      "stats": {
        "weapon_type": "sword",
        "damage_min": 10,
        "damage_max": 20
      }
    },
    {
      "id": "leather_armor",
      "name": "Lederrüstung",
      "description": "Mäßige Rüstung aus Leder",
      "type": "armor",
      "rarity": "common",
      "value": 100,
      "weight": 5,
      "stackable": false,
      "stats": {
        "armor_type": "chest",
        "defense": 15
      }
    },
    {
      "id": "iron_sword",
      "name": "Eisenschwert",
      "description": "Ein stabiles Schwert aus Eisen",
      "type": "weapon",
      "rarity": "uncommon",
      "value": 200,
      "weight": 4,
      "stackable": false,
      "stats": {
        "weapon_type": "sword",
        "damage_min": 25,
        "damage_max": 40
      }
    },
    {
      "id": "steel_helmet",
      "name": "Stahlhelm",
      "description": "Robuster Helm aus Stahl",
      "type": "armor",
      "rarity": "uncommon",
      "value": 150,
      "weight": 3,
      "stackable": false,
      "stats": {
        "armor_type": "head",
        "defense": 25
      }
    }
  ]
}
```