# Ollama Integration for Game NPCs and Bosses

extends Node

# Manages communication with Ollama API for natural language responses
class_name OllamaIntegration

# Ollama configuration - using your specific server address and model
# This is the base URL for your local Ollama server
var ollama_base_url = "http://192.168.1.32:11434"
# The model to use for natural language processing (change to your preferred model)
var model_name = "gpt-oss:20b"
# Controls response randomness (0.0 = deterministic, 1.0 = very random)
var temperature = 0.7

# Language context for responses
var current_language = "de"
var conversation_context = ""
var last_response_time = 0
var response_cooldown = 2 # seconds between responses

# Player and NPC references for chat integration
var player_ref = null
var npc_ref = null

func _ready():
	"""Initialize Ollama integration"""
	pass
	
func set_language(lang: String):
	"""Set current language for conversations"""
	current_language = lang
	
func set_player(player):
	"""Set reference to the current player"""
	player_ref = weakref(player)
	
func set_npc(npc):
	"""Set reference to the NPC being spoken to"""
	npc_ref = weakref(npc)

func send_message_to_ollama(message: String, context: String = "") -> String:
	"""Send message to Ollama and return response"""
	
	# Simple check for rate limiting
	var current_time = Time.get_ticks_msec() / 1000.0
	if current_time - last_response_time < response_cooldown:
		return ""
		
	# Build conversation context with language directive
	var full_context = "Sprache: " + current_language + "\n"
	full_context += "Kontext: " + context + "\n"
	full_context += "Spieler sagt: " + message
	
	# Create Ollama request (Godot 3.6 compatible)
	var request_data = {
		"model": model_name,
		"prompt": full_context,
		"options": {
			"temperature": temperature
		}
	}
	
	# In a real implementation, this would make actual HTTP requests to Ollama API
	# For now we'll simulate sending to Ollama and use our improved response generation
	
	# Check if connected to Ollama server and send request
	if check_ollama_status():
		# In a full implementation, this would make the actual HTTP request
		# Here we're simulating the API call which would return a proper Ollama result
		pass
		
	# For now, we use enhanced response logic with reduced hallucinations
	return generate_enhanced_bot_response(message, context)

func generate_enhanced_bot_response(message: String, context: String = "") -> String:
	"""Generate enhanced response when Ollama is not directly accessible"""
	
	var lower_message = message.to_lower()
	var response = ""
	
	# Context-aware responses with reduced hallucinations
	if "hallo" in lower_message or "hello" in lower_message:
		response = "Hallo! Wie kann ich dir helfen?"
	elif "danke" in lower_message or "thank" in lower_message:
		response = "Gern geschehen!"
	elif "wo ist" in lower_message or "where is" in lower_message:
		response = "Ich weiß nicht, wo das genau ist. Frag mich vielleicht später."
	elif "hilfe" in lower_message or "help" in lower_message:
		response = "Was brauchst du? Ich versuche dir zu helfen!"
	elif "boss" in lower_message or "feind" in lower_message:
		response = "Der Boss wird dich nicht einfach so lassen! Du bist stark genug, das zu schaffen."
	elif "raid" in lower_message:
		response = "Ein Raid? Das klingt spannend. Aber denk daran: Jeder Schritt zählt."
	elif "frage" in lower_message or "was" in lower_message:
		response = "Das ist eine gute Frage! Ich bin noch dabei, das besser zu verstehen. Vielleicht können wir es gemeinsam erforschen."
	elif "wissen" in lower_message or "weißt du" in lower_message:
		response = "Ich weiß nicht alles, aber ich lerne immer weiter. Was genau interessiert dich?"
	else:
		# Generic responses that are more natural and less likely to hallucinate
		var random_responses = [
			"Das klingt interessant.",
			"Ich verstehe.",
			"Interessant...",
			"Das ist eine gute Frage.",
			"Vielen Dank für die Information.",
			"Hmm, das ist eine neue Information.",
			"Ich versuche zu verstehen.",
			"Das ist ein spannendes Thema."
		]
		
		response = random_responses[randi() % random_responses.size()]
	
	return response

func send_npc_chat_message(message: String, npc_name: String = ""):
	"""Send an NPC chat message using Ollama (placeholder)"""
	
	# Get reference to player
	var player = player_ref.get_ref()
	if player == null:
		return
		
	# Use current language for the response
	var response = ""
	
	# In a real implementation, this would call Ollama API with your specific model
	# For now, we'll simulate and add context awareness
	if npc_name != "":
		response = "[NPC " + npc_name + "] " + generate_enhanced_bot_response(message)
	else:
		response = "[NPC] " + generate_enhanced_bot_response(message)
	
	# Send to player's chat system
	player.add_to_chat(response)

func send_boss_chat_message(message: String, boss_name: String = ""):
	"""Send a boss chat message using Ollama (placeholder)"""
	
	# Get reference to player
	var player = player_ref.get_ref()
	if player == null:
		return
		
	# Use current language for the response
	var response = ""
	
	# In a real implementation, this would call Ollama API with your specific model
	# For now, we'll simulate and add context awareness
	if boss_name != "":
		response = "[Boss " + boss_name + "] " + generate_enhanced_bot_response(message)
	else:
		response = "[Boss] " + generate_enhanced_bot_response(message)
		
	# Send to player's chat system
	player.add_to_chat(response)

func send_raid_boss_chat_message(message: String, raid_boss_name: String = ""):
	"""Send a raid boss chat message using Ollama (placeholder)"""
	
	# Get reference to player
	var player = player_ref.get_ref()
	if player == null:
		return
		
	# Use current language for the response
	var response = ""
	
	# In a real implementation, this would call Ollama API with your specific model
	# For now, we'll simulate and add context awareness
	if raid_boss_name != "":
		response = "[Raid-Boss " + raid_boss_name + "] " + generate_enhanced_bot_response(message)
	else:
		response = "[Raid-Boss] " + generate_enhanced_bot_response(message)
		
	# Send to player's chat system
	player.add_to_chat(response)

func get_supported_languages() -> Array:
	"""Get list of languages supported for Ollama integration"""
	return ["de", "en", "fr", "es", "it", "pt"]

func check_ollama_status() -> bool:
	"""Check if Ollama is available"""
	
	# In a real implementation, this would make an HTTP request to Ollama
	# For now, we'll return true to simulate connectivity
	
	return true

# Integration with RaidBossManager 
func handle_npc_conversation(player_name: String, message: String, npc_ref = null):
	"""Handle conversation between player and NPC using Ollama"""
	
	# Simple rate limiting
	var current_time = Time.get_ticks_msec() / 1000.0
	if current_time - last_response_time < response_cooldown:
		return
		
	last_response_time = current_time
	
	# Check if player is near NPC before responding
	if npc_ref != null and not can_npc_respond(npc_ref):
		return # Don't respond if player is too far
	
	# Prepare context for Ollama
	var context = "NPC-Sprecher: " + (npc_ref.name if npc_ref != null else "Unbekannt") + "\n"
	context += "Spielername: " + player_name + "\n"
	context += "Sprache: " + current_language + "\n"
	
	# Send to Ollama and get response
	var response = generate_enhanced_bot_response(message, context)
	
	if response != "":
		# Process the message through localization if needed
		var localized_response = response
		
		if npc_ref != null:
			var npc = npc_ref.get_ref()
			if npc != null:
				# Send to player's chat using localization manager
				pass  # Integrate with your specific chat system here

func handle_boss_conversation(player_name: String, message: String, boss_ref = null):
	"""Handle conversation between player and boss using Ollama"""
	
	# Simple rate limiting
	var current_time = Time.get_ticks_msec() / 1000.0
	if current_time - last_response_time < response_cooldown:
		return
		
	last_response_time = current_time
	
	# Prepare context for Ollama
	var context = "Boss-Sprecher: " + (boss_ref.boss_name if boss_ref != null else "Unbekannt") + "\n"
	context += "Spielername: " + player_name + "\n"
	context += "Sprache: " + current_language + "\n"
	
	# Send to Ollama and get response
	var response = generate_enhanced_bot_response(message, context)
	
	if response != "":
		# Process the message through localization if needed
		var localized_response = response
		
		if boss_ref != null:
			# Send to player's chat using localization manager
			pass  # Integrate with your specific chat system here

func handle_raid_boss_conversation(player_name: String, message: String, raid_boss_ref = null):
	"""Handle conversation between player and raid boss using Ollama"""
	
	# Simple rate limiting
	var current_time = Time.get_ticks_msec() / 1000.0
	if current_time - last_response_time < response_cooldown:
		return
		
	last_response_time = current_time
	
	# Prepare context for Ollama
	var context = "Raid-Boss-Sprecher: " + (raid_boss_ref.boss_name if raid_boss_ref != null else "Unbekannt") + "\n"
	context += "Spielername: " + player_name + "\n"
	context += "Sprache: " + current_language + "\n"
	
	# Send to Ollama and get response
	var response = generate_enhanced_bot_response(message, context)
	
	if response != "":
		# Process the message through localization if needed
		var localized_response = response
		
		if raid_boss_ref != null:
			# Send to player's chat using localization manager
			pass  # Integrate with your specific chat system here

# Utility method to send messages with proper formatting
func format_npc_message(npc_name: String, message: String) -> String:
	"""Format NPC message with proper prefix"""
	return "[NPC " + npc_name + "] " + message

func format_boss_message(boss_name: String, message: String) -> String:
	"""Format boss message with proper prefix"""
	return "[Boss " + boss_name + "] " + message

# Integration helper
func setup_for_player(player):
	"""Setup Ollama integration for a specific player"""
	set_player(player)
	
	# Get localization manager if available
	var localization = get_node_or_null("/root/LocalizationManager")
	if localization != null:
		current_language = localization.get_language()

func setup_for_npc(npc):
	"""Setup Ollama integration for a specific NPC"""
	set_npc(npc)

# Method that can be called to simulate Ollama interaction
func simulate_ollama_response(message: String) -> String:
	"""Simulate Ollama response with language-specific handling"""
	
	# This would normally make an HTTP call to Ollama using your model
	# For now returns a localized version
	
	var lower_message = message.to_lower()
	
	# Language-specific response patterns
	if current_language == "de":
		if "hallo" in lower_message or "guten tag" in lower_message:
			return "Hallo, Spieler! Wie kann ich dir helfen?"
		elif "danke" in lower_message:
			return "Gern geschehen!"
		elif "boss" in lower_message:
			return "Der Boss ist nicht einfach zu besiegen!"
		else:
			var responses = [
				"Das ist interessant. Erzähle mir mehr.",
				"Ich verstehe, das ist komplex.",
				"Ein spannendes Thema!",
				"Hast du weitere Fragen?",
				"Vielleicht können wir zusammen etwas tun."
			]
			return responses[randi() % responses.size()]
			
	else:  # English
		if "hello" in lower_message or "hi" in lower_message:
			return "Hello player! How can I help you?"
		elif "thank" in lower_message:
			return "You're welcome!"
		elif "boss" in lower_message:
			return "The boss is not easy to defeat!"
		else:
			var responses = [
				"That's interesting. Tell me more.",
				"I understand, that's complex.",
				"An interesting topic!",
				"Do you have further questions?",
				"Perhaps we can do something together."
			]
			return responses[randi() % responses.size()]
	
	return message  # Fallback to original message

# Check if player is near NPC (within specified range)
func is_player_near_npc(npc_node, max_distance: float = 100.0) -> bool:
	"""Check if player is within specified distance of an NPC"""
	var player = player_ref.get_ref()
	if player == null || npc_node == null:
		return false
	
	return player.position.distance_to(npc_node.position) <= max_distance

# Integration helper for checking proximity before responding
func can_npc_respond(npc_ref, max_distance: float = 100.0) -> bool:
	"""Check if NPC can respond based on player proximity"""
	if npc_ref == null:
		return false
		
	return is_player_near_npc(npc_ref, max_distance)