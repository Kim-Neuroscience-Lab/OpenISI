class_name Modulations
extends RefCounted
## Strobe parameters for texture-based stimuli.
##
## Strobe (counterphase contrast reversal) is handled via the strobe checkbox.
## The modulation concept has been removed as it was redundant with envelope types:
##   BAR envelope -> sweeps (LR, RL, TB, BT)
##   WEDGE envelope -> rotates (CW, CCW)
##   RING envelope -> expands/contracts (EXP, CON)
##   NONE envelope -> full-field static/strobing pattern


## Strobe parameter (used when strobe checkbox is enabled)
## Only specifies name and display - contracts (min/max/step/unit) come from Config (SSoT)
const STROBE_PARAMS := [
	{ "name": "strobe_frequency_hz", "display": "Strobe Frequency" },
]


## Get strobe parameter definitions (when strobe is enabled)
static func get_strobe_params() -> Array:
	return STROBE_PARAMS
