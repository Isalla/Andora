extends Node

# Ollama API client for Godot
# This class provides methods to interact with the Ollama API for AI model inference

@export var ollama_url: String = "http://localhost:11434"
@export var model_name: String = "llama3"

# Internal HTTP request node
var http_request: HTTPRequest

func _ready():
	# Create and setup the HTTP request node
	http_request = HTTPRequest.new()
	add_child(http_request)
	http_request.request_completed.connect(_http_request_completed)

func _process(delta):
	# Handle any pending requests or responses if needed
	pass

# Send a prompt to the Ollama API and get a response
func send_prompt(prompt: String, callback: Callable) -> Error:
	var request_data = {
		"model": model_name,
		"prompt": prompt,
		"stream": false
	}
	
	var json := JSON.new()
	var body := json.stringify(request_data)
	
	var headers: PackedStringArray = [
		"Content-Type: application/json"
	]
	
	# Store the callback as a custom property on the HTTP request node
	http_request.custom_headers = headers
	http_request.request_data = body
	http_request.callback = callback
	
	var error := http_request.request(ollama_url + "/api/generate", headers, HTTPClient.METHOD_POST, body)
	if error != OK:
		push_error("Failed to send request: " + str(error))
		return error
	
	return OK

# Internal method called when HTTP request is completed
func _http_request_completed(result: int, response_code: int, headers: PackedStringArray, body: PackedByteArray):
	if result != HTTPRequest.RESULT_SUCCESS:
		push_error("HTTP request failed with result: " + str(result))
		return
	
	# Parse the JSON response
	var json := JSON.new()
	var error = json.parse_string(body.get_string_from_utf8())
	
	if error != OK:
		push_error("Failed to parse JSON response")
		return
	
	# Access the response data
	var response_data = json.get_data()
	if not response_data.has("response"):
		push_error("No 'response' field in API response")
		return
	
	var response_text = response_data["response"]
	
	# Call the callback if it exists
	if http_request.callback is Callable:
		http_request.callback.call(response_text)