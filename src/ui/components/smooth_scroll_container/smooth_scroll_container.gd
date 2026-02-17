class_name SmoothScrollContainer
extends ScrollContainer
## ScrollContainer with momentum-based smooth scrolling.
## Matches the Sleep Punk Night design system aesthetic.
##
## Uses velocity-based momentum with friction for natural deceleration.
## Supports custom scrollbar positioning via scrollbar_vertical_inset.

## Pixels to scroll per mouse wheel tick
@export var scroll_speed: float = 50.0

## Smoothing factor (lower = smoother/slower, higher = snappier)
## 0.1 = very smooth, 0.3 = moderate, 0.5 = snappy
@export var smoothing: float = 0.12

## Friction for momentum (0-1, higher = faster stop)
@export var friction: float = 0.95

## Vertical inset for scrollbar (aligns scrollbar with content margins)
## When > 0, creates a custom scrollbar as sibling and hides built-in
@export var scrollbar_vertical_inset: float = 0.0

var _target_scroll: float = 0.0
var _is_trackpad_scrolling: bool = false
var _custom_scrollbar: VScrollBar = null
var _syncing_scrollbar: bool = false  # Prevents feedback loop


func _ready() -> void:
	_target_scroll = float(scroll_vertical)

	# If inset is set, use custom scrollbar instead of built-in
	if scrollbar_vertical_inset > 0:
		vertical_scroll_mode = ScrollContainer.SCROLL_MODE_SHOW_NEVER
		_setup_custom_scrollbar()


func _setup_custom_scrollbar() -> void:
	_custom_scrollbar = VScrollBar.new()
	_custom_scrollbar.name = "CustomVScrollBar"

	# Position: right edge, inset top/bottom
	_custom_scrollbar.set_anchors_preset(Control.PRESET_RIGHT_WIDE)
	_custom_scrollbar.offset_top = scrollbar_vertical_inset
	_custom_scrollbar.offset_bottom = -scrollbar_vertical_inset
	var scrollbar_width := _custom_scrollbar.get_combined_minimum_size().x
	_custom_scrollbar.offset_left = -scrollbar_width
	_custom_scrollbar.offset_right = 0

	# Sync scrollbar value changes back to scroll container
	_custom_scrollbar.value_changed.connect(_on_custom_scrollbar_changed)

	# Add as sibling (after this node in parent)
	call_deferred("_add_scrollbar_to_parent")


func _add_scrollbar_to_parent() -> void:
	if _custom_scrollbar and get_parent():
		get_parent().add_child(_custom_scrollbar)


func _gui_input(event: InputEvent) -> void:
	# Handle mouse wheel (discrete clicks)
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.pressed:
			if mb.button_index == MOUSE_BUTTON_WHEEL_UP:
				_target_scroll -= scroll_speed
				_is_trackpad_scrolling = false
				_clamp_target()
				accept_event()
			elif mb.button_index == MOUSE_BUTTON_WHEEL_DOWN:
				_target_scroll += scroll_speed
				_is_trackpad_scrolling = false
				_clamp_target()
				accept_event()

	# Handle trackpad pan gesture (smooth continuous input)
	elif event is InputEventPanGesture:
		var pan := event as InputEventPanGesture
		_target_scroll += pan.delta.y * scroll_speed * 0.5
		_is_trackpad_scrolling = true
		_clamp_target()
		accept_event()


func _process(_delta: float) -> void:
	var current := float(scroll_vertical)

	# Smoothly interpolate toward target
	var diff := _target_scroll - current
	if abs(diff) > 0.5:
		var new_scroll := current + diff * smoothing
		scroll_vertical = int(new_scroll)
	else:
		scroll_vertical = int(_target_scroll)

	# Sync custom scrollbar with scroll state
	_sync_custom_scrollbar()


func _clamp_target() -> void:
	var max_scroll := _get_max_scroll()
	_target_scroll = clampf(_target_scroll, 0.0, max_scroll)


func _get_max_scroll() -> float:
	var vbar := get_v_scroll_bar()
	if vbar:
		return maxf(0.0, vbar.max_value - vbar.page)
	return 0.0


func _sync_custom_scrollbar() -> void:
	if not _custom_scrollbar:
		return
	var builtin := get_v_scroll_bar()
	if not builtin:
		return

	# Prevent feedback loop: setting value triggers value_changed signal
	_syncing_scrollbar = true

	# Copy range from built-in (it still tracks content size internally)
	_custom_scrollbar.min_value = builtin.min_value
	_custom_scrollbar.max_value = builtin.max_value
	_custom_scrollbar.page = builtin.page
	_custom_scrollbar.value = scroll_vertical

	_syncing_scrollbar = false

	# Show/hide based on whether scrolling is needed
	_custom_scrollbar.visible = builtin.max_value > builtin.page


func _on_custom_scrollbar_changed(value: float) -> void:
	# Ignore if we're programmatically syncing (prevents feedback loop)
	if _syncing_scrollbar:
		return
	_target_scroll = value
	scroll_vertical = int(value)


## Returns the custom scrollbar if using one, otherwise the built-in
func get_active_scrollbar() -> ScrollBar:
	if _custom_scrollbar:
		return _custom_scrollbar
	return get_v_scroll_bar()


## Returns true if using a custom scrollbar (sibling, not part of layout)
func has_custom_scrollbar() -> bool:
	return _custom_scrollbar != null
