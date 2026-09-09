# Inventory System Documentation

## Overview

This system manages player inventory with backpack expansion capabilities, allowing players to store and organize items during gameplay. The system encompasses all game elements including weapons, armor, equipment, and crafting materials.

## Core Functionality

### Item Management
The inventory system handles:
- Item storage and organization
- Equipment management (weapons, armor, etc.)
- Inventory size constraints (standard 8 slots)
- Backpack expansion system (4/8/12/16/20/24/30/38/42/48/52/64 slot sizes)
- Crafting material storage and organization
- Equipment slot management for weapons, armor and accessories

## Technical Implementation

### Item Structure
Each item in the inventory follows a standardized structure:
```json
{
  "name": "Ancient Sword",
  "type": "weapon",
  "quality": 5,
  "size": 16,
  "weight": 3.2,
  "description": "A legendary sword that glows with ancient power"
}
```

### Quality Color System
Items dropped in the world are classified by quality colors with decreasing rarity:
- **Gray (Poor)**: Lowest quality, basic stats
- **Green (Common)**: Standard quality items
- **Blue (Uncommon)**: Better quality with enhanced stats
- **Yellow (Rare)**: High quality with significant bonuses
- **Orange (Epic)**: Very rare items with powerful attributes
- **Purple (Legendary)**: Highest quality, exceptional stats and abilities

### Quality Levels
Equipment items have minimum level requirements and multiple quality levels with additional attributes:
- **Quality 1 (Common)**: Basic stats, no special attributes
- **Quality 2 (Uncommon)**: +10% stat bonus, basic special attributes
- **Quality 3 (Rare)**: +20% stat bonus, moderate special attributes
- **Quality 4 (Epic)**: +30% stat bonus, significant special attributes
- **Quality 5 (Legendary)**: +40% stat bonus, powerful special attributes

Each quality level provides:
- Enhanced base stats
- Unique special abilities or bonuses
- Higher item size requirements
- Increased weight and durability

### Minimum Level Requirements
All equipment items have minimum level requirements for usage:
- Common items: Level 1
- Uncommon items: Level 5
- Rare items: Level 10
- Epic items: Level 20
- Legendary items: Level 30

### Equipment Tier System
All equipment (weapons, armor, accessories) follows a tier-based system that aligns with player levels:
- Tier 0: Level 1-10
- Tier 1: Level 11-20
- Tier 2: Level 21-30
- Tier 3: Level 31-40
- Tier 4: Level 41-50
- Tier 5: Level 51-60
- Tier 6: Level 61-70
- Tier 7: Level 71-80
- Tier 8: Level 81-90
- Tier 9: Level 91-100
- Tier 10: Level 101-110
- Tier 11: Level 111+

### Backpack Expansion System
Backpack expansions are earned through in-game progression (level tiers). They are never purchased with real money or with gold; expansion is tied to player level and remains an in-game progression reward (see `Monetarisierung_und_Donations.md`).

The system allows players to expand their inventory through:
- Tier 0 backpacks (4 slots) - Level 1-10
- Tier 1 backpacks (8 slots) - Level 11-20
- Tier 2 backpacks (12 slots) - Level 21-30
- Tier 3 backpacks (16 slots) - Level 31-40
- Tier 4 backpacks (20 slots) - Level 41-50
- Tier 5 backpacks (24 slots) - Level 51-60
- Tier 6 backpacks (30 slots) - Level 61-70
- Tier 7 backpacks (38 slots) - Level 71-80
- Tier 8 backpacks (42 slots) - Level 81-90
- Tier 9 backpacks (48 slots) - Level 91-100
- Tier 10 backpacks (52 slots) - Level 101-110
- Tier 11 backpacks (64 slots) - Level 111+

Each backpack size corresponds to a specific level tier:
- Tier 0: Level 1-10
- Tier 1: Level 11-20
- Tier 2: Level 21-30
- Tier 3: Level 31-40
- And so on...

### Inventory System Architecture
The inventory system consists of:
1. **Item Storage**: Core inventory management
2. **Backpack Management**: Expansion and sizing logic
3. **Equipment Slot**: Specialized areas for equipped items (weapons, armor, accessories)
4. **Crafting Materials**: Dedicated storage for crafting resources
5. **UI Integration**: Visual representation in the game

## Integration Points

### Boss System Integration
- Item rewards from boss battles are added to player inventory
- Special item drops that trigger personal cutscenes when found
- Inventory size affects how many items the player can carry,
  but does not affect item drop chance or item quality.

### Quest System Integration
- Items required for quests are tracked
- Quest completion rewards are added to inventory
- Special quest items have unique properties and storage requirements

### AI Cutscene Integration
- Personalized cutscenes based on item discoveries
- Inventory expansion events trigger celebration cutscenes
- Rare item finds generate unique storytelling moments

### Crafting System Integration
- Crafting materials stored in dedicated inventory slots
- Equipment crafting requires specific item combinations
- Crafted items are added to player inventory with proper sizing calculations
- Recipe availability is restricted to player level ranges
- Only recipes corresponding to the player's level tier can be used for crafting