# Item Properties Documentation

## Overview
This document describes all properties and characteristics of items in the game inventory system.

## Basic Item Properties

### Core Attributes
- **name**: Unique identifier for the item
- **type**: Category of the item (weapon, armor, accessory, consumable, crafting material)
- **quality**: Quality level (1-5) determining stats and rarity
- **size**: Storage space required in inventory (measured in slots)
- **weight**: Item weight affecting player movement and carrying capacity
- **description**: Detailed description of the item's purpose and properties

### Quality Levels
- **Quality 1 (Gray/Poor)**: Basic functionality, lowest stat bonuses
- **Quality 2 (Green/Common)**: Standard stats and attributes
- **Quality 3 (Blue/Uncommon)**: Enhanced stats with moderate bonuses
- **Quality 4 (Yellow/Rare)**: Significant stat improvements
- **Quality 5 (Orange/Epic)**: Powerful abilities with high bonuses
- **Quality 6 (Purple/Legendary)**: Maximum stat bonuses and exceptional abilities

### Rarity System
- **Common**: 70% chance of appearing
- **Uncommon**: 20% chance of appearing  
- **Rare**: 7% chance of appearing
- **Epic**: 2.5% chance of appearing
- **Legendary**: 0.5% chance of appearing

## Equipment-Specific Properties

### Weapons
- **damage**: Base damage output
- **attack_speed**: Rate of attacks per second
- **range**: Attack range in tiles
- **durability**: Maximum uses before breaking
- **enchantments**: Special effects or bonuses

### Armor
- **defense**: Protection value against damage
- **resistance**: Resistance to specific damage types
- **movement_speed**: Effect on player movement speed
- **weight_reduction**: Reduces overall item weight burden

### Accessories
- **stat_bonus**: Primary stat modification
- **special_ability**: Unique passive or active abilities
- **slot_type**: Type of equipment slot required

## Item Lifecycle

### Acquisition
- Monster drops (quality based on monster level)
- Quest rewards
- Crafting results
- Shop purchases

### Storage
- Inventory slots
- Backpack expansion tiers
- Equipment slots
- Crafting material storage

### Usage
- Equipping to character
- Consuming consumable items
- Selling or trading
- Crafting with materials

## Item Drop System

### Loot Containers
1. **Normal Chest**: 100% chance of common quality items
2. **Standard Chest**: 70% common, 30% uncommon quality items  
3. **Rare Chest**: 50% uncommon, 40% rare, 10% epic quality items
4. **Epic Chest**: 30% rare, 60% epic, 10% legendary quality items
5. **Master Chest**: Contains only legendary quality items

### Monster Drops
- Low-level monsters (1-10): Normal chests
- Mid-level monsters (11-30): Standard chests  
- High-level monsters (31-60): Rare chests
- Boss monsters (61+): Epic or Master chests