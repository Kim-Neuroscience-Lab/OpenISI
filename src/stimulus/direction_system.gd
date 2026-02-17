class_name DirectionSystem
extends RefCounted
## Unified direction handling for all stimulus types.
##
## Maps envelope types to their available directions/conditions.


enum System {
	CARTESIAN,  ## LR, RL, TB, BT (bar sweep)
	POLAR,      ## CW, CCW (wedge rotation)
	RADIAL,     ## EXP, CON (ring expand/contract)
	NONE,       ## No directions (static)
}


## Directions available for each system
const DIRECTIONS := {
	System.CARTESIAN: ["LR", "RL", "TB", "BT"],
	System.POLAR: ["CW", "CCW"],
	System.RADIAL: ["EXP", "CON"],
	System.NONE: [],
}


## Human-readable display names
const DISPLAY_NAMES := {
	"LR": "Left to Right",
	"RL": "Right to Left",
	"TB": "Top to Bottom",
	"BT": "Bottom to Top",
	"CW": "Clockwise",
	"CCW": "Counter-clockwise",
	"EXP": "Expand",
	"CON": "Contract",
	"FULL_FIELD": "Full Field",
}


## Short display names for compact UI
const SHORT_NAMES := {
	"LR": "LR",
	"RL": "RL",
	"TB": "TB",
	"BT": "BT",
	"CW": "CW",
	"CCW": "CCW",
	"EXP": "EXP",
	"CON": "CON",
	"FULL_FIELD": "Full",
}


## Get directions for a system
static func get_directions(system: System) -> Array[String]:
	var dirs: Array[String] = []
	dirs.assign(DIRECTIONS[system])
	return dirs


## Check if a direction is valid for a system
static func is_valid(system: System, direction: String) -> bool:
	return direction in DIRECTIONS[system]


## Get display name for a direction
static func get_display_name(direction: String) -> String:
	return DISPLAY_NAMES[direction]


## Get short name for a direction (for compact UI)
static func get_short_name(direction: String) -> String:
	return SHORT_NAMES[direction]


## Get the direction system for an envelope type
## Envelope determines available directions (bar=cartesian, wedge=polar, ring=radial)
static func get_system_for_envelope(envelope: Envelopes.Type) -> System:
	match envelope:
		Envelopes.Type.NONE:
			return System.NONE
		Envelopes.Type.BAR:
			return System.CARTESIAN
		Envelopes.Type.WEDGE:
			return System.POLAR
		Envelopes.Type.RING:
			return System.RADIAL
		_:
			return System.NONE
