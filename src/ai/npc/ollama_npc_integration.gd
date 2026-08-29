# Ollama NPC Integration for MMORPG

## OllamaNPCManager.gd - Node that manages Ollama NPCs

```gdscript
extends Node

# Manages all Ollama-enabled NPCs in the game world
var npcs = {}
var default_model = "llama3"
var npc_conversations = {}

# Game events manager to get current events for context
var game_events_manager = null

func _ready():
    """Initialize the Ollama NPC system"""
    # Get reference to game events manager (should exist in the main scene)
    game_events_manager = get_node_or_null("/root/GameEventsManager")
    
    if !game_events_manager:
        print("Warning: GameEventsManager not found. Some NPC features may be limited.")

func create_npc(npc_id, npc_data):
    """Create and register a new NPC with Ollama capabilities"""
    var npc = {
        "id": npc_id,
        "name": npc_data.name,
        "model": npc_data.model || default_model,
        "knowledge_base": npc_data.knowledge_base,
        "personality": npc_data.personality,
        "active": true,
        "conversation_history": [],
        "last_interaction": 0,
        "context": {}
    }
    
    npcs[npc_id] = npc
    return npc

func get_npc(npc_id):
    """Get NPC by ID"""
    return npcs.get(npc_id, null)

func handle_player_interaction(npc_id, player_id, message):
    """Handle player interaction with Ollama NPC"""
    var npc = get_npc(npc_id)
    if !npc or !npc.active:
        return "Ich bin nicht verfügbar."
    
    # Update last interaction time
    npc.last_interaction = Time.get_ticks_msec()
    
    # Get context for response
    var context = build_context(npc, player_id, message)
    
    # For now, simulate Ollama response (would call actual API in real implementation)
    var response = generate_simulated_response(message, context)
    
    # Add to conversation history
    add_to_conversation_history(npc_id, player_id, message, response)
    
    return response

func build_context(npc, player_id, message):
    """Build contextual information for NPC response"""
    var context = {
        "npc_name": npc.name,
        "npc_personality": npc.personality,
        "player_id": player_id,
        "message": message,
        "game_events": get_game_events(),
        "npc_knowledge": npc.knowledge_base,
        "time_of_day": get_time_of_day(),
        "location": get_current_location(player_id)
    }
    
    return context

func get_game_events():
    """Get relevant game events for NPC response"""
    if !game_events_manager:
        return []
    
    # Get recent events for context
    var recent_events = game_events_manager.get_recent_events(10)
    return recent_events

func generate_simulated_response(message, context):
    """Generate a simulated Ollama-style response based on the given context"""
    var response = ""
    
    # Extract key information from message for better responses
    var message_lower = message.to_lower()
    
    # Check for auction mentions in the message
    if message_lower.find("auction") != -1 or message_lower.find("gegenstand") != -1 or message_lower.find("item") != -1:
        response = "Ah, du interessierst dich für Gegenstände? Ich habe gehört, dass eine seltenes Schwert in der Auktion zu verkaufen ist!"
    
    # Check for attack mentions
    elif message_lower.find("attack") != -1 or message_lower.find("angriff") != -1 or message_lower.find("schaden") != -1:
        response = "Kürzlich hat ein Spieler einen anderen angegriffen! Der Schurke auf dem Dorfplatz war heute besonders aggressiv."
    
    # Check for shop mentions
    elif message_lower.find("shop") != -1 or message_lower.find("händler") != -1 or message_lower.find("angebot") != -1:
        response = "Händler Käpt'n Rüdiger hat ein Spezialangebot! 50% Rabatt auf alle Rüstungen bis morgen!"
    
    # Default response (if no specific keywords)
    else:
        if context.game_events.size() > 0:
            var event = context.game_events[0]
            match event.type:
                "Auction":
                    response = "Es gibt ein neues Item in der Auktion: " + event.data.item_name + ". Was möchtest du darüber wissen?"
                "PlayerAttack": 
                    response = "Ein Angriff ist gerade passiert! " + event.data.attacker + " hat " + event.data.target + " angegriffen."
                "SpecialOffer":
                    response = "Neues Sonderangebot von " + event.data.vendor + ": " + event.data.offer
                _:
                    response = "Ich habe viele Informationen über diese Welt. Wie kann ich dir helfen?"
        else:
            response = "Ich bin ein NPC in diesem MMORPG. Ich kann dir helfen, bestimmte Ereignisse zu erfahren oder Details über die Welt zu erhalten."
    
    return response

func add_to_conversation_history(npc_id, player_id, message, response):
    """Add conversation to history"""
    if !npc_conversations.has(npc_id):
        npc_conversations[npc_id] = {}
    
    if !npc_conversations[npc_id].has(player_id):
        npc_conversations[npc_id][player_id] = []
    
    var conversation = npc_conversations[npc_id][player_id]
    conversation.append({
        "message": message,
        "response": response,
        "timestamp": Time.get_ticks_msec()
    })
    
    # Keep only last 20 messages
    if conversation.size() > 20:
        conversation.remove_at(0)

func get_conversation_history(npc_id, player_id):
    """Get conversation history for player and NPC"""
    return npc_conversations.get(npc_id, {}).get(player_id, [])

# This would be called when connecting to an actual Ollama server
func connect_to_ollama_server():
    """Connect to the Ollama AI server"""
    # In a real implementation, this would make HTTP requests to 
    # http://localhost:11434/api/generate (or similar)
    return true

# Utility methods for NPC management
func get_npc_status(npc_id):
    """Get status information for an NPC"""
    var npc = get_npc(npc_id)
    if npc:
        return {
            "name": npc.name,
            "active": npc.active,
            "last_interaction": npc.last_interaction,
            "model": npc.model
        }
    else:
        return null

func set_npc_active(npc_id, active):
    """Set NPC active or inactive"""
    var npc = get_npc(npc_id)
    if npc:
        npc.active = active
        return true
    return false

func update_npc_knowledge_base(npc_id, new_knowledge):
    """Update NPC's knowledge base"""
    var npc = get_npc(npc_id)
    if npc:
        if !npc.knowledge_base:
            npc.knowledge_base = {}
        
        for key in new_knowledge:
            npc.knowledge_base[key] = new_knowledge[key]
        
        return true
    return false

# Time and location helpers (would connect to actual game systems)
func get_time_of_day():
    """Get current time of day for context"""
    # This would be connected to the game's time system
    return "Nachmittag"

func get_current_location(player_id):
    """Get player location for context"""
    # This would be connected to the game world 
    return "Dorfplatz"
```

## Enhanced Item Classes with Ollama Context

The Item classes are already implemented in your existing code. We will update them slightly to provide better context to Ollama:

```gdscript
# Item.gd (Extended version with additional fields for NPC context)
tool
extends Resource

# Base item class extended for Ollama integration
class_name Item

# Inherited properties from base item (using same format as your existing code)
var name: String = "Unbenannter Gegenstand"
var description: String = "Beschreibung fehlt"
var value: int = 0
var weight: int = 0
var max_stack_size: int = 1

# Additional fields for Ollama context and integration
var item_type: String = "unknown" # item, weapon, armor, consumable
var rarity: String = "normal" # common, uncommon, rare, epic, legendary
var category: String = "other"
var usage_count: int = 0
var last_used_time: int = 0
var context_tags: Array[String] = [] # Tags for better NPC context

# Properties specific to Ollama integration
var ollama_prompt_suffix: String = ""
var related_events: Array[Dictionary] = []

func _init(name_param: String = "", description_param: String = ""):
    name = name_param
    description = description_param

func get_context_info():
    """Returns context information about this item for Ollama integration"""
    return {
        "name": name,
        "type": item_type,
        "rarity": rarity,
        "category": category,
        "usage_count": usage_count,
        "value": value,
        "weight": weight
    }

# This would be used by NPCs when describing items
func get_item_description_for_npc():
    """Get formatted description for NPC storytelling"""
    var desc = name + " - " + description
    if rarity != "normal":
        desc += " (Rarität: " + rarity + ")"
    
    if item_type != "item":
        desc += " Typ: " + item_type
    
    return desc

# Method to add events related to this item
func add_related_event(event_data):
    """Add event data that happened with this item"""
    related_events.append(event_data)
```

## NPC Script Updates

```gdscript
# NPC.gd (Extended version for Ollama integration)
extends CharacterBody2D

# Ollama-powered NPC extension
export var npc_id = ""
export var npc_name = "Unbekannter NPC"
export var personality = "Neugierig"

# Add reference to Ollama manager and event system
var ollama_manager = null
var game_events_manager = null
var is_interacting = false

func _ready():
    """Initialize enhanced NPC with Ollama support"""
    if npc_id == "":
        npc_id = "npc_" + str(InstanceID)
    
    # Get references to managers
    ollama_manager = get_node_or_null("/root/OllamaNPCManager")
    game_events_manager = get_node_or_null("/root/GameEventsManager")
    
    # Setup NPC with Ollama capabilities if manager exists
    if ollama_manager:
        var npc_data = {
            "name": npc_name,
            "model": "llama3",
            "personality": personality,
            "knowledge_base": {
                "greetings": ["Hallo", "Guten Tag", "Sei gegrüßt"],
                "default_response": "Ich bin ein NPC im Spiel. Ich kann dir über aktuelle Ereignisse erzählen."
            }
        }
        
        ollama_manager.create_npc(npc_id, npc_data)
    
    # Setup interaction detection
    $InteractionZone.connect("area_entered", self, "_on_interaction_zone_enter")
    $InteractionZone.connect("area_exited", self, "_on_interaction_zone_exit")

func _on_interaction_zone_enter(area):
    """Handle player entering interaction zone"""
    if area.get_name() == "Player":
        # Show interaction hint
        show_interaction_hint()

func _on_interaction_zone_exit(area):
    """Handle player leaving interaction zone"""
    if area.get_name() == "Player":
        # Hide interaction hint
        hide_interaction_hint()

func start_interaction(player_id):
    """Start Ollama-powered interaction with player"""
    is_interacting = true
    
    if ollama_manager:
        print("NPC " + npc_name + " - Spielerinteraktion gestartet")
        
        # Ask a question about recent game events
        var initial_message = "Was gibt es Neues in der Region?"
        var response = ollama_manager.handle_player_interaction(npc_id, player_id, initial_message)
        show_dialog(response)

func end_interaction():
    """End the interaction"""
    is_interacting = false
    hide_dialog()

func show_interaction_hint():
    """Show interaction hint when player is near"""
    # Could use a UI element here to indicate interaction possibility

func hide_interaction_hint():
    """Hide interaction hint"""
    # Hide UI element

func show_dialog(message):
    """Display dialog box with NPC's message"""
    if message:
        print("[NPC " + npc_name + "] " + message)

func hide_dialog():
    """Hide dialog box"""
    pass

# This function would be called when a player interacts via a command or keypress
func interact_with_player(player_id):
    """Handle direct player interaction with this NPC"""
    if ollama_manager:
        start_interaction(player_id)
        return true
    return false
```

## Integration Guide for Existing MMORPG

### 1. Add to your game scene:
Add these nodes to your main scene:

- **OllamaNPCManager** (Node) - at root level
- **GameEventsManager** (if not already present, create based on code above)

### 2. Update existing NPC script:
Replace your current NPC implementation with the enhanced one.

### 3. Add game events system:
Make sure to add the GameEventsManager to handle real-time events like auctions, attacks, and offers.

### 4. Usage example:
```gdscript
# Create game events for NPCs to reference
var auction_event = GameEventsManager.create_auction_event(
    "Legendäres Schwert der Lichtung",
    "Ein seltenes Wunderwaffe aus dem Kriegsviertel.",
    "3 Stunden"
)

var attack_event = GameEventsManager.create_attack_event(
    "Schurke",
    "Spieler123", 
    "Dorfplatz",
    "Vor einer Stunde"
)

# When player interacts with NPC
NPC.interact_with_player("player_123")
```

### 5. Configuration:
Set up your Ollama server and update the connection parameters in `OllamaNPCManager.gd` if you want real API integration.

The system will now provide NPCs that can dynamically respond to player questions with context-specific information about recent game events, making the world feel more alive.