# Godot Quest System

## Quest Node Script (Quest.gd)

```gdscript
extends Node

# Quest data structure
var quest_id = ""
var title = ""
var description = ""
var objective = ""
var reward = {
    "experience": 0,
    "gold": 0,
    "items": []
}
var is_active = false
var is_completed = false
var progress = 0
var max_progress = 1

# Quest requirements
var required_level = 1
var required_quests = [] # Quest IDs that must be completed first

func _ready():
    pass

func start_quest():
    is_active = true
    progress = 0
    print("Quest started: " + title)

func complete_quest():
    is_completed = true
    is_active = false
    print("Quest completed: " + title)
    
    # Give rewards when quest completed
    return reward

func update_progress(amount):
    if not is_completed and is_active:
        progress += amount
        if progress >= max_progress:
            complete_quest()

# Check if player can take this quest
func can_take(player_level, completed_quests):
    if player_level < required_level:
        return false
        
    for quest_id in required_quests:
        if not completed_quests.has(quest_id):
            return false
            
    return true

# Get quest info for UI
func get_quest_info():
    return {
        "id": quest_id,
        "title": title,
        "description": description,
        "objective": objective,
        "is_active": is_active,
        "is_completed": is_completed,
        "progress": progress,
        "max_progress": max_progress
    }
```

## Quest Manager Script (QuestManager.gd)

```gdscript
extends Node

# All available quests
var all_quests = {}
var active_quests = {}
var completed_quests = {}

func _ready():
    # Initialize some sample quests
    initialize_sample_quests()

func initialize_sample_quests():
    # Sample quest 1: Introduction to the world
    var introduction_quest = Quest.new()
    introduction_quest.quest_id = "intro_001"
    introduction_quest.title = "Die Einführung in die Welt"
    introduction_quest.description = "Erkunde die erste Region und lerne die Grundlagen des Spiels."
    introduction_quest.objective = "Reise zu der ersten Stadt"
    introduction_quest.required_level = 1
    introduction_quest.reward = {
        "experience": 50,
        "gold": 20,
        "items": []
    }
    introduction_quest.max_progress = 1
    
    all_quests[introduction_quest.quest_id] = introduction_quest
    
    # Sample quest 2: First monster defeat
    var monster_quest = Quest.new()
    monster_quest.quest_id = "monster_001"
    monster_quest.title = "Erste Niederlage"
    monster_quest.description = "Besiege dein erstes Monster in der Wildnis."
    monster_quest.objective = "Töte 5 Slimes"
    monster_quest.required_level = 3
    monster_quest.reward = {
        "experience": 100,
        "gold": 50,
        "items": []
    }
    monster_quest.max_progress = 5
    
    all_quests[monster_quest.quest_id] = monster_quest

func start_quest(quest_id):
    if all_quests.has(quest_id):
        var quest = all_quests[quest_id]
        if not active_quests.has(quest_id) and not completed_quests.has(quest_id):
            active_quests[quest_id] = quest
            quest.start_quest()
            return true
    return false

func complete_quest(quest_id):
    if active_quests.has(quest_id):
        var quest = active_quests[quest_id]
        var reward = quest.complete_quest()
        
        # Move to completed quests
        completed_quests[quest_id] = quest
        active_quests.erase(quest_id)
        
        return reward
    return null

func update_quest_progress(quest_id, amount):
    if active_quests.has(quest_id):
        var quest = active_quests[quest_id]
        quest.update_progress(amount)

func get_active_quests():
    return active_quests

func get_completed_quests():
    return completed_quests

func can_take_quest(quest_id, player_level):
    if all_quests.has(quest_id):
        var quest = all_quests[quest_id]
        return quest.can_take(player_level, completed_quests)
        
    return false

func get_quest_info(quest_id):
    if all_quests.has(quest_id):
        return all_quests[quest_id].get_quest_info()
    return null
```

## Sample Quest Data File (quests/sample_quests.json)

```json
{
  "quests": [
    {
      "id": "intro_001",
      "title": "Die Einführung in die Welt",
      "description": "Erkunde die erste Region und lerne die Grundlagen des Spiels.",
      "objective": "Reise zu der ersten Stadt",
      "required_level": 1,
      "reward": {
        "experience": 50,
        "gold": 20,
        "items": []
      },
      "max_progress": 1
    },
    {
      "id": "monster_001",
      "title": "Erste Niederlage",
      "description": "Besiege dein erstes Monster in der Wildnis.",
      "objective": "Töte 5 Slimes",
      "required_level": 3,
      "reward": {
        "experience": 100,
        "gold": 50,
        "items": []
      },
      "max_progress": 5
    }
  ]
}
```