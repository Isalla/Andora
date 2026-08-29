extends Node

# Demo scene showing how to use the OllamaClient

@onready var ollama_client = $OllamaClient
@onready var text_edit = $Panel/TextEdit
@onready var input_field = $Panel/InputField
@onready var send_button = $Panel/SendButton

func _ready():
	# Connect the button signal
	send_button.pressed.connect(_on_send_button_pressed)

func _on_send_button_pressed():
	var prompt = input_field.text
	if prompt.is_empty():
		return
	
	# Clear previous text
	text_edit.text = "Sending prompt to Ollama...\n"
	
	# Send the prompt using Ollama client
	ollama_client.send_prompt(prompt, Callable(self, "_on_ollama_response"))

func _on_ollama_response(response: String):
	text_edit.text += "Response:\n" + response

func _input(event):
	if event is InputEventKey and event.pressed:
		if event.keycode == KEY_ENTER:
			_on_send_button_pressed()