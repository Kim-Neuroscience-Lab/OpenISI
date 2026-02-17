extends Node
## UpdateChecker autoload: Checks GitHub Releases for new versions on startup.
##
## Queries the GitHub Releases API once after the main scene loads.
## Silent if up to date or if the network is unavailable (no error shown).

## Emitted when a newer version is available on GitHub Releases.
signal update_available(version: String, release_notes: String, download_url: String, asset_size: int)

var _http_request: HTTPRequest = null
var _checked := false


func _ready() -> void:
	# Check after main scene is loaded (short delay to avoid blocking startup)
	var timer := get_tree().create_timer(2.0)
	timer.timeout.connect(_check_for_update)


func _check_for_update() -> void:
	if _checked:
		return
	_checked = true

	_http_request = HTTPRequest.new()
	_http_request.timeout = 10.0
	add_child(_http_request)
	_http_request.request_completed.connect(_on_request_completed)

	var url := "https://api.github.com/repos/%s/releases/latest" % Version.REPO
	var headers := PackedStringArray(["Accept: application/vnd.github+json", "User-Agent: OpenISI/%s" % Version.CURRENT])
	var err := _http_request.request(url, headers)
	if err != OK:
		print("UpdateChecker: Failed to send request (error %d)" % err)
		_cleanup_request()


func _on_request_completed(result: int, response_code: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
	_cleanup_request()

	if result != HTTPRequest.RESULT_SUCCESS:
		print("UpdateChecker: Request failed (result %d)" % result)
		return

	if response_code != 200:
		print("UpdateChecker: GitHub API returned %d" % response_code)
		return

	var json := JSON.new()
	var parse_err := json.parse(body.get_string_from_utf8())
	if parse_err != OK:
		print("UpdateChecker: Failed to parse JSON response")
		return

	var data: Dictionary = json.data
	var tag_name: String = data.get("tag_name", "")
	if tag_name.is_empty():
		return

	# Strip leading "v" for comparison
	var remote_version := tag_name.lstrip("v")
	if not _is_newer(remote_version, Version.CURRENT):
		print("UpdateChecker: Up to date (current=%s, latest=%s)" % [Version.CURRENT, remote_version])
		return

	# Find platform-specific asset
	var asset_name := _get_platform_asset_name()
	var download_url := ""
	var asset_size := 0

	var assets: Array = data.get("assets", [])
	for asset in assets:
		if asset is Dictionary and asset.get("name", "") == asset_name:
			download_url = asset.get("browser_download_url", "")
			asset_size = int(asset.get("size", 0))
			break

	if download_url.is_empty():
		print("UpdateChecker: No asset found for platform (%s)" % asset_name)
		return

	var release_notes: String = data.get("body", "")
	print("UpdateChecker: Update available! %s -> %s" % [Version.CURRENT, remote_version])
	update_available.emit(remote_version, release_notes, download_url, asset_size)


func _is_newer(remote: String, current: String) -> bool:
	var remote_parts := remote.split(".")
	var current_parts := current.split(".")

	for i in range(maxi(remote_parts.size(), current_parts.size())):
		var r := int(remote_parts[i]) if i < remote_parts.size() else 0
		var c := int(current_parts[i]) if i < current_parts.size() else 0
		if r > c:
			return true
		if r < c:
			return false
	return false


func _get_platform_asset_name() -> String:
	match OS.get_name():
		"macOS":
			return "OpenISI-macos.zip"
		"Windows":
			return "OpenISI-windows.zip"
		"Linux":
			return "OpenISI-linux.tar.gz"
		_:
			return ""


func _cleanup_request() -> void:
	if _http_request != null:
		_http_request.queue_free()
		_http_request = null
