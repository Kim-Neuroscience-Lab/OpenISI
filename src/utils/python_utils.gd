class_name PythonUtils
extends RefCounted
## PythonUtils - Python environment utilities.
##
## This file is the Single Source of Truth (SSoT) for Python/daemon path resolution.
## CameraClient, HardwareManager, and other code that needs to run Python scripts
## should use PythonUtils methods instead of duplicating path resolution logic.
##
## In exported builds, the bundled daemon executable is used directly.
## In development, Python venv is used via `python -m daemon.main`.


## Check if running as an exported build (bundled daemon available).
static func is_exported() -> bool:
	return OS.has_feature("standalone")


## Get the path to the bundled daemon executable (exported builds).
## Returns empty string if not found.
static func get_daemon_executable() -> String:
	var exe_dir := OS.get_executable_path().get_base_dir()
	var daemon_name: String
	if OS.get_name() == "Windows":
		daemon_name = "openisi-daemon/openisi-daemon.exe"
	elif OS.get_name() == "macOS":
		# Universal .app contains arch-specific daemon builds
		var arch := "arm64" if Engine.get_architecture_name() == "arm64" else "x86_64"
		daemon_name = "openisi-daemon-%s/openisi-daemon" % arch
	else:
		daemon_name = "openisi-daemon/openisi-daemon"
	var path := exe_dir.path_join(daemon_name)
	if FileAccess.file_exists(path):
		return path
	return ""


## Check if the bundled daemon is available.
static func has_bundled_daemon() -> bool:
	return get_daemon_executable() != ""


## Get the path to the Python interpreter in the project's virtual environment.
## Returns platform-appropriate path (.venv/bin/python or .venv/Scripts/python.exe).
static func get_venv_python_path() -> String:
	var project_path := ProjectSettings.globalize_path("res://")
	if OS.get_name() == "Windows":
		return project_path + ".venv/Scripts/python.exe"
	else:
		return project_path + ".venv/bin/python"


## Get the path to a Python script in the project's python directory.
##
## @param script_name: The script filename (e.g., "enumerate_cameras.py")
## @return: The globalized path to the script
static func get_script_path(script_name: String) -> String:
	return ProjectSettings.globalize_path("res://python/" + script_name)


## Check if the virtual environment exists.
## Returns true if the Python interpreter file exists.
static func venv_exists() -> bool:
	return FileAccess.file_exists(get_venv_python_path())


## Check if the daemon can be run (either bundled or via venv).
static func daemon_available() -> bool:
	return has_bundled_daemon() or venv_exists()


## Get the shell executable for running Python scripts.
## Returns "cmd.exe" on Windows, "/bin/sh" on Unix.
static func get_shell_exe() -> String:
	if OS.get_name() == "Windows":
		return "cmd.exe"
	else:
		return "/bin/sh"
