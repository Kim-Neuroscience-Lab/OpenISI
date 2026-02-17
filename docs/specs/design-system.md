# OpenISI Design System

## Sleep Punk Night + Ceramic

A warm, cozy aesthetic inspired by soft ceramic surfaces with gentle light and shadow.

---

## Color Palette

### Foundation (The Darkness)

| Name | Hex | Usage |
|------|-----|-------|
| `BG_BASE` | `#13111a` | Deep night with violet undertone - base of everything |
| `SURFACE` | `#1c1824` | Primary container color - default button normal state |
| `SURFACE_ELEVATED` | `#241f2e` | Floating elements, cards, hover states |
| `SURFACE_DISABLED` | `#18141f` | Darker than SURFACE - disabled button state |
| `SURFACE_RECESSED` | `#131119` | Darkest button color - pressed button state |
| `WELL` | `#0f0d14` | Soft recesses - input fields, inset areas |

**Brightness order (light to dark):**
```
SURFACE_ELEVATED > SURFACE > SURFACE_DISABLED > SURFACE_RECESSED > WELL
```

### Cream (The Light Sources)

| Name | Hex | Usage |
|------|-----|-------|
| `CREAM` | `#f0e6dc` | Primary text, key glowing elements |
| `CREAM_MUTED` | `#c4b8ab` | Secondary text, labels, pressed/disabled button text |
| `CREAM_DIM` | `#7a7067` | Placeholders, hints |

### Lavender (Ambient Mood)

| Name | Hex | Usage |
|------|-----|-------|
| `LAVENDER` | `#b8a5c9` | Primary accent color |
| `LAVENDER_DIM` | `#8677a3` | Hover states, secondary accents |
| `LAVENDER_DEEP` | `#6b5a8a` | Borders, slider fills |

### Amber (The Nightlight)

| Name | Hex | Usage |
|------|-----|-------|
| `AMBER` | `#e8c49a` | Primary/highlighted buttons |
| `AMBER_GLOW` | `#d4a574` | Deeper amber gradients |
| `AMBER_BRIGHT` | `#fad8ae` | Top of amber gradient (computed) |
| `AMBER_PALE` | `#a89080` | Disabled amber button - "light is off" |
| `AMBER_GLOW_PALE` | `#8a7868` | Disabled amber glow |

### Text on Amber

| Name | Hex | Usage |
|------|-----|-------|
| `BG_BASE` | `#13111a` | Normal/hover amber button text |
| `BG_BASE_MUTED` | `#2a2535` | Pressed/disabled amber button text |

### Status (Gentle, Not Alarming)

| Name | Hex | Usage |
|------|-----|-------|
| `SUCCESS` | `#a8c4b0` | Soft sage - connected, ready |
| `WARNING` | `#e8c49a` | Amber - attention needed |
| `ERROR` | `#c9a9a9` | Dusty rose - something's wrong |
| `INFO` | `#b8a5c9` | Lavender - neutral informational |

---

## Ceramic Styling

### Rim Highlights

All ceramic surfaces have consistent rim highlights that simulate light catching glazed edges:

| Position | Effect | Alpha |
|----------|--------|-------|
| **Top** | Bright cream highlight | `RIM_LIGHT_ALPHA` (0.08) or `RIM_LIGHT_STRONG_ALPHA` (0.12) |
| **Bottom** | Dark shadow | `RIM_DARK_ALPHA` (0.2) |
| **Bottom (subtle)** | Faint cream highlight | `RIM_BOTTOM_HIGHLIGHT_ALPHA` (0.05) |

### Gradient

Bidirectional gradient suggests gentle curvature:
- Top: Lightens by `CERAMIC_GRADIENT_INTENSITY` (0.03)
- Bottom: Darkens by same amount

### Rim Width

| Name | Value | Usage |
|------|-------|-------|
| `CERAMIC_RIM_WIDTH` | 0.03 | Rim width as percentage of element height |

### Base Colors by Component

| Component | Base Color | Notes |
|-----------|------------|-------|
| Cards, Panels, Info Cards | `CERAMIC_BASE_CARD` = `SURFACE_ELEVATED` | Floating/elevated |
| Nav Bar Container | `CERAMIC_BASE_NAV` = `SURFACE` | Surface-level |
| Status Pills/Badges | `CERAMIC_BASE_STATUS` = `SURFACE` | Surface-level |
| Default Buttons | `CERAMIC_BASE_BUTTON` = `SURFACE` | Surface-level |
| Checkbox Tile (selected) | `CERAMIC_BASE_CHECKBOX_ON` = `SURFACE_ELEVATED` | Raised |
| Checkbox Tile (unselected) | `CERAMIC_BASE_CHECKBOX_OFF` = `WELL` | Recessed |

---

## Buttons

### Design Principles

1. **Fully content-aware sizing** - button dimensions are determined entirely by text content plus internal padding (`BUTTON_PADDING_H/V`). External code should never set explicit sizes on buttons.
2. **No outer drop shadows** - buttons rely on rim highlights and base color for depth
3. **Rim highlights match cards** - consistent ceramic feel
4. **States follow intuitive physics:**
   - Hover "lifts" → brighter base
   - Pressed "sinks" → darker base, loses bright rim
   - Disabled "dims" → darker base, muted text

### Default Button Family

| State | Base Color | Rim | Text |
|-------|------------|-----|------|
| **Normal** | `SURFACE` | Full rim (bright top, dark bottom) | `CREAM` |
| **Hover** | `SURFACE_ELEVATED` | Same full rim | `CREAM` |
| **Pressed** | `SURFACE_RECESSED` | **Dark only** (no bright highlight) | `CREAM_MUTED` |
| **Disabled** | `SURFACE_DISABLED` | Same full rim | `CREAM_MUTED` |

**Control Variant:** Same styles, allows smaller size + smaller text (for +/- buttons, etc.)

### Amber Button Family

| State | Base Color | Glow | Rim | Text |
|-------|------------|------|-----|------|
| **Normal** | Amber gradient | Very subtle outer glow | Full rim | `BG_BASE` |
| **Hover** | Brighter amber | More pronounced glow | Same full rim | `BG_BASE` |
| **Pressed** | Darker amber | Pulled in/contained | **Dark only** | `BG_BASE_MUTED` |
| **Disabled** | Pale/muted amber | None | Same full rim | `BG_BASE_MUTED` |

**Amber Gradient (Normal):**
```
Top: AMBER_BRIGHT
Middle (40%): AMBER
Bottom: AMBER_GLOW
```

**Amber Glow:**
- Normal: `0 0 24px rgba(232,196,154,0.15)` (very subtle)
- Hover: `0 0 32px rgba(232,196,154,0.25)` (more pronounced)
- Pressed: Contained/pulled in
- Disabled: None

### Button Glow Constants

| Name | Value | Usage |
|------|-------|-------|
| `BUTTON_GLOW_NORMAL` | 0.15 | Very subtle glow for normal state |
| `BUTTON_GLOW_HOVER` | 0.25 | More pronounced glow for hover |
| `BUTTON_GLOW_PRESSED` | 0.08 | Pulled in glow for pressed state |
| `BUTTON_GLOW_DISABLED` | 0.0 | No glow for disabled state |

---

## Input Wells

Inputs appear recessed into the surface:

- **Background:** `WELL` with darkening gradient
- **Shadow:** Inset shadow (dark at top, simulating depth)
- **Rim:** Subtle bottom highlight (light catches the far edge)
- **Border:** Dark, simulating the lip of the recess

---

## Cards & Panels

### Main Cards (BaseCard)

- **Base:** `SURFACE_ELEVATED`
- **Shadow:** `0 8px 32px` at `SHADOW_ALPHA` (0.6)
- **Rim:** Full ceramic rim (bright top, dark bottom)
- **Radius:** `RADIUS_2XL` (20px)

### Info Cards (nested)

- **Base:** `SURFACE_ELEVATED` (same as parent cards)
- **Shadow:** Smaller (`0 6px 20px`)
- **Rim:** Same ceramic rim
- **Radius:** `RADIUS_LG` (14px)

### Nav Bar Container

- **Base:** `SURFACE` (darker than cards)
- **Shadow:** Same as cards
- **Rim:** Same ceramic rim
- **Radius:** `RADIUS_NAV` (22px) - pill-like

---

## Status Indicators

### Status Pill (Header)

- **Base:** `SURFACE`
- **Rim:** Ceramic rim
- **Dot:** Status color with glow
- **Border:** Subtle status color tint

### Status Badge

- **Base:** `SURFACE`
- **Rim:** Ceramic rim
- **Border:** Status color at `BORDER_ACCENT_ALPHA` (0.25)

---

## Phase Indicator

### Container
- Uses Nav Bar Container style (`SURFACE`)

### Active Pill
- **Base:** `SURFACE_ELEVATED`
- **Shadow:** Card-like
- **Rim:** Full ceramic rim (strong)
- **Text:** `CREAM`, Medium weight

### Inactive Pills
- **Base:** Transparent
- **Text:** `CREAM_DIM`, Regular weight
- **Hover:** Subtle `SURFACE` background

---

## Checkbox Tiles

| State | Base Color | Rim | Indicator | Label |
|-------|------------|-----|-----------|-------|
| **Unselected** | `WELL` | Inset shadow | Transparent + `CREAM_DIM` border | `CREAM_MUTED` |
| **Selected** | `SURFACE_ELEVATED` | Full ceramic (strong) | `LAVENDER` fill + `BG_BASE` checkmark | `CREAM` |

### Indicator Box
- **Size:** 20x20px
- **Border Radius:** `RADIUS_SM`
- **Unselected:** 2px `CREAM_DIM` border, no fill
- **Selected:** `LAVENDER` fill with subtle glow, `BG_BASE` checkmark centered

---

## Dividers

Subtle ceramic ridge effect:
```
Top: Dark line (shadow)
Middle: Transparent
Bottom: Light line (rim highlight)
```

---

## Progress Bars

Progress bars follow the slider pattern with lavender fill:

- **Track:** `WELL` (recessed)
- **Fill:** `LAVENDER_DEEP` (same as slider fills)
- **Border Radius:** `RADIUS_PILL` (fully rounded)

---

## Typography

| Variation | Size | Weight | Color |
|-----------|------|--------|-------|
| `LabelDisplay` | 24px | SemiBold (600) | `CREAM` |
| `LabelTitle` | 20px | Medium (500) | `CREAM` |
| `LabelHeading` | 15px | Medium (500) | `CREAM_MUTED` |
| `LabelBody` | 14px | Regular (400) | `CREAM` |
| `LabelCaption` | 13px | Regular (400) | `CREAM_MUTED` |
| `LabelSmall` | 12px | Regular (400) | `CREAM_MUTED` |
| `LabelSection` | 11px | SemiBold (600) | `LAVENDER` (uppercase) |
| `LabelMono` | 13px | Regular (400) | `CREAM` |

### Button Text

| Name | Size | Notes |
|------|------|-------|
| `FONT_BUTTON` | 13px | Consistent across all button states and modes |

---

## Spacing

Based on 8px grid:

| Name | Value |
|------|-------|
| `SPACING_XS` | 4px |
| `SPACING_SM` | 8px |
| `SPACING_MD` | 12px |
| `SPACING_LG` | 16px |
| `SPACING_XL` | 20px |
| `SPACING_2XL` | 24px |
| `SPACING_3XL` | 32px |

### Padding

| Name | Value | Usage |
|------|-------|-------|
| `INPUT_PADDING_V` | 14px | Standard input vertical padding |
| `INPUT_PADDING_H` | 16px | Standard input horizontal padding |
| `SPINBOX_PADDING_V` | 6px | SpinBox vertical padding (compact) |
| `SPINBOX_PADDING_H` | 8px | SpinBox horizontal padding (compact) |
| `BUTTON_PADDING_V` | 18px | Button internal vertical padding (edge to text) |
| `BUTTON_PADDING_H` | 24px | Button internal horizontal padding (edge to text) |

---

## Component Sizes

### Layout Sizes

| Name | Value | Usage |
|------|-------|-------|
| `INPUT_HEIGHT` | 40px | Standard input height |
| `HEADER_HEIGHT` | 60px | App header |
| `FOOTER_HEIGHT` | 80px | App footer |
| `CARD_MIN_WIDTH` | 200px | Minimum card width |
| `CARD_MIN_HEIGHT` | 100px | Minimum card height (standard card) |
| `INFO_CARD_MIN_HEIGHT` | 80px | Minimum info card height |
| `CARD_WIDTH_MD` | 500px | Medium card width (summary, output) |
| `INPUT_SPINBOX_WIDTH` | 100px | SpinBox input width |
| `LABEL_WIDTH_SM` | 100px | Small label width for form alignment |
| `LABEL_WIDTH_MD` | 120px | Medium label width for form alignment |

### Preview & Content Areas

| Name | Value | Usage |
|------|-------|-------|
| `PREVIEW_HEIGHT_SM` | 200px | Small preview areas |
| `PREVIEW_HEIGHT_MD` | 350px | Medium preview areas |
| `PROGRESS_BAR_HEIGHT` | 24px | Progress bar height |
| `THUMB_WELL_WIDTH` | 100px | Thumbnail well width |
| `THUMB_WELL_HEIGHT` | 75px | Thumbnail well height |

### Indicator Sizes

| Name | Value | Usage |
|------|-------|-------|
| `STATUS_DOT_SIZE` | 8px | Status indicator dots |
| `CHECKBOX_INDICATOR_SIZE` | 20px | Checkbox tile indicator box |
| `PHASE_PILL_MIN_WIDTH` | 80px | Phase indicator pill minimum width |
| `PHASE_PILL_HEIGHT` | 32px | Phase indicator pill height |
| `STATUS_PILL_MIN_WIDTH` | 100px | Status pill minimum width |
| `STATUS_PILL_HEIGHT` | 36px | Status pill height |
| `SCROLLBAR_WIDTH` | 16px | Standard scrollbar width for spacing calculations |
| `DIVIDER_LINE_HEIGHT` | 1px | Divider line thickness |

### Shadow Values

| Name | Value | Usage |
|------|-------|-------|
| `SHADOW_SIZE_XS` | 4px | Extra small element shadows (checkbox) |
| `SHADOW_SIZE_SM` | 16px | Small element shadows (tooltips) |
| `SHADOW_SIZE_MD` | 20px | Medium element shadows (info cards) |
| `SHADOW_SIZE_POPUP` | 24px | Popup shadows |
| `SHADOW_SIZE_LG` | 32px | Large element shadows (cards, panels) |
| `SHADOW_SIZE_XL` | 48px | Extra large shadows (modals) |
| `SHADOW_OFFSET_SM` | 2px | Small elements (checkbox) |
| `SHADOW_OFFSET_MD` | 6px | Medium elements (info cards) |
| `SHADOW_OFFSET_LG` | 8px | Large elements (cards, panels) |
| `SHADOW_OFFSET_XL` | 16px | Extra large (modals) |

### Shadow Alphas

| Name | Value | Usage |
|------|-------|-------|
| `SHADOW_ALPHA` | 0.6 | Standard shadow alpha (cards, panels) |
| `SHADOW_ALPHA_LIGHT` | 0.5 | Lighter shadow alpha (info cards) |
| `SHADOW_ALPHA_SUBTLE` | 0.3 | Subtle shadow alpha (small elements) |
| `SHADOW_ALPHA_MODAL` | 0.7 | Modal shadow alpha (stronger for floating effect) |

---

## Border Radii

| Name | Value | Usage |
|------|-------|-------|
| `RADIUS_SM` | 6px | Small interior elements |
| `RADIUS_MD` | 12px | Inputs, small buttons |
| `RADIUS_LG` | 14px | Primary buttons |
| `RADIUS_XL` | 16px | Cards, preview areas |
| `RADIUS_2XL` | 20px | Main panels |
| `RADIUS_NAV` | 22px | Nav container (pill-like) |
| `RADIUS_PILL` | 9999px | Fully rounded pills |

---

## Border Widths

| Name | Value | Usage |
|------|-------|-------|
| `BORDER_WIDTH_ACCENT` | 1px | Accent borders (phase pill, modals) |
| `BORDER_WIDTH_CHECKBOX` | 2px | Checkbox indicator border |
| `BORDER_WIDTH_INSET_TOP` | 2px | Inset top border (wells, inputs) |
| `BORDER_WIDTH_INSET_SIDE` | 1px | Inset side/bottom border |

---

## Inset & Focus Alphas

### Inset Border Alphas

| Name | Value | Usage |
|------|-------|-------|
| `INSET_BORDER_ALPHA_STRONG` | 0.6 | Well/input inset shadow |
| `INSET_BORDER_ALPHA_DARK` | 0.5 | Input normal border |
| `INSET_BORDER_ALPHA_MED` | 0.3 | Progress/slider borders |
| `INSET_BORDER_ALPHA_LIGHT` | 0.2 | Pressed button border |

### Lavender Accent Alphas

| Name | Value | Usage |
|------|-------|-------|
| `LAVENDER_FOCUS_ALPHA` | 0.3 | Input focus border |
| `LAVENDER_GLOW_ALPHA` | 0.15 | Input focus shadow/glow |
| `LAVENDER_BORDER_ALPHA` | 0.2 | Progress/slider fill borders |

---

## Slider Styling

| Name | Value | Usage |
|------|-------|-------|
| `SLIDER_RADIUS` | 3px | Slider track/fill corner radius |
| `SLIDER_PADDING_V` | 3px | Slider vertical content margin |

---

## Nav Container

| Name | Value | Usage |
|------|-------|-------|
| `NAV_PADDING_V` | 6px | Nav bar vertical padding |

---

## Text Glow Effect

| Name | Value | Usage |
|------|-------|-------|
| `TEXT_GLOW_SIZE` | 20.0 | Text glow blur radius |
| `TEXT_GLOW_INTENSITY` | 1.0 | Text glow intensity multiplier |

---

## Focus Phase Defaults

| Name | Value | Usage |
|------|-------|-------|
| `FOCUS_DEFAULT_EXPOSURE_US` | 30000 | Initial exposure in microseconds |
| `FOCUS_RING_RADIUS_DEFAULT` | 150 | Default head ring radius |
| `FOCUS_PREVIEW_CENTER` | 256 | Preview center coordinate (512/2) |

---

## Animation Timing

| Name | Value | Usage |
|------|-------|-------|
| `ANIM_MICRO` | 0.15s | Hover, focus |
| `ANIM_STATE` | 0.25s | State changes |
| `ANIM_PANEL` | 0.35s | Panel transitions |
| `ANIM_PULSE` | 3.0s | Status pulse (full cycle) |

---

## Element Map

### Containers
| Element | Base Color | Notes |
|---------|------------|-------|
| App Header/Footer | Transparent | No background |
| Main Cards | `SURFACE_ELEVATED` | Floating/elevated |
| Info Cards | `SURFACE_ELEVATED` | Same as main cards |
| Nav Bar Container | `SURFACE` | Surface-level |

### Interactive Elements
| Element | Base Color | Notes |
|---------|------------|-------|
| Default Buttons | `SURFACE` | See button states |
| Amber Buttons | Amber gradient | See button states |
| Input Fields | `WELL` | Recessed |
| Checkbox Tile (selected) | `SURFACE_ELEVATED` | Raised |
| Checkbox Tile (unselected) | `WELL` | Recessed |

### Indicators
| Element | Base Color | Notes |
|---------|------------|-------|
| Status Pills/Badges | `SURFACE` | Surface-level |
| Phase Indicator Container | `SURFACE` | Uses nav bar style |
| Active Phase Pill | `SURFACE_ELEVATED` | Raised |
| Inactive Phase Pills | Transparent | Text only |
| Slider/Progress Fill | `LAVENDER_DEEP` | Accent color |

### Deferred
- Overlays/Popups: Not currently implemented

---

## Implementation Notes

### SSoT (Single Source of Truth)

All styling values are defined in `src/autoload/theme.gd`. Components reference these constants directly - no hardcoded values, no inference.

### Shader Paths

All shader paths are defined as constants in `theme.gd`:

| Constant | Path |
|----------|------|
| `SHADER_BUTTON` | `res://src/ui/theme/shaders/button.gdshader` |
| `SHADER_CERAMIC` | `res://src/ui/theme/shaders/ceramic.gdshader` |
| `SHADER_INPUT_FIELD` | `res://src/ui/theme/shaders/input_field.gdshader` |
| `SHADER_TEXT_GLOW` | `res://src/ui/theme/shaders/text_glow.gdshader` |
| `SHADER_CERAMIC_GRADIENT` | `res://src/ui/theme/shaders/ceramic_gradient.gdshader` |

### Font Paths

All font paths are defined as constants in `theme.gd`:

| Constant | Path |
|----------|------|
| `FONT_PATH_REGULAR` | `res://assets/fonts/PlusJakartaSans-VariableFont_wght.ttf` |
| `FONT_PATH_ITALIC` | `res://assets/fonts/PlusJakartaSans-Italic-VariableFont_wght.ttf` |
| `FONT_PATH_MEDIUM` | `res://assets/fonts/PlusJakartaSans-Medium.ttf` |
| `FONT_PATH_SEMIBOLD` | `res://assets/fonts/PlusJakartaSans-SemiBold.ttf` |
| `FONT_PATH_BOLD` | `res://assets/fonts/PlusJakartaSans-Bold.ttf` |

### Ceramic Shader

The `ceramic.gdshader` provides:
- Bidirectional gradient (lighter top, darker bottom)
- Rim highlights (top and bottom)
- Configurable base color, corner radius, intensities

Components set `_ceramic_base_color` explicitly before shader setup.

### Button Shader

The `button.gdshader` handles:
- Default button states (normal, hover, pressed)
- Amber button mode with glow effects
- Neumorphic inset effect around buttons
