class_name UpdateDialog
extends CanvasLayer
## Modal dialog for app updates.
##
## Shows when UpdateChecker detects a newer version on GitHub Releases.
## Downloads the platform asset and performs a self-update by writing
## a platform-specific updater script that swaps the files after exit.

var _overlay: ColorRect
var _card: PanelContainer
var _title_label: Label
var _version_label: Label
var _notes_label: RichTextLabel
var _progress_bar: ProgressBar
var _progress_label: Label
var _download_button: Button
var _later_button: Button
var _button_container: HBoxContainer
var _progress_container: VBoxContainer

var _tween: Tween

var _download_url := ""
var _asset_size := 0
var _new_version := ""
var _http_download: HTTPRequest = null
var _download_path := ""

const FADE_DURATION := AppTheme.ANIM_MICRO


func _ready() -> void:
	layer = AppTheme.Z_INDEX_MODAL
	visible = false
	_build_ui()

	UpdateChecker.update_available.connect(_on_update_available)


func _build_ui() -> void:
	# Modal overlay
	_overlay = ColorRect.new()
	_overlay.name = "Overlay"
	_overlay.color = AppTheme.with_alpha(AppTheme.BG_BASE, 0.0)
	_overlay.set_anchors_preset(Control.PRESET_FULL_RECT)
	_overlay.mouse_filter = Control.MOUSE_FILTER_STOP
	add_child(_overlay)

	var center := CenterContainer.new()
	center.name = "CenterContainer"
	center.set_anchors_preset(Control.PRESET_FULL_RECT)
	center.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_overlay.add_child(center)

	# Card
	_card = PanelContainer.new()
	_card.name = "Card"
	_card.custom_minimum_size.x = AppTheme.DIALOG_WIDTH
	_card.modulate.a = 0.0
	_card.theme_type_variation = "PanelModal"
	center.add_child(_card)

	var margin := MarginContainer.new()
	margin.name = "Margin"
	margin.theme_type_variation = "MarginPanel"
	_card.add_child(margin)

	var vbox := VBoxContainer.new()
	vbox.name = "Content"
	vbox.theme_type_variation = "VBoxMD"
	margin.add_child(vbox)

	# Title
	_title_label = Label.new()
	_title_label.name = "Title"
	_title_label.text = "Update Available"
	_title_label.theme_type_variation = "LabelTitleBold"
	vbox.add_child(_title_label)

	# Version info
	_version_label = Label.new()
	_version_label.name = "Version"
	_version_label.theme_type_variation = "LabelTitleMuted"
	vbox.add_child(_version_label)

	# Release notes
	_notes_label = RichTextLabel.new()
	_notes_label.name = "Notes"
	_notes_label.bbcode_enabled = true
	_notes_label.fit_content = true
	_notes_label.scroll_active = true
	_notes_label.custom_minimum_size.y = 120
	_notes_label.custom_minimum_size.x = 0
	_notes_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vbox.add_child(_notes_label)

	# Progress section (hidden initially)
	_progress_container = VBoxContainer.new()
	_progress_container.name = "ProgressContainer"
	_progress_container.visible = false
	_progress_container.theme_type_variation = "VBoxXS"
	vbox.add_child(_progress_container)

	_progress_bar = ProgressBar.new()
	_progress_bar.name = "ProgressBar"
	_progress_bar.custom_minimum_size.y = 8
	_progress_container.add_child(_progress_bar)

	_progress_label = Label.new()
	_progress_label.name = "ProgressLabel"
	_progress_label.text = "Downloading..."
	_progress_label.theme_type_variation = "LabelSmallDim"
	_progress_container.add_child(_progress_label)

	# Spacer
	var spacer := Control.new()
	spacer.custom_minimum_size.y = AppTheme.SPACING_SM
	vbox.add_child(spacer)

	# Buttons
	_button_container = HBoxContainer.new()
	_button_container.name = "Buttons"
	_button_container.alignment = BoxContainer.ALIGNMENT_END
	_button_container.theme_type_variation = "HBoxSM"
	vbox.add_child(_button_container)

	_later_button = Button.new()
	_later_button.text = "Later"
	_later_button.pressed.connect(_on_later_pressed)
	_button_container.add_child(_later_button)

	_download_button = Button.new()
	_download_button.text = "Download & Install"
	_download_button.pressed.connect(_on_download_pressed)
	_button_container.add_child(_download_button)


func _on_update_available(version: String, release_notes: String, download_url: String, asset_size: int) -> void:
	_new_version = version
	_download_url = download_url
	_asset_size = asset_size

	_version_label.text = "v%s is available (you have v%s)" % [version, Version.CURRENT]
	_notes_label.text = release_notes if not release_notes.is_empty() else "No release notes."

	_progress_container.visible = false
	_download_button.visible = true
	_download_button.disabled = false
	_later_button.visible = true

	_show_animated()


func _on_download_pressed() -> void:
	_download_button.disabled = true
	_later_button.visible = false
	_progress_container.visible = true
	_progress_bar.value = 0
	_progress_label.text = "Downloading..."

	# Download to temp location
	var asset_name := "OpenISI-update" + _get_archive_extension()
	_download_path = OS.get_user_data_dir() + "/" + asset_name

	_http_download = HTTPRequest.new()
	_http_download.download_file = _download_path
	add_child(_http_download)
	_http_download.request_completed.connect(_on_download_completed)

	var headers := PackedStringArray(["User-Agent: OpenISI/%s" % Version.CURRENT])
	var err := _http_download.request(_download_url, headers)
	if err != OK:
		_progress_label.text = "Download failed (error %d)" % err
		_download_button.disabled = false
		_later_button.visible = true
		_cleanup_download()


func _process(_delta: float) -> void:
	if _http_download != null and _asset_size > 0:
		var downloaded := _http_download.get_downloaded_bytes()
		var progress := float(downloaded) / float(_asset_size) * 100.0
		_progress_bar.value = progress
		_progress_label.text = "Downloading... %.0f%% (%s / %s)" % [
			progress,
			_format_bytes(downloaded),
			_format_bytes(_asset_size),
		]


func _on_download_completed(result: int, response_code: int, _headers: PackedStringArray, _body: PackedByteArray) -> void:
	_cleanup_download()

	if result != HTTPRequest.RESULT_SUCCESS or response_code != 200:
		_progress_label.text = "Download failed (HTTP %d)" % response_code
		_download_button.disabled = false
		_later_button.visible = true
		return

	_progress_bar.value = 100
	_progress_label.text = "Download complete. Installing..."

	# Write updater script and launch it
	_perform_self_update()


func _perform_self_update() -> void:
	var exe_path := OS.get_executable_path()
	var pid := OS.get_process_id()

	match OS.get_name():
		"macOS":
			_self_update_macos(exe_path, pid)
		"Windows":
			_self_update_windows(exe_path, pid)
		"Linux":
			_self_update_linux(exe_path, pid)


func _self_update_macos(exe_path: String, pid: int) -> void:
	# exe_path is inside OpenISI.app/Contents/MacOS/OpenISI
	var app_path := exe_path.get_base_dir().get_base_dir().get_base_dir()
	var app_parent := app_path.get_base_dir()
	var app_name := app_path.get_file()
	var temp_dir := OS.get_user_data_dir() + "/update_temp"
	var script_path := OS.get_user_data_dir() + "/update.sh"

	var script := "#!/bin/bash\n"
	script += "# OpenISI self-updater\n"
	script += "while kill -0 %d 2>/dev/null; do sleep 0.5; done\n" % pid
	script += "rm -rf '%s'\n" % temp_dir
	script += "mkdir -p '%s'\n" % temp_dir
	script += "unzip -q '%s' -d '%s'\n" % [_download_path, temp_dir]
	script += "rm -rf '%s/%s'\n" % [app_parent, app_name]
	script += "mv '%s'/*.app '%s/%s'\n" % [temp_dir, app_parent, app_name]
	script += "rm -rf '%s' '%s'\n" % [temp_dir, _download_path]
	script += "open '%s/%s'\n" % [app_parent, app_name]
	script += "rm -- '$0'\n"

	_write_and_run_script(script_path, script)
	get_tree().quit()


func _self_update_windows(exe_path: String, pid: int) -> void:
	var app_dir := exe_path.get_base_dir()
	var temp_dir := OS.get_user_data_dir() + "\\update_temp"
	var script_path := OS.get_user_data_dir() + "\\update.bat"

	var script := "@echo off\r\n"
	script += "REM OpenISI self-updater\r\n"
	script += ":wait\r\n"
	script += "tasklist /FI \"PID eq %d\" 2>NUL | find /I \"%d\" >NUL\r\n" % [pid, pid]
	script += "if not errorlevel 1 (timeout /t 1 /nobreak >NUL & goto wait)\r\n"
	script += "if exist \"%s\" rmdir /s /q \"%s\"\r\n" % [temp_dir, temp_dir]
	script += "mkdir \"%s\"\r\n" % temp_dir
	script += "powershell -Command \"Expand-Archive -Path '%s' -DestinationPath '%s' -Force\"\r\n" % [_download_path, temp_dir]
	script += "xcopy /s /e /y \"%s\\*\" \"%s\\\"\r\n" % [temp_dir, app_dir]
	script += "rmdir /s /q \"%s\"\r\n" % temp_dir
	script += "del \"%s\"\r\n" % _download_path
	script += "start \"\" \"%s\"\r\n" % exe_path
	script += "del \"%%~f0\"\r\n"

	_write_and_run_script(script_path, script)
	get_tree().quit()


func _self_update_linux(exe_path: String, pid: int) -> void:
	var app_dir := exe_path.get_base_dir()
	var temp_dir := OS.get_user_data_dir() + "/update_temp"
	var script_path := OS.get_user_data_dir() + "/update.sh"

	var script := "#!/bin/bash\n"
	script += "# OpenISI self-updater\n"
	script += "while kill -0 %d 2>/dev/null; do sleep 0.5; done\n" % pid
	script += "rm -rf '%s'\n" % temp_dir
	script += "mkdir -p '%s'\n" % temp_dir
	script += "tar xzf '%s' -C '%s'\n" % [_download_path, temp_dir]
	script += "cp -rf '%s'/* '%s/'\n" % [temp_dir, app_dir]
	script += "chmod +x '%s'\n" % exe_path
	script += "rm -rf '%s' '%s'\n" % [temp_dir, _download_path]
	script += "'%s' &\n" % exe_path
	script += "rm -- '$0'\n"

	_write_and_run_script(script_path, script)
	get_tree().quit()


func _write_and_run_script(script_path: String, content: String) -> void:
	var file := FileAccess.open(script_path, FileAccess.WRITE)
	if file == null:
		_progress_label.text = "Failed to write updater script"
		return
	file.store_string(content)
	file.close()

	if OS.get_name() == "Windows":
		OS.create_process("cmd.exe", ["/c", "start", "/b", script_path], false)
	else:
		OS.execute("chmod", ["+x", script_path])
		OS.create_process("/bin/bash", [script_path], false)


func _on_later_pressed() -> void:
	_hide_animated()


func _show_animated() -> void:
	visible = true
	if _tween:
		_tween.kill()
	_tween = create_tween()
	_tween.set_parallel(true)
	_tween.tween_property(_overlay, "color:a", AppTheme.SHADOW_ALPHA_MODAL, FADE_DURATION)
	_tween.tween_property(_card, "modulate:a", 1.0, FADE_DURATION)


func _hide_animated() -> void:
	if _tween:
		_tween.kill()
	_tween = create_tween()
	_tween.set_parallel(true)
	_tween.tween_property(_overlay, "color:a", 0.0, FADE_DURATION)
	_tween.tween_property(_card, "modulate:a", 0.0, FADE_DURATION)
	_tween.chain().tween_callback(func(): visible = false)


func _get_archive_extension() -> String:
	match OS.get_name():
		"Linux":
			return ".tar.gz"
		_:
			return ".zip"


func _format_bytes(bytes: int) -> String:
	if bytes < 1024:
		return "%d B" % bytes
	elif bytes < 1048576:
		return "%.1f KB" % (float(bytes) / 1024.0)
	else:
		return "%.1f MB" % (float(bytes) / 1048576.0)


func _cleanup_download() -> void:
	if _http_download != null:
		_http_download.queue_free()
		_http_download = null


func _input(event: InputEvent) -> void:
	if not visible:
		return
	if event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE:
		if not _progress_container.visible:
			_on_later_pressed()
			get_viewport().set_input_as_handled()
