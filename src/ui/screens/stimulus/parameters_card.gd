class_name ParametersCard
extends MarginContainer
## ParametersCard - Dynamic parameter controls
##
## Generates parameter UI based on currently selected stimulus components.
## Parameters are discovered from Carriers, Envelopes, and Modulations definitions.


signal parameter_changed(key: String, value: Variant)

var _card: Control = null
var _param_container: VBoxContainer = null
var _param_controls: Dictionary = {}  # param_name -> Control
var _current_params: Dictionary = {}
var _current_envelope: Envelopes.Type = Envelopes.Type.NONE
var _current_space: int = DisplayGeometry.ProjectionType.CARTESIAN


func _ready() -> void:
	_build_ui()


func _build_ui() -> void:
	_card = Card.new()
	_card.title = "Stimulus Parameters"
	add_child(_card)

	_param_container = VBoxContainer.new()
	_param_container.theme_type_variation = "VBoxMD"
	_card.get_content_slot().add_child(_param_container)


## Rebuild parameter UI based on component types
func set_component_types(carrier: Carriers.Type, envelope: Envelopes.Type, space: int, strobe: bool) -> void:
	# Clear existing controls
	for child in _param_container.get_children():
		child.queue_free()
	_param_controls.clear()
	_current_envelope = envelope
	_current_space = space

	# Collect params from all components
	var all_params: Array = []
	# For polar envelopes (wedge/ring), always use angular params regardless of space setting
	var effective_space := space
	if Envelopes.uses_polar_coordinates(envelope):
		effective_space = DisplayGeometry.ProjectionType.SPHERICAL
	all_params.append_array(Carriers.get_params_for_space(carrier, effective_space))
	all_params.append_array(Envelopes.get_params_for_space(envelope, effective_space))

	# Add strobe params if enabled (counterphase reversal)
	if strobe:
		all_params.append_array(Modulations.get_strobe_params())

	# Generate controls for each param
	for param_def in all_params:
		var row := _create_param_row(param_def)
		_param_container.add_child(row)


func _create_param_row(param_def: Dictionary) -> HBoxContainer:
	var row := HBoxContainer.new()
	row.theme_type_variation = "HBoxSM"

	var param_name: String = param_def["name"]
	var contract := Settings.lookup_param_contract(param_name)

	# Label
	var label := Label.new()
	label.text = param_def["display"]
	label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	row.add_child(label)

	# Control based on contract type
	var control: Control = null
	var param_type: int = contract["type"] if contract.has("type") else TYPE_FLOAT
	match param_type:
		TYPE_FLOAT:
			control = _create_float_control(param_name, contract)
		TYPE_INT:
			control = _create_int_control(param_name, contract)
		TYPE_BOOL:
			control = _create_bool_control(param_name)
		_:
			control = _create_float_control(param_name, contract)

	if control:
		row.add_child(control)
		_param_controls[param_name] = control

	# Unit label from contract
	if contract.has("unit"):
		var unit_label := Label.new()
		unit_label.text = contract["unit"]
		unit_label.theme_type_variation = "LabelSmallDim"
		row.add_child(unit_label)

	return row


func _create_float_control(param_name: String, contract: Dictionary) -> StyledSpinBox:
	var spinbox := StyledSpinBox.new()
	spinbox.min_value = contract["min"]
	spinbox.max_value = contract["max"]
	spinbox.step = contract["step"]
	spinbox.value = _get_param_value(param_name)
	spinbox.custom_minimum_size = Vector2(AppTheme.INPUT_SPINBOX_WIDTH, AppTheme.INPUT_HEIGHT)
	spinbox.value_changed.connect(_on_param_changed.bind(param_name))
	return spinbox


func _create_int_control(param_name: String, contract: Dictionary) -> StyledSpinBox:
	var spinbox := StyledSpinBox.new()
	spinbox.min_value = contract["min"]
	spinbox.max_value = contract["max"]
	spinbox.step = contract["step"] if contract.has("step") else 1
	spinbox.value = _get_param_value(param_name)
	spinbox.custom_minimum_size = Vector2(AppTheme.INPUT_SPINBOX_WIDTH, AppTheme.INPUT_HEIGHT)
	spinbox.value_changed.connect(_on_param_changed.bind(param_name))
	return spinbox


func _create_bool_control(param_name: String) -> CheckboxTile:
	var checkbox := CheckboxTile.new()
	checkbox.text = "Enabled"
	checkbox.button_pressed = _get_param_value(param_name)
	checkbox.toggled.connect(_on_bool_param_changed.bind(param_name))
	return checkbox


func _get_param_value(param_name: String) -> Variant:
	return _current_params[param_name]


func _on_param_changed(value: float, param_name: String) -> void:
	var final_value := value

	# For check_size_deg with polar stimuli (wedge/ring), quantize to valid values
	# that divide evenly into 360 for seamless wrapping
	if param_name == "check_size_deg" and Envelopes.uses_polar_coordinates(_current_envelope):
		final_value = Carriers.get_nearest_polar_check_size(value)
		# Update spinbox to show quantized value
		if _param_controls.has(param_name):
			var spinbox: StyledSpinBox = _param_controls[param_name] as StyledSpinBox
			if spinbox and absf(spinbox.value - final_value) > 0.01:
				spinbox.set_value_no_signal(final_value)

	_current_params[param_name] = final_value
	parameter_changed.emit(param_name, final_value)


func _on_bool_param_changed(pressed: bool, param_name: String) -> void:
	_current_params[param_name] = pressed
	parameter_changed.emit(param_name, pressed)


func _on_enum_param_changed(index: int, param_name: String, options: Variant) -> void:
	var value: Variant
	if options is Dictionary:
		var opts_dict: Dictionary = options as Dictionary
		var keys: Array = opts_dict.keys()
		if index >= 0 and index < keys.size():
			value = keys[index]
	elif options is Array:
		var opts_arr: Array = options as Array
		if index >= 0 and index < opts_arr.size():
			value = opts_arr[index]

	_current_params[param_name] = value
	parameter_changed.emit(param_name, value)


## Get all current parameter values
func get_parameters() -> Dictionary:
	var params := {}
	for param_name in _param_controls:
		var control: Control = _param_controls[param_name]
		if control is StyledSpinBox:
			params[param_name] = control.value
		elif control is CheckboxTile:
			params[param_name] = control.button_pressed
		elif control is OptionButton:
			params[param_name] = control.get_selected_id()
	return params


## Set parameter values (used when loading from protocol)
func set_parameters(params: Dictionary) -> void:
	_current_params = params.duplicate()

	# Update UI controls
	for param_name in params:
		if _param_controls.has(param_name):
			var control: Control = _param_controls[param_name]
			var value: Variant = params[param_name]

			if control is StyledSpinBox:
				control.value = value
			elif control is CheckboxTile:
				control.button_pressed = value
			elif control is OptionButton:
				# Find and select the item with matching id
				for i: int in range(control.item_count):
					if control.get_item_id(i) == value:
						control.select(i)
						break
