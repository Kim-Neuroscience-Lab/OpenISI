class_name FileUtils
extends RefCounted
## FileUtils - Common file loading utilities.
##
## This file is the Single Source of Truth (SSoT) for JSON file loading patterns.
## Config files and other code should use FileUtils.load_json() instead of
## duplicating the same file-loading boilerplate.
##
## Usage: var data := FileUtils.load_json("res://config/hardware.json")


## Load and parse a JSON file, returns empty dict on failure.
## Handles file existence check, file access, JSON parsing, and error reporting.
##
## @param path: The file path to load (res:// or user:// paths)
## @param silent: If true, suppresses error messages for missing files (useful for optional user configs)
## @return: The parsed Dictionary, or empty dict on failure
static func load_json(path: String, silent: bool = false) -> Dictionary:
	if not FileAccess.file_exists(path):
		if not silent:
			push_error("FileUtils: File not found: %s" % path)
		return {}

	var file := FileAccess.open(path, FileAccess.READ)
	if not file:
		push_error("FileUtils: Failed to open file: %s (error: %s)" % [path, FileAccess.get_open_error()])
		return {}

	var content := file.get_as_text()
	file.close()

	var json := JSON.new()
	var error := json.parse(content)
	if error != OK:
		push_error("FileUtils: Failed to parse JSON in %s (line %d): %s" % [
			path, json.get_error_line(), json.get_error_message()
		])
		return {}

	var data = json.get_data()
	if data is Dictionary:
		return data
	else:
		push_error("FileUtils: JSON root is not a Dictionary in %s" % path)
		return {}


## Save a Dictionary as JSON to a file.
##
## @param path: The file path to save to
## @param data: The Dictionary to save
## @param indent: Indentation string (default: tab for readability)
## @return: True if save succeeded, false otherwise
static func save_json(path: String, data: Dictionary, indent: String = "\t") -> bool:
	var file := FileAccess.open(path, FileAccess.WRITE)
	if not file:
		push_error("FileUtils: Failed to open file for writing: %s (error: %s)" % [
			path, FileAccess.get_open_error()
		])
		return false

	file.store_string(JSON.stringify(data, indent))
	file.close()
	return true
