## FormatUtils - Common formatting utilities
class_name FormatUtils
extends RefCounted

## Format a number with thousands separators (e.g., 1,234,567)
static func format_number(n: int) -> String:
	var s := str(abs(n))
	var result := ""
	var count := 0
	for i in range(s.length() - 1, -1, -1):
		if count > 0 and count % 3 == 0:
			result = "," + result
		result = s[i] + result
		count += 1
	return ("-" if n < 0 else "") + result

## Format duration in seconds to "M:SS" or "H:MM:SS" format
static func format_duration(seconds: float) -> String:
	var total_sec := int(seconds)
	var hours := floori(total_sec / 3600.0)
	var minutes := floori((total_sec % 3600) / 60.0)
	var secs := total_sec % 60
	if hours > 0:
		return "%d:%02d:%02d" % [hours, minutes, secs]
	return "%d:%02d" % [minutes, secs]

## Format bytes to human-readable size (KB, MB, GB)
static func format_bytes(bytes: int) -> String:
	if bytes < 1024:
		return "%d B" % bytes
	elif bytes < 1024 * 1024:
		return "%.1f KB" % (bytes / 1024.0)
	elif bytes < 1024 * 1024 * 1024:
		return "%.1f MB" % (bytes / (1024.0 * 1024.0))
	else:
		return "%.2f GB" % (bytes / (1024.0 * 1024.0 * 1024.0))

## Format a float with specified decimal places
static func format_float(value: float, decimals: int = 1) -> String:
	return ("%." + str(decimals) + "f") % value
