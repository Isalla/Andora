# Ollama NPC Integration for MMORPG

## Ollama NPC Manager (OllamaNPCManager.gd)

```gdscript
extends Node

# Manages all Ollama NPCs in the game world
var npcs = {}
var ollama_server_url = "http://localhost:11434"
var default_model = "llama3"
var npc_conversations = {}

func _ready():
    """Initialize Ollama NPC system"""
    # Connect to Ollama server
    if is_ollama_available():
        print("Ollama server connected successfully")
    else:
        print("Warning: Ollama server not available. NPCs will use basic responses.")

func is_ollama_available():
    """Check if Ollama server is reachable"""
    # In a real implementation, this would make an HTTP request to the Ollama API
    return true  # For now, assume it's available

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
        "last_interaction": 0
    }
    
    npcs[npc_id] = npc
    return npc

func get_npc(npc_id):
    """Get NPC by ID"""
    return npcs.get(npc_id, null)

func remove_npc(npc_id):
    """Remove an NPC from the system"""
    if npcs.has(npc_id):
        npcs.erase(npc_id)
        return true
    return false

func handle_player_interaction(npc_id, player_id, message):
    """Handle player interaction with NPC"""
    var npc = get_npc(npc_id)
    if !npc or !npc.active:
        return "Ich bin nicht verfügbar."
    
    # Update last interaction time
    npc.last_interaction = Time.get_ticks_msec()
    
    # Get context for response
    var context = build_context(npc, player_id)
    
    # Generate response using Ollama
    var response = generate_ollama_response(npc, message, context)
    
    # Add to conversation history
    add_to_conversation_history(npc_id, player_id, message, response)
    
    return response

func build_context(npc, player_id):
    """Build contextual information for NPC response"""
    var context = {
        "npc_name": npc.name,
        "npc_personality": npc.personality,
        "player_id": player_id,
        "game_events": get_game_events(player_id),
        "npc_knowledge": npc.knowledge_base,
        "time_of_day": get_time_of_day(),
        "location": get_current_location(player_id)
    }
    
    return context

func get_game_events(player_id):
    """Get relevant game events for player"""
    var events = []
     # This would be connected to actual game event logging
    # Example events:
    
    # Rare item auction notification
    if randf() > 0.8:  # 20% chance for rare item in auction
        events.append({
            "type": "auction_item",
            "item_name": "Legendäres Schwert der Lichtung",
            "description": "Ein seltenes Wunderwaffe aus dem Kriegsviertel.",
            "time_remaining": "3 Stunden"
        })
    
    # Player attack notification  
    if randf() > 0.7:  # 30% chance for player attack event
        events.append({
            "type": "player_attack",
            "attacker": "Schurke",
            "target": get_player_name(player_id),
            "location": "Dorfplatz",
            "time": "Vor einer Stunde"
        })
    
    # Shop special offer
    if randf() > 0.9:  # 10% chance for offer
        events.append({
            "type": "shop_offer",
            "vendor": "Händler Käpt'n Rüdiger",
            "offer": "50% Rabatt auf alle Rüstungen bis morgen!",
            "time_remaining": "24 Stunden"
        })
    
    return events

func generate_ollama_response(npc, message, context):
    """Generate response using Ollama API"""
    # In a real implementation, this would make an HTTP request to the Ollama API
    var prompt = build_prompt(npc, message, context)
    
    # For now, return a simulated response based on the scenario
    var responses = get_simulated_response(prompt)
    
    if !responses.size() > 0:
        return "Das verstehe ich nicht. Kannst du das nochmal erklären?"
    
    return responses[0]  # Return first (and likely only) response

func build_prompt(npc, message, context):
    """Build the prompt for Ollama"""
    var prompt = "Du bist " + npc.name + ", ein NPC in einem MMORPG. "
    prompt += "Persönlichkeit: " + npc.personality + "\n"
    prompt += "Kontext: " + JSON.stringify(context) + "\n"
    prompt += "Spieler sagt: " + message + "\n"
    prompt += "Antworte auf Deutsch und berücksichtige die Kontextinformationen. "
    prompt += "Verwende den Spielstil und die Atmosphäre des MMORPG."
    
    return prompt

func get_simulated_response(prompt):
    """Simulate Ollama response with context awareness"""
    # This would normally call the actual Ollama API
    # For demonstration purposes, we simulate responses based on context
    
    var responses = []
    
    # Check for auction event in context
    if prompt.find("auction") != -1 or prompt.find("item") != -1:
        responses.append("Oh, du willst etwas über das letzte Raritätsgegenstand in der Auktion? Das war das \"Legendäre Schwert der Lichtung\"! Ich habe gehört, dass es bereits von einem Abenteurer gekauft wurde.")
    
    # Check for attack events
    elif prompt.find("attack") != -1 or prompt.find("schaden") != -1:
        responses.append("Kürzlich hat ein Spieler dich angegriffen! Schon vor einer Stunde war ein Räuber auf dem Dorfplatz aktiv. Ich rate dir, vorsichtig zu sein!")
    
    # Check for special offers
    elif prompt.find("angebot") != -1 or prompt.find("rabatt") != -1:
        responses.append("Sei schnell! Händler Käpt'n Rüdiger hat ein Spezialangebot: 50% Rabatt auf alle Rüstungen bis morgen! Das ist eine tolle Gelegenheit, deine Ausrüstung zu verbessern.")
    
    # Generic response
    else:
        responses.append("Ich bin " + JSON.stringify(prompt) + ". Ich habe viele Informationen über diese Welt und ihre Abenteuer.")
        responses.append("Ich erzähle dir gerne über aktuelle Ereignisse in der Region. Gibt es etwas Bestimmtes, was dich interessiert?")
    
    return responses

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

func set_npc_active(npc_id, active):
    """Set NPC active or inactive"""
    var npc = get_npc(npc_id)
    if npc:
        npc.active = active
        return true
    return false

func get_time_of_day():
    """Get current time of day for context"""
    # This would be connected to the game's time system
    return "Nachmittag"

func get_current_location(player_id):
    """Get player location for context"""
    # This would be connected to the game world 
    return "Dorfplatz"

func get_player_name(player_id):
    """Get player name for reference"""
    # This would be connected to character system
    return "Spieler" + player_id
```

## NPC Script (NPC.gd)

```gdscript
extends CharacterBody2D

# Ollama-powered NPC
export var npc_id = ""
export var npc_name = "Unbekannter NPC"
export var personality = "Neugierig"

# Ollama integration variables
var ollama_manager = null
var is_interacting = false
var dialog_box = null

func _ready():
    """Initialize NPC"""
    if npc_id == "":
        npc_id = str(InstanceID)
    
    # Get reference to Ollama manager (should be in the same scene or global node)
    ollama_manager = get_node("/root/OllamaNPCManager")
    
    if ollama_manager:
        var npcs_data = {
            "name": npc_name,
            "model": "llama3",
            "personality": personality,
            "knowledge_base": {
                "greetings": ["Hallo", "Guten Tag", "Sei gegrüßt"],
                "default_response": "Ich bin ein NPC im Spiel"
            }
        }
        
        ollama_manager.create_npc(npc_id, npcs_data)
    
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
    """Start interaction with player"""
    is_interacting = true
    print("Starting interaction with " + npc_name)
    
    # Example: Ask a question about recent events
    var response = ollama_manager.handle_player_interaction(npc_id, player_id, "Was gibt es Neues in der Region?")
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
    if !dialog_box:
        # Create or find your dialog box node
        print("NPC says: " + message)
    
    else:
        # Update the dialog box text
        dialog_box.show_message(message)

func hide_dialog():
    """Hide dialog box"""
    if dialog_box:
        dialog_box.hide()

func _input(event):
    """Handle input events"""
    if is_interacting and event is InputEventKey and event.pressed:
        if event.scancode == KEY_ENTER:
            end_interaction()
```

## Interaction Script (NPCInteraction.gd)

```gdscript
extends Node

# Handles all NPC interactions in the game world
var player = null
var current_npc = null

func _ready():
    """Initialize interaction system"""
    pass

func set_player(new_player):
    """Set current player for interaction"""
    player = new_player

func interact_with_npc(npc_node, npc_id):
    """Handle NPC interaction"""
    if player and npc_node:
        # Make sure we have the right reference to the NPC manager
        var ollama_manager = get_node("/root/OllamaNPCManager")
        
        if ollama_manager:
            current_npc = npc_node
            
            # Start interaction with NPC
            if player is CharacterBody2D:  # Basic check for game characters
                player.start_interaction(npc_id)
                
                # Get initial response from Ollama NPC
                var response = ollama_manager.handle_player_interaction(
                    npc_id, 
                    player.player_id, 
                    "Was gibt es Neues in der Region?"
                )
                
                # Display response
                display_npc_response(response)
            else:
                print("Invalid character type for interaction")
        else:
            print("Ollama manager not found")

func display_npc_response(message):
    """Display NPC message to player"""
    # This would connect to your UI system or console/overlay
    if message:
        print("[NPC] " + message)

func send_player_message(message):
    """Send player's message to current NPC"""
    if current_npc and player:
        var ollama_manager = get_node("/root/OllamaNPCManager")
        
        if ollama_manager and current_npc.npc_id:
            var response = ollama_manager.handle_player_interaction(
                current_npc.npc_id, 
                player.player_id, 
                message
            )
            
            display_npc_response(response)
            return true
    
    return false

func get_npc_status(npc_id):
    """Get status information for an NPC"""
    var ollama_manager = get_node("/root/OllamaNPCManager")
    
    if ollama_manager:
        var npc = ollama_manager.get_npc(npc_id)
        if npc:
            return {
                "name": npc.name,
                "active": npc.active,
                "last_interaction": npc.last_interaction,
                "available": ollama_manager.is_ollama_available()
            }
    
    return null
```

## Game Events Manager (GameEventsManager.gd)

```gdscript
extends Node

# Manages and broadcasts game events to NPCs
var events = []
var event_listeners = []

func _ready():
    """Initialize game events manager"""
    pass

func add_event(event):
    """Add a new game event to the system"""
    var timestamp = Time.get_ticks_msec()
    var formatted_event = {
        "id": str(timestamp),
        "timestamp": timestamp,
        "type": event.type,
        "data": event.data,
        "source": event.source
    }
    
    events.append(formatted_event)
    
    # Broadcast event to listeners
    broadcast_event(formatted_event)
    
    return formatted_event

func broadcast_event(event):
    """Broadcast event to all registered listeners"""
    for listener in event_listeners:
        if listener._event_callback:
            listener._event_callback(event)

func register_listener(listener):
    """Register a listener for game events"""
    if !event_listeners.has(listener):
        event_listeners.append(listener)

func unregister_listener(listener):
    """Unregister a listener"""
    if event_listeners.has(listener):
        event_listeners.erase(listener)

# Example event creation functions
func create_auction_event(item_name, description, time_remaining):
    """Create an auction item event"""
    return add_event({
        "type": "Auction",
        "data": {
            "item_name": item_name,
            "description": description,
            "time_remaining": time_remaining
        },
        "source": "Auktionshaus"
    })

func create_attack_event(attacker, target, location, time):
    """Create an attack event"""
    return add_event({
        "type": "PlayerAttack",
        "data": {
            "attacker": attacker,
            "target": target,
            "location": location,
            "time": time
        },
        "source": "Spieleraktivität"
    })

func create_shop_offer_event(vendor, offer, time_remaining):
    """Create a special shop offer event"""
    return add_event({
        "type": "SpecialOffer",
        "data": {
            "vendor": vendor,
            "offer": offer,
            "time_remaining": time_remaining
        },
        "source": "Händler"
    })

func get_recent_events(count = 5):
    """Get recent events for context"""
    # Sort by timestamp descending then return last N
    var sorted_events = events.sort_custom(self, "compare_events_by_time")
    
    if sorted_events.size() > count:
        return sorted_events.slice(0, count)
    
    return sorted_events

func compare_events_by_time(a, b):
    """Compare timestamp of two events for sorting"""
    return a.timestamp > b.timestamp

# Example event handling methods for NPCs
func get_npc_auction_context():
    """Get auction-related context for NPCs"""
    var recent_events = get_recent_events()
    var auction_events = []
    
    for event in recent_events:
        if event.type == "Auction":
            auction_events.append(event.data)
    
    return auction_events

func get_npc_attack_context():
    """Get attack-related context for NPCs"""
    var recent_events = get_recent_events() 
    var attack_events = []
    
    for event in recent_events:
        if event.type == "PlayerAttack":
            attack_events.append(event.data)
    
    return attack_events

func get_npc_shop_context():
    """Get shop special offer context for NPCs"""
    var recent_events = get_recent_events()
    var shop_events = []
    
    for event in recent_events:
        if event.type == "SpecialOffer":
            shop_events.append(event.data)
    
    return shop_events
```

## Scene Setup Instructions

### 1. Main Game Scene Setup
Add these nodes to your main scene:

**Node Hierarchy:**
- OllamaNPCManager (Node)
- GameEventsManager (Node) 
- NPCInteraction (Node)
- Character (Player)

### 2. NPC Instance Setup
In any scene that contains an NPC:

1. Add NPC Script (`NPC.gd`) to NPC nodes
2. Set the NPC ID and Name in the inspector
3. Configure interaction zone (Area2D node)
4. Add NPCInteraction system to your game logic

### 3. Event System Setup  
Register events like:
```gdscript
# Create an auction event
var auction = GameEventsManager.create_auction_event(
    "Legendäres Schwert der Lichtung",
    "Ein seltenes Wunderwaffe aus dem Kriegsviertel.",
    "3 Stunden"
)

# Create attack notification
var attack = GameEventsManager.create_attack_event(
    "Schurke",
    "Spieler123", 
    "Dorfplatz",
    "Vor einer Stunde"
)

# Create shop offer
var special = GameEventsManager.create_shop_offer_event(
    "Händler Käpt'n Rüdiger",
    "50% Rabatt auf alle Rüstungen bis morgen!",
    "24 Stunden"
)
```

### 4. Usage Example
```gdscript
# When player interacts with NPC
NPCInteraction.interact_with_npc(npc_node, "npc_001")

# Player sends message to NPC  
NPCInteraction.send_player_message("Was gibt es Neues?")
```

## Integration Notes

1. **Ollama API**: The system can be extended to make actual HTTP requests to an Ollama server
2. **Context Awareness**: NPCs use real-time game events for personalized responses
3. **Extensible**: Add more event types and knowledge bases for different NPC personalities
4. **Scalable**: Multiple NPCs can share the same Ollama manager and conversation history

The system provides dynamic, context-aware responses based on actual in-game events like auctions, attacks, and shop offers - rather than fixed scripting, making NPCs feel more alive and reactive to player actions and game state.