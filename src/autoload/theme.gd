extends Node
## Theme autoload: Centralized styling definitions for the application.
##
## Builds a Godot Theme resource programmatically at startup and assigns it
## to the scene root. Components inherit styles automatically via Godot's
## theme propagation system.
##
## SSoT Architecture:
##   - Colors, spacing, radii: Constants in this file (single source)
##   - StyledButton: Uses button.gdshader for raised surface effects, reads colors from here
##   - Regular Button: Uses StyleBox from this theme (simplified, no gradient)
##   - Labels: Theme variations (LabelTitle, LabelHeading, etc.)
##   - Panels: Theme variations (PanelWell, PanelInfoCard, etc.)
##
## Sleep Punk Night design system with raised surface styling.
## See docs/specs/2026-01-19-ui_style_update.md for full specification.


# =============================================================================
# COLOR PALETTE - Foundation (The Darkness)
# =============================================================================

## Deep night with violet undertone - base of everything
const BG_BASE := Color("#13111a")

## Softer contrast for pressed/disabled amber button text
const BG_BASE_MUTED := Color("#2a2535")

## Like fleece in shadow - primary container color
const SURFACE := Color("#1c1824")

## Floating elements, modals, cards
const SURFACE_ELEVATED := Color("#241f2e")

## Soft recesses - input fields, inset areas
const WELL := Color("#0f0d14")

## Disabled button state - 1/3 toward WELL from SURFACE
const SURFACE_DISABLED := Color("#18141f")

## Pressed button state - 2/3 toward WELL from SURFACE
const SURFACE_RECESSED := Color("#131119")


# =============================================================================
# COLOR PALETTE - Cream (The Light Sources)
# =============================================================================

## Warm candlelight - primary text, key glowing elements
const CREAM := Color("#f0e6dc")

## Soft, receded - secondary text, labels
const CREAM_MUTED := Color("#c4b8ab")

## Distant warmth - placeholders, hints, disabled states
const CREAM_DIM := Color("#7a7067")


# =============================================================================
# COLOR PALETTE - Lavender (Ambient Mood)
# =============================================================================

## Soft purple glow - primary accent color
const LAVENDER := Color("#b8a5c9")

## Muted - hover states, secondary accents
const LAVENDER_DIM := Color("#8677a3")

## Rich purple - borders, slider fills, subtle accents
const LAVENDER_DEEP := Color("#6b5a8a")


# =============================================================================
# COLOR PALETTE - Amber (The Nightlight)
# =============================================================================

## Warm gold - primary buttons, important values
const AMBER := Color("#e8c49a")

## Deeper amber - gradients, warm accents
const AMBER_GLOW := Color("#d4a574")

## Brightened amber for button highlights (pre-computed)
const AMBER_BRIGHT := Color(0.976, 0.847, 0.690, 1.0)

## Disabled amber button - desaturated "light is off"
const AMBER_PALE := Color("#a89080")

## Disabled amber glow - matching muted
const AMBER_GLOW_PALE := Color("#8a7868")


# =============================================================================
# COLOR PALETTE - Status (Gentle, Not Alarming)
# =============================================================================

## Soft sage - "All is well." Connected, ready.
const SUCCESS := Color("#a8c4b0")

## Dusty rose - "Attention needed" or "Something's wrong." Not aggressive.
## Used for both warnings and errors (amber reserved for active/important UI elements)
const ERROR := Color("#c9a9a9")


# =============================================================================
# SHADER PRELOADS
# =============================================================================
## Preloaded shaders for use by UI components. Use these instead of load() calls.

const ButtonShader: Shader = preload("res://src/ui/theme/shaders/button.gdshader")
const RaisedSurfaceShader: Shader = preload("res://src/ui/theme/shaders/raised_surface.gdshader")
const InsetSurfaceShader: Shader = preload("res://src/ui/theme/shaders/inset_surface.gdshader")
const TextGlowShader: Shader = preload("res://src/ui/theme/shaders/text_glow.gdshader")
const RoundedMaskShader: Shader = preload("res://src/ui/theme/shaders/rounded_mask.gdshader")


# =============================================================================
# FONT PRELOADS
# =============================================================================
## Preloaded fonts for use by UI components. Use these instead of load() calls.

const FontRegular: Font = preload("res://assets/fonts/PlusJakartaSans-VariableFont_wght.ttf")
const FontItalic: Font = preload("res://assets/fonts/PlusJakartaSans-Italic-VariableFont_wght.ttf")
const FontMedium: Font = preload("res://assets/fonts/PlusJakartaSans-Medium.ttf")
const FontSemiBold: Font = preload("res://assets/fonts/PlusJakartaSans-SemiBold.ttf")
const FontBold: Font = preload("res://assets/fonts/PlusJakartaSans-Bold.ttf")



# =============================================================================
# RAISED SURFACE STYLING
# =============================================================================

## Surface base colors - explicit mapping of component types to base colors
const SURFACE_COLOR_CARD := SURFACE_ELEVATED         # Cards, panels, info cards
const SURFACE_COLOR_NAV := SURFACE                   # Nav bar container
const SURFACE_COLOR_STATUS := SURFACE                # Status pills, badges
const SURFACE_COLOR_BUTTON := SURFACE                # Secondary/default buttons
const SURFACE_COLOR_CHECKBOX_ON := SURFACE_ELEVATED  # Checkbox tile selected
const SURFACE_COLOR_CHECKBOX_OFF := WELL             # Checkbox tile unselected
const SURFACE_COLOR_BUTTON_HOVER := SURFACE_ELEVATED   # Button hover state
const SURFACE_COLOR_BUTTON_PRESSED := SURFACE_RECESSED # Button pressed state
const SURFACE_COLOR_BUTTON_DISABLED := SURFACE_DISABLED # Button disabled state

## Default rim highlight (Cream at 8% opacity)
const RIM_LIGHT_ALPHA := 0.08

## Elevated or focused elements (Cream at 12% opacity)
const RIM_LIGHT_STRONG_ALPHA := 0.12

## Bottom rim shadow (dark edge)
const RIM_DARK_ALPHA := 0.2

## Bottom rim subtle highlight
const RIM_BOTTOM_HIGHLIGHT_ALPHA := 0.05

## Raised gradient intensity (bidirectional: lighten top, darken bottom)
const RAISED_GRADIENT_INTENSITY := 0.03

## Button glow intensities (amber mode)
const BUTTON_GLOW_NORMAL := 0.15          ## Very subtle
const BUTTON_GLOW_HOVER := 0.25           ## More pronounced
const BUTTON_GLOW_PRESSED := 0.08         ## Pulled in
const BUTTON_GLOW_DISABLED := 0.0         ## None

## Text glow effect parameters
const TEXT_GLOW_SIZE := 20.0
const TEXT_GLOW_INTENSITY := 1.0


# =============================================================================
# SHADOW & BORDER ALPHAS
# =============================================================================

## Standard shadow alpha (cards, panels)
const SHADOW_ALPHA := 0.6

## Lighter shadow alpha (info cards)
const SHADOW_ALPHA_LIGHT := 0.5

## Subtle shadow alpha (small elements)
const SHADOW_ALPHA_SUBTLE := 0.3

## Modal shadow alpha (stronger for floating effect)
const SHADOW_ALPHA_MODAL := 0.7

## Hover state background alpha
const HOVER_ALPHA := 0.5

## Divider dark line alpha
const DIVIDER_DARK_ALPHA := 0.25

## Divider light line alpha
const DIVIDER_LIGHT_ALPHA := 0.06

## Glow effect intensity
const GLOW_INTENSITY := 0.25

## Inset border alphas (for wells, inputs, pressed states)
const INSET_BORDER_ALPHA_STRONG := 0.6    ## Well/input inset shadow
const INSET_BORDER_ALPHA_DARK := 0.5      ## Input normal border
const INSET_BORDER_ALPHA_MED := 0.3       ## Progress/slider borders
const INSET_BORDER_ALPHA_LIGHT := 0.2     ## Pressed button border

## Lavender accent alphas (focus states, glows)
const LAVENDER_FOCUS_ALPHA := 0.3         ## Input focus border
const LAVENDER_GLOW_ALPHA := 0.15         ## Input focus shadow/glow
const LAVENDER_BORDER_ALPHA := 0.2        ## Progress/slider fill borders

## Shadow offsets by size
const SHADOW_OFFSET_SM := 2               ## Small elements (checkbox)
const SHADOW_OFFSET_MD := 6               ## Medium elements (info cards)
const SHADOW_OFFSET_LG := 8               ## Large elements (cards, panels)
const SHADOW_OFFSET_XL := 16              ## Extra large (modals)

## Shadow sizes
const SHADOW_SIZE_XS := 4               ## Extra small element shadows (checkbox)
const SHADOW_SIZE_SM := 16              ## Small element shadows (tooltips)
const SHADOW_SIZE_MD := 20              ## Medium element shadows (info cards)
const SHADOW_SIZE_POPUP := 24           ## Popup shadows
const SHADOW_SIZE_LG := 32              ## Large element shadows (cards, panels)
const SHADOW_SIZE_XL := 48              ## Extra large shadows (modals)

## Border accent alpha (status badges, etc.)
const BORDER_ACCENT_ALPHA := 0.25



# =============================================================================
# TYPOGRAPHY
# =============================================================================

const FONT_DISPLAY := 24   ## Screen headers, major titles
const FONT_TITLE := 20     ## Panel headers
const FONT_ICON := 16      ## Icon labels
const FONT_HEADING := 15   ## Section titles
const FONT_BODY := 14      ## Default text
const FONT_CAPTION := 13   ## Secondary labels
const FONT_SM := 12     ## Hints, metadata
const FONT_XS := 11      ## Section headers (uppercase)
const FONT_MONO := 13      ## Values, paths, technical data
const FONT_BUTTON := 13    ## Button text (consistent across all states)
const FONT_SPLASH := 48    ## Splash screen title (extra large)


# =============================================================================
# SPACING (8px base grid)
# =============================================================================

const SPACING_XS := 4
const SPACING_SM := 8
const SPACING_MD := 12
const SPACING_LG := 16
const SPACING_XL := 20
const SPACING_2XL := 24
const SPACING_3XL := 32

## Scroll fade configuration - height of the fade gradient zone
const SCROLL_FADE_HEIGHT := 48

## Input-specific padding (doesn't match grid exactly)
const INPUT_PADDING_V := 14
const INPUT_ARROW_SPACE := 20  ## Extra space for dropdown arrow in OptionButton
const SPINBOX_PADDING_V := 6   ## SpinBox internal vertical padding (compact)
const SPINBOX_PADDING_H := 8   ## SpinBox internal horizontal padding (compact)

## Button-specific padding
const BUTTON_SHADER_INSET := 4  ## Visual inset from Control edge to button shape (neumorphic border)
const BUTTON_PADDING_V := 18    ## Internal padding (button edge to text) for large buttons
const BUTTON_PADDING_H := 24
const BUTTON_SM_PADDING_V := 10 ## Internal padding for small buttons

## Component sizes
const STATUS_DOT_SIZE := 8            ## Status indicator dots
const STATUS_INDICATOR_SIZE := 12     ## Larger status indicators (diagnostics)
const CHECKBOX_INDICATOR_SIZE := 20   ## Checkbox tile indicator box
const NAV_PILL_MIN_WIDTH := 80        ## Navigation pill minimum width
const NAV_PILL_HEIGHT := 32           ## Navigation pill height
const SCROLLBAR_WIDTH := 16           ## Standard scrollbar width for spacing calculations

## Card internal spacing

## Component sizes (buttons are content-aware - no explicit sizes)
const INPUT_HEIGHT := 40               ## Standard input height
const HEADER_HEIGHT := 60              ## App header
const FOOTER_HEIGHT := 80              ## App footer
const CARD_MIN_WIDTH := 200            ## Minimum card width
const CARD_MIN_HEIGHT := 100           ## Minimum card height (standard card)
const INFO_CARD_MIN_HEIGHT := 80       ## Minimum info card height
const PREVIEW_HEIGHT_SM := 200         ## Small preview areas
const PREVIEW_HEIGHT_MD := 350         ## Medium preview areas
const PROGRESS_BAR_HEIGHT := 24        ## Progress bar height
const DIVIDER_LINE_HEIGHT := 1         ## Divider line thickness
const STATUS_PILL_MIN_WIDTH := 100     ## Status pill minimum width
const STATUS_PILL_HEIGHT := 36         ## Status pill height
const THUMB_WELL_WIDTH := 100          ## Thumbnail well width
const THUMB_WELL_HEIGHT := 75          ## Thumbnail well height
const INPUT_SPINBOX_WIDTH := 100       ## SpinBox input width
const CARD_WIDTH_MD := 500             ## Medium card width (summary, output)
const LABEL_WIDTH_SM := 100            ## Small label width for form alignment
const LABEL_WIDTH_MD := 120            ## Medium label width for form alignment
const DIALOG_WIDTH := 420              ## Modal dialog width
const DIALOG_WIDTH_SM := 400           ## Small dialog width
const DIALOG_HEIGHT_SM := 300          ## Small dialog height
const DIALOG_INPUT_MIN_WIDTH := 300    ## Dialog input field minimum width
const DIALOG_LIST_MIN_WIDTH := 380     ## Dialog list minimum width
const DIALOG_LIST_MIN_HEIGHT := 200    ## Dialog list minimum height
const DIALOG_DETAILS_HEIGHT := 60      ## Collapsible details section min height
const ICON_BUTTON_SIZE := 28           ## Small icon-only buttons (up/down/remove)
const RADIO_BUTTON_WIDTH := 120        ## Radio button tile width
const SEQUENCE_INDEX_WIDTH := 20       ## Sequence list index label width
const SEQUENCE_NAME_WIDTH := 50        ## Sequence list name label width
const SEQUENCE_BTN_WIDTH := 70         ## Sequence add button width

## Slider styling
const SLIDER_RADIUS := 3               ## Slider track/fill corner radius
const SLIDER_PADDING_V := 3            ## Slider vertical content margin

## Border widths
const BORDER_WIDTH_ACCENT := 1         ## Accent borders (nav pill, modals)
const BORDER_WIDTH_CHECKBOX := 2       ## Checkbox indicator border
const BORDER_WIDTH_INSET_TOP := 2      ## Inset top border (wells, inputs)
const BORDER_WIDTH_INSET_SIDE := 1     ## Inset side/bottom border

## Nav container padding
const NAV_PADDING_V := 6               ## Nav bar vertical padding


# =============================================================================
# BORDER RADII
# =============================================================================

const RADIUS_SM := 6       ## Small interior elements
const RADIUS_MD := 12      ## Inputs, small buttons
const RADIUS_LG := 14      ## Primary buttons
const RADIUS_XL := 16      ## Cards, preview areas
const RADIUS_2XL := 20     ## Main panels
const RADIUS_NAV := 22     ## Nav container (pill-like)
const RADIUS_PILL := 9999  ## Fully rounded pills


# =============================================================================
# ANIMATION TIMING (seconds)
# =============================================================================

const ANIM_MICRO := 0.15   ## Hover, focus
const ANIM_STATE := 0.25   ## State changes
const ANIM_PANEL := 0.35   ## Panel transitions
const ANIM_PULSE := 3.0    ## Status pulse (full cycle)


# =============================================================================
# Z-INDEX LAYERING
# =============================================================================

const Z_INDEX_SCROLL_FADE := 100   ## Scroll fade overlays (below chrome)
const Z_INDEX_CHROME := 200        ## Header, footer (above content)
const Z_INDEX_MODAL := 300         ## Modals, dialogs (above everything)


# =============================================================================
# DIAGNOSTICS TOOL (Developer UI)
# =============================================================================
## High-contrast colors for timing diagnostics - intentionally brighter than
## user-facing UI for at-a-glance pass/fail visibility in developer tools.

const DIAG_WINDOW_SIZE := Vector2i(620, 580)     ## Timing diagnostics window size
const DIAG_WINDOW_MIN_SIZE := Vector2i(580, 540) ## Minimum window size

## Diagnostics label/value widths
const DIAG_LABEL_WIDTH_SM := 50     ## Short labels (Offset, Align, Drift)
const DIAG_LABEL_WIDTH_MD := 60     ## Standard metric labels
const DIAG_VALUE_WIDTH_SM := 70     ## Small value columns
const DIAG_VALUE_WIDTH_MD := 80     ## Medium value columns
const DIAG_VALUE_WIDTH_LG := 100    ## Large value columns

## Diagnostics status colors (high contrast)
const DIAG_STATUS_IDLE := Color("#808080")     ## Gray - idle/unknown
const DIAG_STATUS_RUNNING := Color("#32CD32")  ## Lime green - active/running
const DIAG_STATUS_COMPLETE := Color("#1E90FF") ## Dodger blue - complete
const DIAG_STATUS_OK := Color("#32CD32")       ## Lime green - pass
const DIAG_STATUS_FAIL := Color("#FF4500")     ## Orange red - fail


# =============================================================================
# FOCUS SCREEN DEFAULTS
# =============================================================================

const FOCUS_DEFAULT_EXPOSURE_US := 30000   ## Initial exposure in microseconds
const FOCUS_RING_RADIUS_DEFAULT := 150     ## Default head ring radius
const FOCUS_PREVIEW_CENTER := 256          ## Preview center coordinate (512/2)


# =============================================================================
# THEME RESOURCE
# =============================================================================

var _theme: Theme
var _font: Font
var _font_italic: Font
var _font_medium: Font
var _font_semibold: Font
var _font_bold: Font

## Public font accessors
var font_medium: Font:
	get: return _font_medium

var font_semibold: Font:
	get: return _font_semibold

var font_bold: Font:
	get: return _font_bold


func _ready() -> void:
	_theme = Theme.new()
	_load_fonts()
	_build_theme()
	get_tree().root.theme = _theme


func _load_fonts() -> void:
	# Use preloaded font constants
	_font = FontRegular
	_font_italic = FontItalic
	_font_medium = FontMedium
	_font_semibold = FontSemiBold
	_font_bold = FontBold

	if _font:
		_theme.default_font = _font
	else:
		push_warning("Could not load Plus Jakarta Sans font")


# =============================================================================
# THEME BUILDER
# =============================================================================

func _build_theme() -> void:
	_build_container_styles()
	_build_button_styles()
	_build_label_styles()
	_build_panel_styles()
	_build_input_styles()
	_build_progress_styles()
	_build_slider_styles()
	_build_misc_styles()


# =============================================================================
# CONTAINER STYLES - Eliminate separation/margin overrides
# =============================================================================

func _build_container_styles() -> void:
	# --- VBoxContainer Variations ---
	# Default VBoxContainer has no separation set, inherits from engine default

	# VBoxTight - No separation (commonly used for root layouts)
	_theme.set_type_variation("VBoxTight", "VBoxContainer")
	_theme.set_constant("separation", "VBoxTight", 0)

	# VBoxXS - Extra small separation (4px)
	_theme.set_type_variation("VBoxXS", "VBoxContainer")
	_theme.set_constant("separation", "VBoxXS", SPACING_XS)

	# VBoxSM - Small separation (8px)
	_theme.set_type_variation("VBoxSM", "VBoxContainer")
	_theme.set_constant("separation", "VBoxSM", SPACING_SM)

	# VBoxMD - Medium separation (12px)
	_theme.set_type_variation("VBoxMD", "VBoxContainer")
	_theme.set_constant("separation", "VBoxMD", SPACING_MD)

	# VBoxLG - Large separation (16px)
	_theme.set_type_variation("VBoxLG", "VBoxContainer")
	_theme.set_constant("separation", "VBoxLG", SPACING_LG)

	# VBoxXL - Extra large separation (20px)
	_theme.set_type_variation("VBoxXL", "VBoxContainer")
	_theme.set_constant("separation", "VBoxXL", SPACING_XL)

	# VBox2XL - 2x large separation (24px)
	_theme.set_type_variation("VBox2XL", "VBoxContainer")
	_theme.set_constant("separation", "VBox2XL", SPACING_2XL)

	# VBox3XL - 3x large separation (32px)
	_theme.set_type_variation("VBox3XL", "VBoxContainer")
	_theme.set_constant("separation", "VBox3XL", SPACING_3XL)

	# --- HBoxContainer Variations ---

	# HBoxTight - No separation
	_theme.set_type_variation("HBoxTight", "HBoxContainer")
	_theme.set_constant("separation", "HBoxTight", 0)

	# HBoxXS - Extra small separation (4px)
	_theme.set_type_variation("HBoxXS", "HBoxContainer")
	_theme.set_constant("separation", "HBoxXS", SPACING_XS)

	# HBoxSM - Small separation (8px)
	_theme.set_type_variation("HBoxSM", "HBoxContainer")
	_theme.set_constant("separation", "HBoxSM", SPACING_SM)

	# HBoxMD - Medium separation (12px)
	_theme.set_type_variation("HBoxMD", "HBoxContainer")
	_theme.set_constant("separation", "HBoxMD", SPACING_MD)

	# HBoxLG - Large separation (16px)
	_theme.set_type_variation("HBoxLG", "HBoxContainer")
	_theme.set_constant("separation", "HBoxLG", SPACING_LG)

	# HBoxXL - Extra large separation (20px)
	_theme.set_type_variation("HBoxXL", "HBoxContainer")
	_theme.set_constant("separation", "HBoxXL", SPACING_XL)

	# HBox2XL - 2x large separation (24px)
	_theme.set_type_variation("HBox2XL", "HBoxContainer")
	_theme.set_constant("separation", "HBox2XL", SPACING_2XL)

	# --- GridContainer Variations ---

	# GridMD - Medium grid spacing (h=16, v=8 - common for metrics grids)
	_theme.set_type_variation("GridMD", "GridContainer")
	_theme.set_constant("h_separation", "GridMD", SPACING_LG)
	_theme.set_constant("v_separation", "GridMD", SPACING_SM)

	# HBox3XL - 3x large separation (32px)
	_theme.set_type_variation("HBox3XL", "HBoxContainer")
	_theme.set_constant("separation", "HBox3XL", SPACING_3XL)

	# --- MarginContainer Variations ---

	# MarginScreenContent - Standard screen content margins (horizontal padding, scroll fade vertical)
	_theme.set_type_variation("MarginScreenContent", "MarginContainer")
	_theme.set_constant("margin_left", "MarginScreenContent", SPACING_2XL)
	_theme.set_constant("margin_right", "MarginScreenContent", SPACING_2XL)
	_theme.set_constant("margin_top", "MarginScreenContent", SCROLL_FADE_HEIGHT)
	_theme.set_constant("margin_bottom", "MarginScreenContent", SCROLL_FADE_HEIGHT)

	# MarginScreenContentWithScrollbar - For screens with built-in scrollbar (reduced right margin)
	_theme.set_type_variation("MarginScreenContentWithScrollbar", "MarginContainer")
	_theme.set_constant("margin_left", "MarginScreenContentWithScrollbar", SPACING_2XL)
	_theme.set_constant("margin_right", "MarginScreenContentWithScrollbar", SPACING_2XL - SCROLLBAR_WIDTH)
	_theme.set_constant("margin_top", "MarginScreenContentWithScrollbar", SCROLL_FADE_HEIGHT)
	_theme.set_constant("margin_bottom", "MarginScreenContentWithScrollbar", SCROLL_FADE_HEIGHT)

	# MarginScreenContentLeftOnly - Left padding only (for content beside scrollbar or sidebar)
	_theme.set_type_variation("MarginScreenContentLeftOnly", "MarginContainer")
	_theme.set_constant("margin_left", "MarginScreenContentLeftOnly", SPACING_2XL)
	_theme.set_constant("margin_right", "MarginScreenContentLeftOnly", 0)
	_theme.set_constant("margin_top", "MarginScreenContentLeftOnly", SCROLL_FADE_HEIGHT)
	_theme.set_constant("margin_bottom", "MarginScreenContentLeftOnly", SCROLL_FADE_HEIGHT)

	# MarginScreenContentRightOnly - For right-side scroll content (no left margin)
	_theme.set_type_variation("MarginScreenContentRightOnly", "MarginContainer")
	_theme.set_constant("margin_left", "MarginScreenContentRightOnly", 0)
	_theme.set_constant("margin_right", "MarginScreenContentRightOnly", SPACING_2XL)
	_theme.set_constant("margin_top", "MarginScreenContentRightOnly", SCROLL_FADE_HEIGHT)
	_theme.set_constant("margin_bottom", "MarginScreenContentRightOnly", SCROLL_FADE_HEIGHT)

	# MarginPanel - Standard panel padding (16px all sides)
	_theme.set_type_variation("MarginPanel", "MarginContainer")
	_theme.set_constant("margin_left", "MarginPanel", SPACING_LG)
	_theme.set_constant("margin_right", "MarginPanel", SPACING_LG)
	_theme.set_constant("margin_top", "MarginPanel", SPACING_LG)
	_theme.set_constant("margin_bottom", "MarginPanel", SPACING_LG)


func _build_button_styles() -> void:
	# Default Button style (for regular Godot Button widgets)
	# Note: StyledButton uses button.gdshader for raised surface effects
	_theme.set_stylebox("normal", "Button", _create_button_secondary_normal())
	_theme.set_stylebox("hover", "Button", _create_button_secondary_hover())
	_theme.set_stylebox("pressed", "Button", _create_button_secondary_pressed())
	_theme.set_stylebox("disabled", "Button", _create_button_secondary_pressed())
	_theme.set_stylebox("focus", "Button", StyleBoxEmpty.new())
	_theme.set_color("font_color", "Button", CREAM_MUTED)
	_theme.set_color("font_hover_color", "Button", CREAM)
	_theme.set_color("font_pressed_color", "Button", CREAM_MUTED)
	_theme.set_color("font_disabled_color", "Button", CREAM_DIM)
	_theme.set_font_size("font_size", "Button", FONT_SM)  # 12px per mockup

	# ButtonDestructive = Error-colored text
	_theme.set_type_variation("ButtonDestructive", "Button")
	_theme.set_stylebox("normal", "ButtonDestructive", _create_button_secondary_normal())
	_theme.set_stylebox("hover", "ButtonDestructive", _create_button_secondary_hover())
	_theme.set_stylebox("pressed", "ButtonDestructive", _create_button_secondary_pressed())
	_theme.set_stylebox("focus", "ButtonDestructive", StyleBoxEmpty.new())
	_theme.set_color("font_color", "ButtonDestructive", ERROR)
	_theme.set_color("font_hover_color", "ButtonDestructive", ERROR)
	_theme.set_color("font_pressed_color", "ButtonDestructive", ERROR)

	# ButtonTransparent = Empty stylebox for shader-backed buttons
	# Used by StyledButton where button.gdshader handles all visuals
	_theme.set_type_variation("ButtonTransparent", "Button")
	_theme.set_stylebox("normal", "ButtonTransparent", StyleBoxEmpty.new())
	_theme.set_stylebox("hover", "ButtonTransparent", StyleBoxEmpty.new())
	_theme.set_stylebox("pressed", "ButtonTransparent", StyleBoxEmpty.new())
	_theme.set_stylebox("disabled", "ButtonTransparent", StyleBoxEmpty.new())
	_theme.set_stylebox("focus", "ButtonTransparent", StyleBoxEmpty.new())
	_theme.set_font_size("font_size", "ButtonTransparent", FONT_BUTTON)

	# ButtonTransparentPrimary = Transparent with dark text (for nightlight/amber buttons)
	_theme.set_type_variation("ButtonTransparentPrimary", "Button")
	_theme.set_stylebox("normal", "ButtonTransparentPrimary", StyleBoxEmpty.new())
	_theme.set_stylebox("hover", "ButtonTransparentPrimary", StyleBoxEmpty.new())
	_theme.set_stylebox("pressed", "ButtonTransparentPrimary", StyleBoxEmpty.new())
	_theme.set_stylebox("disabled", "ButtonTransparentPrimary", StyleBoxEmpty.new())
	_theme.set_stylebox("focus", "ButtonTransparentPrimary", StyleBoxEmpty.new())
	_theme.set_color("font_color", "ButtonTransparentPrimary", BG_BASE)
	_theme.set_color("font_hover_color", "ButtonTransparentPrimary", BG_BASE)
	_theme.set_color("font_pressed_color", "ButtonTransparentPrimary", BG_BASE_MUTED)
	_theme.set_color("font_disabled_color", "ButtonTransparentPrimary", BG_BASE_MUTED)
	_theme.set_font_size("font_size", "ButtonTransparentPrimary", FONT_BUTTON)
	if _font_semibold:
		_theme.set_font("font", "ButtonTransparentPrimary", _font_semibold)

	# ButtonTransparentSecondary = Transparent with cream text (for secondary buttons)
	_theme.set_type_variation("ButtonTransparentSecondary", "Button")
	_theme.set_stylebox("normal", "ButtonTransparentSecondary", StyleBoxEmpty.new())
	_theme.set_stylebox("hover", "ButtonTransparentSecondary", StyleBoxEmpty.new())
	_theme.set_stylebox("pressed", "ButtonTransparentSecondary", StyleBoxEmpty.new())
	_theme.set_stylebox("disabled", "ButtonTransparentSecondary", StyleBoxEmpty.new())
	_theme.set_stylebox("focus", "ButtonTransparentSecondary", StyleBoxEmpty.new())
	_theme.set_color("font_color", "ButtonTransparentSecondary", CREAM_MUTED)
	_theme.set_color("font_hover_color", "ButtonTransparentSecondary", CREAM)
	_theme.set_color("font_pressed_color", "ButtonTransparentSecondary", CREAM_MUTED)
	_theme.set_color("font_disabled_color", "ButtonTransparentSecondary", CREAM_DIM)
	_theme.set_font_size("font_size", "ButtonTransparentSecondary", FONT_BUTTON)

	# ButtonTransparentDestructive = Transparent with error text
	_theme.set_type_variation("ButtonTransparentDestructive", "Button")
	_theme.set_stylebox("normal", "ButtonTransparentDestructive", StyleBoxEmpty.new())
	_theme.set_stylebox("hover", "ButtonTransparentDestructive", StyleBoxEmpty.new())
	_theme.set_stylebox("pressed", "ButtonTransparentDestructive", StyleBoxEmpty.new())
	_theme.set_stylebox("disabled", "ButtonTransparentDestructive", StyleBoxEmpty.new())
	_theme.set_stylebox("focus", "ButtonTransparentDestructive", StyleBoxEmpty.new())
	_theme.set_color("font_color", "ButtonTransparentDestructive", ERROR)
	_theme.set_color("font_hover_color", "ButtonTransparentDestructive", ERROR)
	_theme.set_color("font_pressed_color", "ButtonTransparentDestructive", ERROR)
	_theme.set_color("font_disabled_color", "ButtonTransparentDestructive", CREAM_DIM)
	_theme.set_font_size("font_size", "ButtonTransparentDestructive", FONT_BUTTON)

	# Shared StyleBox for all ButtonStyled* variations (shader handles visuals)
	var styled_button_style := StyleBoxEmpty.new()
	styled_button_style.content_margin_left = BUTTON_PADDING_H
	styled_button_style.content_margin_right = BUTTON_PADDING_H
	styled_button_style.content_margin_top = BUTTON_PADDING_V
	styled_button_style.content_margin_bottom = BUTTON_PADDING_V

	# ButtonStyledPrimary = For StyledButton nightlight mode (dark text on amber, semibold)
	_theme.set_type_variation("ButtonStyledPrimary", "Button")
	_theme.set_stylebox("normal", "ButtonStyledPrimary", styled_button_style)
	_theme.set_stylebox("hover", "ButtonStyledPrimary", styled_button_style)
	_theme.set_stylebox("pressed", "ButtonStyledPrimary", styled_button_style)
	_theme.set_stylebox("disabled", "ButtonStyledPrimary", styled_button_style)
	_theme.set_stylebox("focus", "ButtonStyledPrimary", StyleBoxEmpty.new())
	_theme.set_color("font_color", "ButtonStyledPrimary", BG_BASE)
	_theme.set_color("font_hover_color", "ButtonStyledPrimary", BG_BASE)
	_theme.set_color("font_pressed_color", "ButtonStyledPrimary", BG_BASE_MUTED)
	_theme.set_color("font_disabled_color", "ButtonStyledPrimary", BG_BASE_MUTED)
	_theme.set_font_size("font_size", "ButtonStyledPrimary", FONT_BUTTON)
	if _font_semibold:
		_theme.set_font("font", "ButtonStyledPrimary", _font_semibold)

	# ButtonStyledSecondary = For StyledButton secondary mode (cream text)
	_theme.set_type_variation("ButtonStyledSecondary", "Button")
	_theme.set_stylebox("normal", "ButtonStyledSecondary", styled_button_style)
	_theme.set_stylebox("hover", "ButtonStyledSecondary", styled_button_style)
	_theme.set_stylebox("pressed", "ButtonStyledSecondary", styled_button_style)
	_theme.set_stylebox("disabled", "ButtonStyledSecondary", styled_button_style)
	_theme.set_stylebox("focus", "ButtonStyledSecondary", StyleBoxEmpty.new())
	_theme.set_color("font_color", "ButtonStyledSecondary", CREAM_MUTED)
	_theme.set_color("font_hover_color", "ButtonStyledSecondary", CREAM)
	_theme.set_color("font_pressed_color", "ButtonStyledSecondary", CREAM_MUTED)
	_theme.set_color("font_disabled_color", "ButtonStyledSecondary", CREAM_DIM)
	_theme.set_font_size("font_size", "ButtonStyledSecondary", FONT_BUTTON)

	# ButtonStyledDestructive = For StyledButton destructive mode (error text)
	_theme.set_type_variation("ButtonStyledDestructive", "Button")
	_theme.set_stylebox("normal", "ButtonStyledDestructive", styled_button_style)
	_theme.set_stylebox("hover", "ButtonStyledDestructive", styled_button_style)
	_theme.set_stylebox("pressed", "ButtonStyledDestructive", styled_button_style)
	_theme.set_stylebox("disabled", "ButtonStyledDestructive", styled_button_style)
	_theme.set_stylebox("focus", "ButtonStyledDestructive", StyleBoxEmpty.new())
	_theme.set_color("font_color", "ButtonStyledDestructive", ERROR)
	_theme.set_color("font_hover_color", "ButtonStyledDestructive", ERROR)
	_theme.set_color("font_pressed_color", "ButtonStyledDestructive", ERROR)
	_theme.set_color("font_disabled_color", "ButtonStyledDestructive", CREAM_DIM)
	_theme.set_font_size("font_size", "ButtonStyledDestructive", FONT_BUTTON)

	# ButtonNavActive = Navigation bar active tab (transparent, medium font, cream)
	_theme.set_type_variation("ButtonNavActive", "Button")
	_theme.set_stylebox("normal", "ButtonNavActive", StyleBoxEmpty.new())
	_theme.set_stylebox("hover", "ButtonNavActive", StyleBoxEmpty.new())
	_theme.set_stylebox("pressed", "ButtonNavActive", StyleBoxEmpty.new())
	_theme.set_stylebox("focus", "ButtonNavActive", StyleBoxEmpty.new())
	_theme.set_color("font_color", "ButtonNavActive", CREAM)
	_theme.set_color("font_hover_color", "ButtonNavActive", CREAM)
	_theme.set_color("font_pressed_color", "ButtonNavActive", CREAM)
	_theme.set_font_size("font_size", "ButtonNavActive", FONT_CAPTION)
	if _font_medium:
		_theme.set_font("font", "ButtonNavActive", _font_medium)

	# ButtonNavInactive = Navigation bar inactive tab (transparent, dim)
	_theme.set_type_variation("ButtonNavInactive", "Button")
	_theme.set_stylebox("normal", "ButtonNavInactive", _create_nav_inactive_style())
	_theme.set_stylebox("hover", "ButtonNavInactive", _create_nav_hover_style())
	_theme.set_stylebox("pressed", "ButtonNavInactive", _create_nav_inactive_style())
	_theme.set_stylebox("focus", "ButtonNavInactive", StyleBoxEmpty.new())
	_theme.set_color("font_color", "ButtonNavInactive", CREAM_DIM)
	_theme.set_color("font_hover_color", "ButtonNavInactive", CREAM)
	_theme.set_color("font_pressed_color", "ButtonNavInactive", CREAM_DIM)
	_theme.set_font_size("font_size", "ButtonNavInactive", FONT_CAPTION)

	# ButtonLink = Small link-style button (flat, dim, hover brightens)
	_theme.set_type_variation("ButtonLink", "Button")
	_theme.set_stylebox("normal", "ButtonLink", StyleBoxEmpty.new())
	_theme.set_stylebox("hover", "ButtonLink", StyleBoxEmpty.new())
	_theme.set_stylebox("pressed", "ButtonLink", StyleBoxEmpty.new())
	_theme.set_stylebox("focus", "ButtonLink", StyleBoxEmpty.new())
	_theme.set_color("font_color", "ButtonLink", CREAM_DIM)
	_theme.set_color("font_hover_color", "ButtonLink", CREAM_MUTED)
	_theme.set_color("font_pressed_color", "ButtonLink", CREAM_DIM)
	_theme.set_font_size("font_size", "ButtonLink", FONT_SM)


func _build_label_styles() -> void:
	# Default Label - Body: 14px, Regular (400)
	_theme.set_color("font_color", "Label", CREAM)
	_theme.set_font_size("font_size", "Label", FONT_BODY)

	# LabelTitle - 20px, Medium (500)
	_theme.set_type_variation("LabelTitle", "Label")
	_theme.set_font_size("font_size", "LabelTitle", FONT_TITLE)
	_theme.set_color("font_color", "LabelTitle", CREAM)
	if _font_medium:
		_theme.set_font("font", "LabelTitle", _font_medium)

	# LabelHeading - 15px, Medium (500), muted color for card headers
	_theme.set_type_variation("LabelHeading", "Label")
	_theme.set_font_size("font_size", "LabelHeading", FONT_HEADING)
	_theme.set_color("font_color", "LabelHeading", CREAM_MUTED)  # Per mockup line 330
	if _font_medium:
		_theme.set_font("font", "LabelHeading", _font_medium)

	# LabelCaption - 13px, Regular (400)
	_theme.set_type_variation("LabelCaption", "Label")
	_theme.set_font_size("font_size", "LabelCaption", FONT_CAPTION)
	_theme.set_color("font_color", "LabelCaption", CREAM_MUTED)

	# LabelSmall - 12px, Regular (400)
	_theme.set_type_variation("LabelSmall", "Label")
	_theme.set_font_size("font_size", "LabelSmall", FONT_SM)
	_theme.set_color("font_color", "LabelSmall", CREAM_MUTED)

	# LabelSection - 11px, SemiBold (600), uppercase, tracked
	_theme.set_type_variation("LabelSection", "Label")
	_theme.set_font_size("font_size", "LabelSection", FONT_XS)
	_theme.set_color("font_color", "LabelSection", LAVENDER)
	if _font_semibold:
		_theme.set_font("font", "LabelSection", _font_semibold)

	# LabelMono - 13px, Regular (400)
	_theme.set_type_variation("LabelMono", "Label")
	_theme.set_font_size("font_size", "LabelMono", FONT_MONO)
	_theme.set_color("font_color", "LabelMono", CREAM)

	# LabelDim - Body size, Regular (400)
	_theme.set_type_variation("LabelDim", "Label")
	_theme.set_font_size("font_size", "LabelDim", FONT_BODY)
	_theme.set_color("font_color", "LabelDim", CREAM_DIM)

	# LabelSmallDim - 12px, Regular (400)
	_theme.set_type_variation("LabelSmallDim", "Label")
	_theme.set_font_size("font_size", "LabelSmallDim", FONT_SM)
	_theme.set_color("font_color", "LabelSmallDim", CREAM_DIM)

	# LabelDisplay - 24px, SemiBold (600)
	_theme.set_type_variation("LabelDisplay", "Label")
	_theme.set_font_size("font_size", "LabelDisplay", FONT_DISPLAY)
	_theme.set_color("font_color", "LabelDisplay", CREAM)
	if _font_semibold:
		_theme.set_font("font", "LabelDisplay", _font_semibold)

	# LabelLogo - 20px, SemiBold (600)
	_theme.set_type_variation("LabelLogo", "Label")
	_theme.set_font_size("font_size", "LabelLogo", FONT_TITLE)
	_theme.set_color("font_color", "LabelLogo", CREAM)
	if _font_semibold:
		_theme.set_font("font", "LabelLogo", _font_semibold)

	# LabelMonoMuted - 13px, Regular (400)
	_theme.set_type_variation("LabelMonoMuted", "Label")
	_theme.set_font_size("font_size", "LabelMonoMuted", FONT_MONO)
	_theme.set_color("font_color", "LabelMonoMuted", CREAM_MUTED)

	# --- Status Label Variations (for dynamic status display) ---

	# LabelSuccess - Body size, success green color
	_theme.set_type_variation("LabelSuccess", "Label")
	_theme.set_font_size("font_size", "LabelSuccess", FONT_BODY)
	_theme.set_color("font_color", "LabelSuccess", SUCCESS)

	# LabelTitleSuccess - Title size, medium font, success green
	_theme.set_type_variation("LabelTitleSuccess", "Label")
	_theme.set_font_size("font_size", "LabelTitleSuccess", FONT_TITLE)
	_theme.set_color("font_color", "LabelTitleSuccess", SUCCESS)
	if _font_medium:
		_theme.set_font("font", "LabelTitleSuccess", _font_medium)

	# LabelError - Body size, error rose color
	_theme.set_type_variation("LabelError", "Label")
	_theme.set_font_size("font_size", "LabelError", FONT_BODY)
	_theme.set_color("font_color", "LabelError", ERROR)

	# LabelInfo - Body size, info lavender color
	_theme.set_type_variation("LabelInfo", "Label")
	_theme.set_font_size("font_size", "LabelInfo", FONT_BODY)
	_theme.set_color("font_color", "LabelInfo", LAVENDER)

	# LabelAmber - Body size, amber for highlighted/important values
	_theme.set_type_variation("LabelAmber", "Label")
	_theme.set_font_size("font_size", "LabelAmber", FONT_BODY)
	_theme.set_color("font_color", "LabelAmber", AMBER)

	# LabelSmallAmber - Small size, amber for highlighted/important values
	_theme.set_type_variation("LabelSmallAmber", "Label")
	_theme.set_font_size("font_size", "LabelSmallAmber", FONT_SM)
	_theme.set_color("font_color", "LabelSmallAmber", AMBER)

	# LabelSmallSuccess - Small size, success green
	_theme.set_type_variation("LabelSmallSuccess", "Label")
	_theme.set_font_size("font_size", "LabelSmallSuccess", FONT_SM)
	_theme.set_color("font_color", "LabelSmallSuccess", SUCCESS)

	# LabelSmallError - Small size, error rose
	_theme.set_type_variation("LabelSmallError", "Label")
	_theme.set_font_size("font_size", "LabelSmallError", FONT_SM)
	_theme.set_color("font_color", "LabelSmallError", ERROR)

	# LabelSmallInfo - Small size, info lavender
	_theme.set_type_variation("LabelSmallInfo", "Label")
	_theme.set_font_size("font_size", "LabelSmallInfo", FONT_SM)
	_theme.set_color("font_color", "LabelSmallInfo", LAVENDER)

	# --- Additional Specialty Labels ---

	# LabelSplash - 48px, SemiBold, for splash screen title
	_theme.set_type_variation("LabelSplash", "Label")
	_theme.set_font_size("font_size", "LabelSplash", FONT_SPLASH)
	_theme.set_color("font_color", "LabelSplash", CREAM)
	if _font_semibold:
		_theme.set_font("font", "LabelSplash", _font_semibold)

	# LabelIcon - 16px, for icon labels
	_theme.set_type_variation("LabelIcon", "Label")
	_theme.set_font_size("font_size", "LabelIcon", FONT_ICON)
	_theme.set_color("font_color", "LabelIcon", CREAM)

	# LabelTitleBold - 20px, Bold, for dialog/error titles
	_theme.set_type_variation("LabelTitleBold", "Label")
	_theme.set_font_size("font_size", "LabelTitleBold", FONT_TITLE)
	_theme.set_color("font_color", "LabelTitleBold", CREAM)
	if _font_bold:
		_theme.set_font("font", "LabelTitleBold", _font_bold)

	# LabelTitleMuted - 20px, CREAM_MUTED for message labels in dialogs
	_theme.set_type_variation("LabelTitleMuted", "Label")
	_theme.set_font_size("font_size", "LabelTitleMuted", FONT_TITLE)
	_theme.set_color("font_color", "LabelTitleMuted", CREAM_MUTED)

	# LabelTitleError - 20px, Bold, error color for error dialog titles
	_theme.set_type_variation("LabelTitleError", "Label")
	_theme.set_font_size("font_size", "LabelTitleError", FONT_TITLE)
	_theme.set_color("font_color", "LabelTitleError", ERROR)
	if _font_bold:
		_theme.set_font("font", "LabelTitleError", _font_bold)

	# LabelIconTitle - Title-sized icons for dialog severity indicators
	_theme.set_type_variation("LabelIconTitle", "Label")
	_theme.set_font_size("font_size", "LabelIconTitle", FONT_TITLE)
	_theme.set_color("font_color", "LabelIconTitle", CREAM)

	# LabelIconTitleSuccess - Title icon in success green
	_theme.set_type_variation("LabelIconTitleSuccess", "Label")
	_theme.set_font_size("font_size", "LabelIconTitleSuccess", FONT_TITLE)
	_theme.set_color("font_color", "LabelIconTitleSuccess", SUCCESS)

	# LabelIconTitleError - Title icon in error rose
	_theme.set_type_variation("LabelIconTitleError", "Label")
	_theme.set_font_size("font_size", "LabelIconTitleError", FONT_TITLE)
	_theme.set_color("font_color", "LabelIconTitleError", ERROR)

	# LabelCheckboxOn is identical to default Label - use "Label" directly

	# LabelCheckboxOff - Body size for unselected checkbox tiles (muted cream)
	_theme.set_type_variation("LabelCheckboxOff", "Label")
	_theme.set_font_size("font_size", "LabelCheckboxOff", FONT_BODY)
	_theme.set_color("font_color", "LabelCheckboxOff", CREAM_MUTED)

	# LabelCheckboxDisabled - Body size for disabled checkbox tiles (dim cream)
	_theme.set_type_variation("LabelCheckboxDisabled", "Label")
	_theme.set_font_size("font_size", "LabelCheckboxDisabled", FONT_BODY)
	_theme.set_color("font_color", "LabelCheckboxDisabled", CREAM_DIM)

	# LabelCheckmark - Body size for checkmark in BG_BASE color
	_theme.set_type_variation("LabelCheckmark", "Label")
	_theme.set_font_size("font_size", "LabelCheckmark", FONT_BODY)
	_theme.set_color("font_color", "LabelCheckmark", BG_BASE)


func _build_panel_styles() -> void:
	# Default PanelContainer = Card style
	_theme.set_stylebox("panel", "PanelContainer", _create_card_style())

	# PanelSurface = Elevated surface (same style as default card)
	_theme.set_type_variation("PanelSurface", "PanelContainer")
	_theme.set_stylebox("panel", "PanelSurface", _create_card_style())

	# PanelWell = Inset/recessed area
	_theme.set_type_variation("PanelWell", "PanelContainer")
	_theme.set_stylebox("panel", "PanelWell", _create_well_style())

	# PanelWellFlush = Inset/recessed area with no padding (for previews/viewports)
	_theme.set_type_variation("PanelWellFlush", "PanelContainer")
	_theme.set_stylebox("panel", "PanelWellFlush", _create_well_flush_style())

	# PanelInfoCard = Smaller info display
	_theme.set_type_variation("PanelInfoCard", "PanelContainer")
	_theme.set_stylebox("panel", "PanelInfoCard", _create_info_card_style())

	# PanelModal = Dialog/modal
	_theme.set_type_variation("PanelModal", "PanelContainer")
	_theme.set_stylebox("panel", "PanelModal", _create_modal_style())

	# PanelTransparent = No background
	_theme.set_type_variation("PanelTransparent", "PanelContainer")
	_theme.set_stylebox("panel", "PanelTransparent", StyleBoxEmpty.new())

	# PanelNavContainer = Nav bar container (darker than cards, same shadow treatment)
	_theme.set_type_variation("PanelNavContainer", "PanelContainer")
	_theme.set_stylebox("panel", "PanelNavContainer", _create_nav_container_style())

	# PanelHeaderFooter = Transparent panel with padding for header/footer areas
	_theme.set_type_variation("PanelHeaderFooter", "PanelContainer")
	var header_footer_style := StyleBoxEmpty.new()
	header_footer_style.content_margin_left = SPACING_2XL
	header_footer_style.content_margin_right = SPACING_2XL
	header_footer_style.content_margin_top = SPACING_MD
	header_footer_style.content_margin_bottom = SPACING_MD
	_theme.set_stylebox("panel", "PanelHeaderFooter", header_footer_style)

	# PanelFooter = Transparent panel with footer-specific padding
	_theme.set_type_variation("PanelFooter", "PanelContainer")
	var footer_style := StyleBoxEmpty.new()
	footer_style.content_margin_left = SPACING_2XL
	footer_style.content_margin_right = SPACING_2XL
	footer_style.content_margin_top = SPACING_LG
	footer_style.content_margin_bottom = SPACING_LG
	_theme.set_stylebox("panel", "PanelFooter", footer_style)

	# PanelPill = Pill-shaped panel with padding only (shader handles visuals)
	# Used by StatusPill, NavigationBar active tab, and similar pill-shaped elements
	_theme.set_type_variation("PanelPill", "PanelContainer")
	var pill_style := StyleBoxEmpty.new()
	pill_style.content_margin_left = SPACING_LG
	pill_style.content_margin_right = SPACING_LG
	pill_style.content_margin_top = SPACING_SM
	pill_style.content_margin_bottom = SPACING_SM
	_theme.set_stylebox("panel", "PanelPill", pill_style)
	# Also register for Panel base type (used by nav bar backgrounds)
	_theme.set_type_variation("PanelPillBG", "Panel")
	_theme.set_stylebox("panel", "PanelPillBG", pill_style)

	# PanelCheckboxTile = Checkbox tile panel with padding only (shader handles visuals)
	_theme.set_type_variation("PanelCheckboxTile", "PanelContainer")
	var checkbox_style := StyleBoxEmpty.new()
	checkbox_style.content_margin_left = SPACING_MD
	checkbox_style.content_margin_right = SPACING_MD
	checkbox_style.content_margin_top = SPACING_MD
	checkbox_style.content_margin_bottom = SPACING_MD
	_theme.set_stylebox("panel", "PanelCheckboxTile", checkbox_style)

	# --- Status Badge Panel Variations ---

	# PanelBadgeSuccess = Badge with success-colored border
	_theme.set_type_variation("PanelBadgeSuccess", "PanelContainer")
	_theme.set_stylebox("panel", "PanelBadgeSuccess", _create_badge_style(SUCCESS))

	# PanelBadgeError = Badge with error-colored border
	_theme.set_type_variation("PanelBadgeError", "PanelContainer")
	_theme.set_stylebox("panel", "PanelBadgeError", _create_badge_style(ERROR))

	# PanelBadgeInfo = Badge with info-colored border
	_theme.set_type_variation("PanelBadgeInfo", "PanelContainer")
	_theme.set_stylebox("panel", "PanelBadgeInfo", _create_badge_style(LAVENDER))

	# PanelBadgeNeutral = Badge with neutral (muted) border
	_theme.set_type_variation("PanelBadgeNeutral", "PanelContainer")
	_theme.set_stylebox("panel", "PanelBadgeNeutral", _create_badge_style(CREAM_MUTED))

	# --- Checkbox Indicator Panel Variations ---

	# PanelCheckboxIndicatorOn = Selected checkbox indicator (lavender fill with glow)
	_theme.set_type_variation("PanelCheckboxIndicatorOn", "PanelContainer")
	_theme.set_stylebox("panel", "PanelCheckboxIndicatorOn", _create_checkbox_indicator_on_style())

	# PanelCheckboxIndicatorOff = Unselected checkbox indicator (transparent with border)
	_theme.set_type_variation("PanelCheckboxIndicatorOff", "PanelContainer")
	_theme.set_stylebox("panel", "PanelCheckboxIndicatorOff", _create_checkbox_indicator_off_style())


func _build_input_styles() -> void:
	# LineEdit
	_theme.set_stylebox("normal", "LineEdit", _create_input_normal())
	_theme.set_stylebox("focus", "LineEdit", _create_input_focus())
	_theme.set_stylebox("read_only", "LineEdit", _create_input_normal())
	_theme.set_color("font_color", "LineEdit", CREAM)
	_theme.set_color("font_placeholder_color", "LineEdit", CREAM_DIM)
	_theme.set_color("caret_color", "LineEdit", CREAM)
	_theme.set_color("selection_color", "LineEdit", with_alpha(LAVENDER, 0.3))
	_theme.set_font_size("font_size", "LineEdit", FONT_BODY)

	# LineEditTransparent - For shader-backed inputs (StyledLineEdit, StyledSpinBox inner edit)
	_theme.set_type_variation("LineEditTransparent", "LineEdit")
	var lineedit_transparent := StyleBoxEmpty.new()
	lineedit_transparent.content_margin_left = SPACING_LG
	lineedit_transparent.content_margin_right = SPACING_LG
	lineedit_transparent.content_margin_top = INPUT_PADDING_V
	lineedit_transparent.content_margin_bottom = INPUT_PADDING_V
	_theme.set_stylebox("normal", "LineEditTransparent", lineedit_transparent)
	_theme.set_stylebox("focus", "LineEditTransparent", lineedit_transparent)
	_theme.set_stylebox("read_only", "LineEditTransparent", lineedit_transparent)

	# LineEditTransparentCompact - For SpinBox inner edit (smaller padding)
	_theme.set_type_variation("LineEditTransparentCompact", "LineEdit")
	var lineedit_compact := StyleBoxEmpty.new()
	lineedit_compact.content_margin_left = SPINBOX_PADDING_H
	lineedit_compact.content_margin_right = SPINBOX_PADDING_H
	lineedit_compact.content_margin_top = SPINBOX_PADDING_V
	lineedit_compact.content_margin_bottom = SPINBOX_PADDING_V
	_theme.set_stylebox("normal", "LineEditTransparentCompact", lineedit_compact)
	_theme.set_stylebox("focus", "LineEditTransparentCompact", lineedit_compact)
	_theme.set_stylebox("read_only", "LineEditTransparentCompact", lineedit_compact)

	# TextEdit
	_theme.set_stylebox("normal", "TextEdit", _create_input_normal())
	_theme.set_stylebox("focus", "TextEdit", _create_input_focus())
	_theme.set_stylebox("read_only", "TextEdit", _create_input_normal())
	_theme.set_color("font_color", "TextEdit", CREAM)
	_theme.set_color("font_placeholder_color", "TextEdit", CREAM_DIM)
	_theme.set_color("caret_color", "TextEdit", CREAM)
	_theme.set_color("selection_color", "TextEdit", with_alpha(LAVENDER, 0.3))
	_theme.set_font_size("font_size", "TextEdit", FONT_BODY)

	# SpinBox
	_theme.set_stylebox("focus", "SpinBox", StyleBoxEmpty.new())
	_theme.set_color("font_color", "SpinBox", CREAM)
	_theme.set_font_size("font_size", "SpinBox", FONT_BODY)

	# OptionButton
	_theme.set_stylebox("normal", "OptionButton", _create_input_normal())
	_theme.set_stylebox("hover", "OptionButton", _create_input_normal())
	_theme.set_stylebox("pressed", "OptionButton", _create_input_normal())
	_theme.set_stylebox("focus", "OptionButton", _create_input_focus())
	_theme.set_color("font_color", "OptionButton", CREAM)
	_theme.set_color("font_hover_color", "OptionButton", CREAM)
	_theme.set_color("font_pressed_color", "OptionButton", CREAM)
	_theme.set_color("font_disabled_color", "OptionButton", CREAM_DIM)
	_theme.set_font_size("font_size", "OptionButton", FONT_BODY)

	# OptionButtonTransparent - For shader-backed dropdowns (StyledOptionButton)
	_theme.set_type_variation("OptionButtonTransparent", "OptionButton")
	var optionbtn_transparent := StyleBoxEmpty.new()
	optionbtn_transparent.content_margin_left = SPACING_LG
	optionbtn_transparent.content_margin_right = SPACING_LG + INPUT_ARROW_SPACE
	optionbtn_transparent.content_margin_top = INPUT_PADDING_V
	optionbtn_transparent.content_margin_bottom = INPUT_PADDING_V
	_theme.set_stylebox("normal", "OptionButtonTransparent", optionbtn_transparent)
	_theme.set_stylebox("hover", "OptionButtonTransparent", optionbtn_transparent)
	_theme.set_stylebox("pressed", "OptionButtonTransparent", optionbtn_transparent)
	_theme.set_stylebox("disabled", "OptionButtonTransparent", optionbtn_transparent)
	_theme.set_stylebox("focus", "OptionButtonTransparent", optionbtn_transparent)


func _build_progress_styles() -> void:
	_theme.set_stylebox("background", "ProgressBar", _create_progress_bg())
	_theme.set_stylebox("fill", "ProgressBar", _create_progress_fill())
	_theme.set_color("font_color", "ProgressBar", CREAM)
	_theme.set_font_size("font_size", "ProgressBar", FONT_SM)


func _build_slider_styles() -> void:
	# HSlider
	_theme.set_stylebox("slider", "HSlider", _create_slider_track())
	_theme.set_stylebox("grabber_area", "HSlider", _create_slider_fill())
	_theme.set_stylebox("grabber_area_highlight", "HSlider", _create_slider_fill())
	_theme.set_stylebox("focus", "HSlider", StyleBoxEmpty.new())

	# VSlider
	_theme.set_stylebox("slider", "VSlider", _create_slider_track())
	_theme.set_stylebox("grabber_area", "VSlider", _create_slider_fill())
	_theme.set_stylebox("grabber_area_highlight", "VSlider", _create_slider_fill())
	_theme.set_stylebox("focus", "VSlider", StyleBoxEmpty.new())


func _build_misc_styles() -> void:
	# ScrollContainer - transparent
	_theme.set_stylebox("panel", "ScrollContainer", StyleBoxEmpty.new())

	# VScrollBar - raised surface style (NOTE: setting "scroll" style causes Godot bug #82818)
	_theme.set_stylebox("grabber", "VScrollBar", _create_scrollbar_grabber())
	_theme.set_stylebox("grabber_highlight", "VScrollBar", _create_scrollbar_grabber_hover())
	_theme.set_stylebox("grabber_pressed", "VScrollBar", _create_scrollbar_grabber_pressed())

	# HScrollBar - same style
	_theme.set_stylebox("grabber", "HScrollBar", _create_scrollbar_grabber())
	_theme.set_stylebox("grabber_highlight", "HScrollBar", _create_scrollbar_grabber_hover())
	_theme.set_stylebox("grabber_pressed", "HScrollBar", _create_scrollbar_grabber_pressed())

	# PopupMenu / PopupPanel
	var popup_style := _create_popup_style()
	_theme.set_stylebox("panel", "PopupMenu", popup_style)
	_theme.set_stylebox("panel", "PopupPanel", popup_style)
	_theme.set_color("font_color", "PopupMenu", CREAM)
	_theme.set_color("font_hover_color", "PopupMenu", CREAM)
	_theme.set_color("font_disabled_color", "PopupMenu", CREAM_DIM)
	_theme.set_font_size("font_size", "PopupMenu", FONT_BODY)

	# Tooltip
	_theme.set_stylebox("panel", "TooltipPanel", _create_tooltip_style())
	_theme.set_color("font_color", "TooltipLabel", CREAM)
	_theme.set_font_size("font_size", "TooltipLabel", FONT_SM)

	# CheckBox - minimal styling, content only
	_theme.set_stylebox("normal", "CheckBox", StyleBoxEmpty.new())
	_theme.set_stylebox("hover", "CheckBox", StyleBoxEmpty.new())
	_theme.set_stylebox("pressed", "CheckBox", StyleBoxEmpty.new())
	_theme.set_stylebox("focus", "CheckBox", StyleBoxEmpty.new())
	_theme.set_color("font_color", "CheckBox", CREAM)
	_theme.set_color("font_hover_color", "CheckBox", CREAM)
	_theme.set_color("font_pressed_color", "CheckBox", CREAM)
	_theme.set_color("font_disabled_color", "CheckBox", CREAM_DIM)
	_theme.set_font_size("font_size", "CheckBox", FONT_BODY)

	# TabContainer
	_theme.set_color("font_selected_color", "TabContainer", CREAM)
	_theme.set_color("font_unselected_color", "TabContainer", CREAM_MUTED)
	_theme.set_color("font_disabled_color", "TabContainer", CREAM_DIM)
	_theme.set_font_size("font_size", "TabContainer", FONT_BODY)

	# ItemList / Tree - no focus outline
	_theme.set_stylebox("focus", "ItemList", StyleBoxEmpty.new())
	_theme.set_stylebox("focus", "Tree", StyleBoxEmpty.new())

	# RichTextLabel - Default styling
	_theme.set_font_size("normal_font_size", "RichTextLabel", FONT_BODY)
	_theme.set_color("default_color", "RichTextLabel", CREAM)

	# RichTextLabelDetails - For collapsible details (e.g., error dialog)
	_theme.set_type_variation("RichTextLabelDetails", "RichTextLabel")
	_theme.set_font_size("normal_font_size", "RichTextLabelDetails", FONT_SM)
	_theme.set_color("default_color", "RichTextLabelDetails", CREAM_DIM)
	var details_style := StyleBoxFlat.new()
	details_style.bg_color = WELL
	details_style.corner_radius_top_left = RADIUS_SM
	details_style.corner_radius_top_right = RADIUS_SM
	details_style.corner_radius_bottom_left = RADIUS_SM
	details_style.corner_radius_bottom_right = RADIUS_SM
	details_style.content_margin_left = SPACING_SM
	details_style.content_margin_right = SPACING_SM
	details_style.content_margin_top = SPACING_SM
	details_style.content_margin_bottom = SPACING_SM
	_theme.set_stylebox("normal", "RichTextLabelDetails", details_style)


# =============================================================================
# STYLEBOX CREATORS - Buttons (for regular Godot Button widgets)
# Note: StyledButton uses button.gdshader for raised surface effects
# =============================================================================

func _create_button_secondary_normal() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = SURFACE
	_set_radius(style, RADIUS_MD)
	_set_border(style, 1)
	style.border_color = with_alpha(CREAM, RIM_LIGHT_ALPHA)
	_set_padding_hv(style, SPACING_LG, BUTTON_SM_PADDING_V)
	return style


func _create_button_secondary_hover() -> StyleBoxFlat:
	var style := _create_button_secondary_normal()
	style.bg_color = SURFACE_ELEVATED
	style.border_color = with_alpha(LAVENDER_DEEP, LAVENDER_BORDER_ALPHA)
	return style


func _create_button_secondary_pressed() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = WELL
	_set_radius(style, RADIUS_MD)
	_set_border(style, BORDER_WIDTH_INSET_SIDE)
	style.border_color = with_alpha(Color.BLACK, INSET_BORDER_ALPHA_LIGHT)
	_set_padding_hv(style, SPACING_LG, BUTTON_SM_PADDING_V)
	return style


func _create_nav_inactive_style() -> StyleBoxFlat:
	## Inactive nav tab - transparent with pill padding
	var style := StyleBoxFlat.new()
	style.bg_color = Color.TRANSPARENT
	_set_radius(style, RADIUS_MD)
	_set_padding_hv(style, SPACING_LG, SPACING_SM)
	return style


func _create_nav_hover_style() -> StyleBoxFlat:
	## Hover state for inactive nav tab - subtle highlight
	var style := StyleBoxFlat.new()
	style.bg_color = with_alpha(SURFACE, HOVER_ALPHA)
	_set_radius(style, RADIUS_MD)
	_set_padding_hv(style, SPACING_LG, SPACING_SM)
	return style


func _create_badge_style(status_color: Color) -> StyleBoxFlat:
	## Badge panel with status-colored border
	var style := StyleBoxFlat.new()
	style.bg_color = SURFACE
	_set_radius(style, RADIUS_MD)
	_set_border(style, BORDER_WIDTH_ACCENT)
	style.border_color = with_alpha(status_color, BORDER_ACCENT_ALPHA)
	_set_padding_hv(style, SPACING_LG, SPACING_SM)
	return style


func _create_checkbox_indicator_on_style() -> StyleBoxFlat:
	## Selected checkbox indicator - lavender fill with subtle glow
	var style := StyleBoxFlat.new()
	style.bg_color = LAVENDER
	_set_radius(style, RADIUS_SM)
	style.shadow_color = with_alpha(LAVENDER, SHADOW_ALPHA_SUBTLE)
	style.shadow_size = SHADOW_SIZE_XS
	return style


func _create_checkbox_indicator_off_style() -> StyleBoxFlat:
	## Unselected checkbox indicator - transparent with border
	var style := StyleBoxFlat.new()
	style.bg_color = Color.TRANSPARENT
	_set_radius(style, RADIUS_SM)
	_set_border(style, BORDER_WIDTH_CHECKBOX)
	style.border_color = CREAM_DIM
	return style


# =============================================================================
# STYLEBOX CREATORS - Panels
# =============================================================================

func _create_card_style() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	# Cards use elevated surface color
	style.bg_color = SURFACE_ELEVATED
	_set_radius(style, RADIUS_2XL)
	# No border - rim highlight is drawn manually in BaseCard._draw()
	# Outer shadow: 0 8px 32px
	style.shadow_color = with_alpha(BG_BASE, SHADOW_ALPHA)
	style.shadow_size = SHADOW_SIZE_LG
	style.shadow_offset = Vector2(0, SHADOW_OFFSET_LG)
	_set_padding(style, SPACING_2XL)
	return style


func _create_info_card_style() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	# Same elevated background as main cards for consistency
	style.bg_color = SURFACE_ELEVATED
	_set_radius(style, RADIUS_LG)
	# No border - rim highlight drawn manually in BaseCard._draw()
	# Shadow (slightly smaller than main cards)
	style.shadow_color = with_alpha(BG_BASE, SHADOW_ALPHA_LIGHT)
	style.shadow_size = SHADOW_SIZE_MD
	style.shadow_offset = Vector2(0, SHADOW_OFFSET_MD)
	_set_padding(style, SPACING_2XL)
	return style


func _create_well_style() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = WELL
	_set_radius(style, RADIUS_MD)
	# Inset shadow effect: darker top border simulates depth
	style.border_width_top = BORDER_WIDTH_INSET_TOP
	style.border_width_left = BORDER_WIDTH_INSET_SIDE
	style.border_width_right = BORDER_WIDTH_INSET_SIDE
	style.border_width_bottom = BORDER_WIDTH_INSET_SIDE
	style.border_color = with_alpha(Color.BLACK, INSET_BORDER_ALPHA_STRONG)
	# Inner shadow approximation via expand margin
	style.expand_margin_top = BORDER_WIDTH_INSET_SIDE
	style.expand_margin_left = BORDER_WIDTH_INSET_SIDE
	style.expand_margin_right = BORDER_WIDTH_INSET_SIDE
	style.expand_margin_bottom = BORDER_WIDTH_INSET_SIDE
	_set_padding_hv(style, SPACING_LG, INPUT_PADDING_V)
	return style


func _create_well_flush_style() -> StyleBoxFlat:
	## Well style with no padding - for previews/viewports where content should
	## extend to the edges of the container.
	var style := StyleBoxFlat.new()
	style.bg_color = WELL
	_set_radius(style, RADIUS_MD)
	# Inset shadow effect: darker top border simulates depth
	style.border_width_top = BORDER_WIDTH_INSET_TOP
	style.border_width_left = BORDER_WIDTH_INSET_SIDE
	style.border_width_right = BORDER_WIDTH_INSET_SIDE
	style.border_width_bottom = BORDER_WIDTH_INSET_SIDE
	style.border_color = with_alpha(Color.BLACK, INSET_BORDER_ALPHA_STRONG)
	# Inner shadow approximation via expand margin
	style.expand_margin_top = BORDER_WIDTH_INSET_SIDE
	style.expand_margin_left = BORDER_WIDTH_INSET_SIDE
	style.expand_margin_right = BORDER_WIDTH_INSET_SIDE
	style.expand_margin_bottom = BORDER_WIDTH_INSET_SIDE
	# No padding - content extends to edges
	return style


func _create_modal_style() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = SURFACE_ELEVATED
	_set_radius(style, RADIUS_2XL)
	# Modals use stronger rim for more prominence
	style.border_width_top = BORDER_WIDTH_ACCENT
	style.border_width_left = 0
	style.border_width_right = 0
	style.border_width_bottom = 0
	style.border_color = with_alpha(CREAM, RIM_LIGHT_STRONG_ALPHA)
	# Larger shadow for floating effect (stronger than standard)
	style.shadow_color = with_alpha(BG_BASE, SHADOW_ALPHA_MODAL)
	style.shadow_size = SHADOW_SIZE_XL
	style.shadow_offset = Vector2(0, SHADOW_OFFSET_XL)
	_set_padding(style, SPACING_2XL)
	return style


func _create_nav_container_style() -> StyleBoxFlat:
	## Nav bar container: similar to cards but with SURFACE background (darker).
	## Used for screen navigation and similar navigation elements.
	var style := StyleBoxFlat.new()
	style.bg_color = SURFACE  # Darker than cards (SURFACE_ELEVATED)
	_set_radius(style, RADIUS_NAV)
	# Same shadow treatment as cards
	style.shadow_color = with_alpha(BG_BASE, SHADOW_ALPHA)
	style.shadow_size = SHADOW_SIZE_LG
	style.shadow_offset = Vector2(0, SHADOW_OFFSET_LG)
	# Rim highlight border
	_set_border(style, BORDER_WIDTH_ACCENT)
	style.border_color = with_alpha(CREAM, RIM_LIGHT_ALPHA)
	# Padding for nav items
	style.content_margin_top = NAV_PADDING_V
	style.content_margin_bottom = NAV_PADDING_V
	style.content_margin_left = SPACING_SM
	style.content_margin_right = SPACING_SM
	return style


# =============================================================================
# STYLEBOX CREATORS - Inputs
# =============================================================================

func _create_input_normal() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = WELL
	_set_radius(style, RADIUS_MD)
	# Inset effect: darker top border
	style.border_width_top = BORDER_WIDTH_INSET_TOP
	style.border_width_left = BORDER_WIDTH_INSET_SIDE
	style.border_width_right = BORDER_WIDTH_INSET_SIDE
	style.border_width_bottom = BORDER_WIDTH_INSET_SIDE
	style.border_color = with_alpha(Color.BLACK, INSET_BORDER_ALPHA_DARK)
	_set_padding_hv(style, SPACING_LG, INPUT_PADDING_V)
	return style


func _create_input_focus() -> StyleBoxFlat:
	var style := _create_input_normal()
	_set_border(style, BORDER_WIDTH_INSET_TOP)
	style.border_color = with_alpha(LAVENDER, LAVENDER_FOCUS_ALPHA)
	style.shadow_color = with_alpha(LAVENDER, LAVENDER_GLOW_ALPHA)
	style.shadow_size = SPACING_XS
	return style


# =============================================================================
# STYLEBOX CREATORS - Progress & Sliders
# =============================================================================

func _create_progress_bg() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = WELL
	_set_radius(style, RADIUS_SM)
	style.border_width_top = BORDER_WIDTH_INSET_TOP
	style.border_width_left = BORDER_WIDTH_INSET_SIDE
	style.border_width_right = BORDER_WIDTH_INSET_SIDE
	style.border_width_bottom = BORDER_WIDTH_INSET_SIDE
	style.border_color = with_alpha(Color.BLACK, INSET_BORDER_ALPHA_MED)
	return style


func _create_progress_fill() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = LAVENDER_DEEP
	_set_radius(style, RADIUS_SM)
	style.border_width_top = BORDER_WIDTH_ACCENT
	style.border_color = with_alpha(LAVENDER, LAVENDER_FOCUS_ALPHA)
	style.shadow_color = with_alpha(LAVENDER, LAVENDER_BORDER_ALPHA)
	style.shadow_size = SPACING_XS
	return style


func _create_slider_track() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = WELL
	_set_radius(style, SLIDER_RADIUS)
	style.border_width_top = BORDER_WIDTH_INSET_TOP
	style.border_width_left = BORDER_WIDTH_INSET_SIDE
	style.border_width_right = BORDER_WIDTH_INSET_SIDE
	style.border_width_bottom = BORDER_WIDTH_INSET_SIDE
	style.border_color = with_alpha(Color.BLACK, INSET_BORDER_ALPHA_MED)
	style.content_margin_top = SLIDER_PADDING_V
	style.content_margin_bottom = SLIDER_PADDING_V
	return style


func _create_slider_fill() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = LAVENDER_DEEP
	_set_radius(style, SLIDER_RADIUS)
	style.border_width_top = BORDER_WIDTH_ACCENT
	style.border_color = with_alpha(LAVENDER, LAVENDER_BORDER_ALPHA)
	style.content_margin_top = SLIDER_PADDING_V
	style.content_margin_bottom = SLIDER_PADDING_V
	return style


# =============================================================================
# STYLEBOX CREATORS - Misc
# =============================================================================

func _create_popup_style() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = SURFACE_ELEVATED
	_set_radius(style, RADIUS_LG)
	_set_border(style, BORDER_WIDTH_ACCENT)
	style.border_color = with_alpha(CREAM, RIM_LIGHT_STRONG_ALPHA)
	style.shadow_color = with_alpha(BG_BASE, SHADOW_ALPHA)
	style.shadow_size = SHADOW_SIZE_POPUP
	style.shadow_offset = Vector2(0, SHADOW_OFFSET_LG)
	_set_padding(style, SPACING_SM)
	return style


func _create_tooltip_style() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = SURFACE_ELEVATED
	_set_radius(style, RADIUS_MD)
	_set_border(style, BORDER_WIDTH_ACCENT)
	style.border_color = with_alpha(CREAM, RIM_LIGHT_ALPHA)
	style.shadow_color = with_alpha(BG_BASE, SHADOW_ALPHA)
	style.shadow_size = SHADOW_SIZE_SM
	style.shadow_offset = Vector2(0, SPACING_XS)
	_set_padding_hv(style, SPACING_MD, SPACING_SM)
	return style


func _create_scrollbar_grabber() -> StyleBoxFlat:
	# Raised surface grabber with subtle rim highlight
	var style := StyleBoxFlat.new()
	style.bg_color = SURFACE_ELEVATED
	_set_radius(style, RADIUS_PILL)
	# Subtle top rim highlight
	style.border_width_top = BORDER_WIDTH_ACCENT
	style.border_color = with_alpha(CREAM, RIM_LIGHT_ALPHA)
	# Content margin ensures minimum grabber size
	style.content_margin_left = 6
	style.content_margin_right = 6
	style.content_margin_top = 6
	style.content_margin_bottom = 6
	return style


func _create_scrollbar_grabber_hover() -> StyleBoxFlat:
	# Brighter on hover
	var style := StyleBoxFlat.new()
	style.bg_color = Color(SURFACE_ELEVATED.r + 0.05, SURFACE_ELEVATED.g + 0.05, SURFACE_ELEVATED.b + 0.05)
	_set_radius(style, RADIUS_PILL)
	style.border_width_top = BORDER_WIDTH_ACCENT
	style.border_color = with_alpha(CREAM, RIM_LIGHT_STRONG_ALPHA)
	style.content_margin_left = 6
	style.content_margin_right = 6
	style.content_margin_top = 6
	style.content_margin_bottom = 6
	return style


func _create_scrollbar_grabber_pressed() -> StyleBoxFlat:
	# Slightly darker when pressed
	var style := StyleBoxFlat.new()
	style.bg_color = SURFACE
	_set_radius(style, RADIUS_PILL)
	style.border_width_top = BORDER_WIDTH_ACCENT
	style.border_color = with_alpha(CREAM, RIM_LIGHT_ALPHA * 0.5)
	style.content_margin_left = 6
	style.content_margin_right = 6
	style.content_margin_top = 6
	style.content_margin_bottom = 6
	return style


# =============================================================================
# PUBLIC API - Dynamic Styles
# =============================================================================

## Returns a color with modified alpha (avoids verbose Color(c.r, c.g, c.b, alpha))
func with_alpha(color: Color, alpha: float) -> Color:
	return Color(color.r, color.g, color.b, alpha)


## Returns the color for a status string.
func get_status_color(status: String) -> Color:
	match status.to_lower():
		"success", "connected", "ready", "pass", "complete":
			return SUCCESS
		"warning", "attention", "pending", "error", "fail", "disconnected", "failed":
			return ERROR
		"info", "active", "running":
			return LAVENDER
		_:
			return CREAM_MUTED


## Returns the Label theme variation name for a status string.
## Use with label.theme_type_variation = AppTheme.get_status_label_variation(status)
func get_status_label_variation(status: String, small: bool = false) -> String:
	var prefix := "LabelSmall" if small else "Label"
	match status.to_lower():
		"success", "connected", "ready", "pass", "complete":
			return prefix + "Success"
		"warning", "attention", "pending", "error", "fail", "disconnected", "failed":
			return prefix + "Error"
		"info", "active", "running":
			return prefix + "Info"
		_:
			return "LabelDim" if not small else "LabelSmallDim"


## Returns the VBoxContainer theme variation for a given spacing constant.
## Use with vbox.theme_type_variation = AppTheme.get_vbox_variation(AppTheme.SPACING_MD)
func get_vbox_variation(spacing: int) -> String:
	match spacing:
		0:
			return "VBoxTight"
		SPACING_XS:
			return "VBoxXS"
		SPACING_SM:
			return "VBoxSM"
		SPACING_MD:
			return "VBoxMD"
		SPACING_LG:
			return "VBoxLG"
		SPACING_XL:
			return "VBoxXL"
		SPACING_2XL:
			return "VBox2XL"
		SPACING_3XL:
			return "VBox3XL"
		_:
			return ""  # Use default VBoxContainer


## Returns the HBoxContainer theme variation for a given spacing constant.
func get_hbox_variation(spacing: int) -> String:
	match spacing:
		0:
			return "HBoxTight"
		SPACING_XS:
			return "HBoxXS"
		SPACING_SM:
			return "HBoxSM"
		SPACING_MD:
			return "HBoxMD"
		SPACING_LG:
			return "HBoxLG"
		SPACING_XL:
			return "HBoxXL"
		SPACING_2XL:
			return "HBox2XL"
		_:
			return ""  # Use default HBoxContainer


# =============================================================================
# SHADER MATERIAL FACTORIES
# =============================================================================

## Creates a text glow shader material.
## Used by SectionHeader for the lavender glow effect.
func create_text_glow_material(glow_color: Color = LAVENDER, intensity: float = 0.4) -> ShaderMaterial:
	var mat := ShaderMaterial.new()
	mat.shader = TextGlowShader
	mat.set_shader_parameter("glow_color", with_alpha(glow_color, intensity))
	mat.set_shader_parameter("glow_size", TEXT_GLOW_SIZE)
	mat.set_shader_parameter("glow_intensity", TEXT_GLOW_INTENSITY)
	return mat


## Creates a raised surface shader material with full rim highlights.
## For cards, panels, nav bars with the convex raised appearance.
## Optional accent_border_color adds a colored inner border (e.g., for status indicators).
func create_raised_surface_material(
	base_color: Color = SURFACE_ELEVATED,
	corner_radius: float = float(RADIUS_2XL),
	accent_border_color: Color = Color.TRANSPARENT,
	rim_top_alpha: float = RIM_LIGHT_ALPHA
) -> ShaderMaterial:
	var mat := ShaderMaterial.new()
	mat.shader = RaisedSurfaceShader
	mat.set_shader_parameter("base_color", base_color)
	mat.set_shader_parameter("corner_radius", corner_radius)
	mat.set_shader_parameter("gradient_intensity", RAISED_GRADIENT_INTENSITY)
	mat.set_shader_parameter("rim_top_color", with_alpha(CREAM, rim_top_alpha))
	mat.set_shader_parameter("rim_bottom_color", with_alpha(Color.BLACK, RIM_DARK_ALPHA))
	mat.set_shader_parameter("rim_bottom_highlight", with_alpha(CREAM, RIM_BOTTOM_HIGHLIGHT_ALPHA))
	if accent_border_color.a > 0.0:
		mat.set_shader_parameter("accent_border_color", accent_border_color)
		mat.set_shader_parameter("accent_border_width", float(BORDER_WIDTH_ACCENT))
	return mat


## Creates an inset surface shader material for input fields.
## For text inputs, spinboxes, dropdowns with the recessed appearance.
func create_inset_surface_material(corner_radius: float = float(RADIUS_SM)) -> ShaderMaterial:
	var mat := ShaderMaterial.new()
	mat.shader = InsetSurfaceShader
	mat.set_shader_parameter("base_color", WELL)
	mat.set_shader_parameter("cream", CREAM)
	mat.set_shader_parameter("corner_radius", corner_radius)
	return mat


## Creates a ColorRect with inset surface shader for input components.
## Returns [ColorRect, ShaderMaterial] for caller to store references.
## The ColorRect is configured with PRESET_FULL_RECT and MOUSE_FILTER_IGNORE.
func create_inset_shader_background(corner_radius: float = float(RADIUS_SM)) -> Array:
	var material := create_inset_surface_material(corner_radius)
	var bg := ColorRect.new()
	bg.name = "ShaderBG"
	bg.color = Color.WHITE  # Shader will override
	bg.set_anchors_preset(Control.PRESET_FULL_RECT)
	bg.mouse_filter = Control.MOUSE_FILTER_IGNORE
	bg.material = material
	return [bg, material]


## Creates a rounded mask shader material for preview wells.
## For camera/stimulus preview areas with inset clipping.
func create_rounded_mask_material(
	corner_radius: float = float(RADIUS_MD),
	rim_base_color: Color = WELL
) -> ShaderMaterial:
	var mat := ShaderMaterial.new()
	mat.shader = RoundedMaskShader
	mat.set_shader_parameter("corner_radius", corner_radius)
	mat.set_shader_parameter("rim_base_color", rim_base_color)
	mat.set_shader_parameter("rim_top_color", with_alpha(Color.BLACK, 0.3))
	mat.set_shader_parameter("rim_bottom_color", with_alpha(CREAM, 0.15))
	return mat


## Creates a button shader material with all color parameters initialized.
## The button shader supports two modes (secondary/nightlight) and four states
## (normal/hover/pressed/disabled). State changes are handled by the button component.
func create_button_material() -> ShaderMaterial:
	var mat := ShaderMaterial.new()
	mat.shader = ButtonShader
	# Surface colors for each button state
	mat.set_shader_parameter("surface", SURFACE_COLOR_BUTTON)
	mat.set_shader_parameter("surface_elevated", SURFACE_COLOR_BUTTON_HOVER)
	mat.set_shader_parameter("surface_disabled", SURFACE_COLOR_BUTTON_DISABLED)
	mat.set_shader_parameter("surface_recessed", SURFACE_COLOR_BUTTON_PRESSED)
	# Shared colors
	mat.set_shader_parameter("well", WELL)
	mat.set_shader_parameter("cream", CREAM)
	mat.set_shader_parameter("lavender_deep", LAVENDER_DEEP)
	# Amber colors for nightlight mode
	mat.set_shader_parameter("amber", AMBER)
	mat.set_shader_parameter("amber_glow", AMBER_GLOW)
	mat.set_shader_parameter("amber_bright", AMBER_BRIGHT)
	mat.set_shader_parameter("amber_pale", AMBER_PALE)
	mat.set_shader_parameter("amber_glow_pale", AMBER_GLOW_PALE)
	# Initial state
	mat.set_shader_parameter("mode", 0)
	mat.set_shader_parameter("button_state", 0)
	mat.set_shader_parameter("rim_mode", 0)
	mat.set_shader_parameter("glow_intensity", BUTTON_GLOW_NORMAL)
	mat.set_shader_parameter("corner_radius", RADIUS_MD)
	return mat


# =============================================================================
# PRIVATE HELPERS
# =============================================================================

func _set_radius(style: StyleBoxFlat, radius: int) -> void:
	style.corner_radius_top_left = radius
	style.corner_radius_top_right = radius
	style.corner_radius_bottom_left = radius
	style.corner_radius_bottom_right = radius


func _set_border(style: StyleBoxFlat, width: int) -> void:
	style.border_width_top = width
	style.border_width_left = width
	style.border_width_bottom = width
	style.border_width_right = width


func _set_padding(style: StyleBoxFlat, padding: int) -> void:
	style.content_margin_top = padding
	style.content_margin_left = padding
	style.content_margin_bottom = padding
	style.content_margin_right = padding


func _set_padding_hv(style: StyleBoxFlat, horizontal: int, vertical: int) -> void:
	style.content_margin_top = vertical
	style.content_margin_bottom = vertical
	style.content_margin_left = horizontal
	style.content_margin_right = horizontal
