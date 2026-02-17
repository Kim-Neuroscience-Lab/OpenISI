# OpenISI Design System

## Sleep Punk Night + Ceramic

---

## Philosophy

### The Feeling

**Sleep Punk at 4am.** You're cocooned in warmth and softness. The darkness isn't absence — it's a blanket. You know dawn is coming eventually, you can almost feel its warmth at the edges, but right now you're wrapped in the quiet.

This is the future, and it was gentle.

### Three Qualities

**1. Luminous Softness**
Light emanates from within rather than illuminating from outside. Elements glow softly. There are no harsh shadows — only soft halos and ambient presence. The UI is a collection of warm light sources floating in comfortable darkness.

**2. Tactile Softness**
Things feel like they would yield slightly if pressed. Generous curves, pillowy forms, surfaces that suggest depth without hardness. The visual weight of a cushion, not a tile.

**3. Ceramic Density**
Despite the softness, elements have *mass*. They're solid but not hard. Think of the satisfying "thock" of ceramic keycaps — there's substance and weight, but also smoothness and gentle curvature. Handmade quality. Glazed edges that catch light.

### What We're Avoiding

- Cold tech aesthetics
- Sharp edges and harsh contrasts
- Pure blacks and pure whites
- Industrial/masculine feeling
- Weightless floating (too thin)
- Heavy carved shadows (too hard)

---

## Color Palette

### Foundation — The Darkness

Warm, plum-shifted darks. Never pure gray or blue-gray — always maintain a slight violet/mauve undertone. The darkness should feel like being wrapped in a soft blanket, not sitting in a server room.

| Token | Hex | Description |
|-------|-----|-------------|
| **Background** | `#13111a` | Deep night with violet undertone. The base of everything. |
| **Surface** | `#1c1824` | Like fleece in shadow. Primary container color. |
| **Surface Elevated** | `#241f2e` | Floating elements, one step up. Modals, cards. |
| **Well** | `#0f0d14` | Soft recesses. Input fields, inset areas. Sinking into cushions. |

### Cream — The Light Sources

Warm off-whites that glow like candlelight. These are the primary light sources in the UI. Never use pure white — it's too harsh for dark rooms and breaks the cozy feeling.

| Token | Hex | Description |
|-------|-----|-------------|
| **Cream** | `#f0e6dc` | Warm candlelight. Primary text, key glowing elements. |
| **Cream Muted** | `#c4b8ab` | Soft, receded. Secondary text, labels. |
| **Cream Dim** | `#7a7067` | Distant warmth. Placeholders, hints, disabled states. |

### Lavender — Ambient Mood

Soft purple that provides ambient presence throughout the interface. Like a dim LED strip in a cozy room, or the last light of twilight. Used for accents, selected states, and section identity.

| Token | Hex | Description |
|-------|-----|-------------|
| **Lavender** | `#b8a5c9` | Soft purple glow. Primary accent color. |
| **Lavender Dim** | `#8677a3` | Muted. Hover states, secondary accents. |
| **Lavender Deep** | `#6b5a8a` | Rich purple. Borders, slider fills, subtle accents. |

### Amber — The Nightlight

The one warm beacon. Use sparingly — this is the color that draws the eye, the primary action, the thing that says "here." Like a single warm nightlight in a dim room.

| Token | Hex | Description |
|-------|-----|-------------|
| **Amber** | `#e8c49a` | Warm gold. Primary buttons, important values. |
| **Amber Glow** | `#d4a574` | Deeper amber. Gradients, warm accents. |

### Status — Gentle, Not Alarming

In a dark room during an experiment, you don't want a screaming red error. Status colors should inform without startling. Muted, soft, but still communicative.

| Token | Hex | Description |
|-------|-----|-------------|
| **Success** | `#a8c4b0` | Soft sage. "All is well." Connected, ready. |
| **Warning** | `#e8c49a` | Amber (shared). "Attention needed." |
| **Error** | `#c9a9a9` | Dusty rose. "Something's wrong." Not aggressive. |
| **Info** | `#b8a5c9` | Lavender (shared). Neutral informational. |

### Rim Light — Ceramic Highlights

The subtle highlight that catches on the top edge of ceramic surfaces. Creates that glazed-edge quality.

| Token | Value | Description |
|-------|-------|-------------|
| **Rim Light** | Cream at 8% opacity | Default ceramic rim highlight |
| **Rim Light Strong** | Cream at 12% opacity | Elevated or focused elements |

---

## Ceramic Styling Principles

### 1. Vertical Gradients (Curvature)

Every raised surface has a subtle vertical gradient suggesting gentle convex curvature — lighter at top, darker at bottom. Like light falling softly on a ceramic bowl or the top of a keycap.

- **Top**: Base color lightened ~5%
- **Middle**: Base color
- **Bottom**: Base color darkened ~5%

The effect should be subtle — you feel it more than see it. It creates the sense that surfaces have gentle dimension without being overtly 3D.

### 2. Rim Highlights (Glazed Edges)

The top edge of ceramic catches light. Every raised element should have a subtle 1px lighter line along its top edge — like light catching the rim of a glazed ceramic piece.

- Cream at 8% opacity for normal elements
- Cream at 12% opacity for elevated or focused elements
- This highlight is interior to the element, not a border

### 3. Soft Recesses (Wells)

Input fields and inset areas feel like pressing into soft ceramic. They're darker than their surroundings, with a gentle inner shadow from above, and often a subtle light catching the bottom interior edge.

- Darker fill color (well color)
- Soft shadow from top interior
- Optional: very subtle rim light on bottom interior edge

### 4. Outer Glow (Floating in Darkness)

Panels don't cast hard drop shadows. Instead, they have soft, diffuse glows that suggest floating in darkness. The glow is large, heavily blurred, and uses near-black with the same violet undertone as the background.

- Large blur radius (24-40px)
- Positioned slightly below the element
- Very dark, subtle — creates depth without weight
- Optional: extremely subtle cream glow (1px, very low opacity) to suggest the element is a light source

### 5. No Hard Edges

Nothing should have sharp corners or abrupt transitions. Everything is rounded, everything fades gently. Borders, when present, are subtle and low-contrast.

---

## Typography

### Font Selection

**Primary font**: A humanist sans-serif with warmth and roundness. The font should feel friendly and approachable, not cold or overly geometric.

Good choices: Plus Jakarta Sans, Nunito, Outfit, Source Sans 3

Avoid: Geometric fonts (Futura, Montserrat), monospace for body text, decorative fonts

**Monospace font**: For technical values, file paths, timestamps, and data. Should still feel relatively warm.

Good choices: JetBrains Mono, IBM Plex Mono

### Type Scale

| Role | Size | Weight | Use |
|------|------|--------|-----|
| **Display** | 24px | SemiBold (600) | Phase headers, major titles |
| **Title** | 20px | Medium (500) | Panel headers |
| **Heading** | 15px | Medium (500) | Section titles, large button text |
| **Body** | 14px | Regular (400) | Default text, form labels |
| **Caption** | 13px | Regular (400) | Secondary labels, input text |
| **Small** | 12px | Regular (400) | Hints, metadata, table values |
| **Tiny** | 11px | SemiBold (600) | Section headers (uppercase, tracked) |
| **Mono** | 13px | Regular (400) | Values, paths, technical data |

### Text Colors by Context

| Context | Color |
|---------|-------|
| Primary text (headings, labels) | Cream |
| Secondary text (descriptions, secondary labels) | Cream Muted |
| Tertiary text (placeholders, hints, disabled) | Cream Dim |
| Section headers | Lavender (with subtle glow) |
| Important values | Amber |
| Data values | Cream |
| Links / interactive text | Lavender |

### Section Headers

Uppercase, tracked (increased letter spacing), in lavender with a subtle text glow. These provide visual rhythm and help organize dense control panels.

---

## Spacing

### Base Unit

**4px** is the base unit. All spacing should be multiples of 4px.

### Scale

| Token | Value | Use |
|-------|-------|-----|
| **XS** | 4px | Tight gaps, icon internal padding |
| **SM** | 8px | Small gaps, inline element spacing |
| **MD** | 12px | Default gaps between related items |
| **LG** | 16px | Section padding, input internal padding |
| **XL** | 20px | Panel padding, larger gaps |
| **2XL** | 24px | Main panel padding, section separation |
| **3XL** | 32px | Major section separation, breathing room |

### Generous Margins

Sleep Punk requires *ma* — the Japanese concept of meaningful negative space. When in doubt, add more space, not more decoration. Let elements breathe.

### Common Spacing Patterns

| Context | Value |
|---------|-------|
| Main panel padding | 24px |
| Card/info box padding | 14-16px |
| Input field padding | 14px vertical, 16px horizontal |
| Large button padding | 18px vertical, 24px horizontal |
| Small button/pill padding | 8-10px vertical, 14-16px horizontal |
| Gap between form fields | 16px |
| Gap between sections (with divider) | 24px |
| Grid item gaps (checkboxes, etc.) | 8px |
| Gap between status items | 20px |

---

## Border Radii

Generous radii throughout for that soft, pillowy ceramic feel. Nothing should have sharp corners.

| Token | Value | Use |
|-------|-------|-----|
| **SM** | 6px | Small interior elements, checkbox indicators |
| **MD** | 12px | Inputs, small buttons, checkbox tiles |
| **LG** | 14px | Primary buttons, dropdown items |
| **XL** | 16-18px | Cards, preview areas |
| **2XL** | 20px | Main panels, large containers |
| **Pill** | 9999px | Fully rounded pills, phase indicator container |

---

## Component Styling

### Panels (Main Containers)

Large containers that hold content sections.

- **Fill**: Surface Elevated with vertical gradient (curvature)
- **Border**: 1px, Lavender Deep at very low opacity (~6%)
- **Corner radius**: 20px
- **Outer glow**: Large, soft, dark, positioned below
- **Rim highlight**: 1px interior top edge, Cream at 8%
- **Padding**: 24px

### Input Fields

Text inputs, number inputs, dropdowns.

- **Fill**: Well color with vertical gradient (subtle inward curve)
- **Border**: 1px, Lavender Deep at low opacity (~8%)
- **Corner radius**: 12px
- **Inner shadow**: Soft shadow from top, creating recessed feeling
- **Bottom rim**: Very subtle light on interior bottom edge
- **Padding**: 14px vertical, 16px horizontal
- **Text**: Cream (or Cream Dim for placeholder)
- **Focus state**: Border brightens (Lavender at ~30%), subtle outer glow

### Primary Button (Amber)

The main call-to-action. The nightlight.

- **Fill**: Amber with vertical gradient (convex ceramic)
- **Corner radius**: 14px
- **Outer glow**: Warm amber glow, noticeable but not harsh
- **Outer shadow**: Soft dark shadow below
- **Rim highlight**: Interior top edge, white at ~20%
- **Bottom rim**: Interior bottom edge, black at ~10%
- **Padding**: 18px vertical, 24px horizontal
- **Text**: Background color (dark), SemiBold
- **Hover**: Glow intensifies, shadow expands slightly
- **Pressed**: Slight scale down (98%), glow reduces

### Secondary Button

Less prominent actions.

- **Fill**: Surface with vertical gradient
- **Border**: 1px, Lavender Deep at low opacity (~10%)
- **Corner radius**: 12px
- **Rim highlight**: Interior top edge
- **Padding**: 10px vertical, 18px horizontal
- **Text**: Cream Muted
- **Hover**: Fill shifts to Surface Elevated, border brightens, text to Cream

### Checkbox / Toggle Tiles

Selectable option tiles (like direction selection).

**Unselected state:**

- Fill: Well with gradient (recessed)
- Border: 1px, near-black at low opacity
- Inner shadow (recessed feeling)
- Indicator: Empty square, 2px border in Cream Dim, 6px radius

**Selected state:**

- Fill: Surface Elevated with gradient (raised)
- Border: 1px, Lavender Deep at ~30%
- Rim highlight on top edge
- Indicator: Lavender gradient fill, checkmark, small glow

### Slider

**Track:**

- Fill: Well with gradient (channel)
- Height: 6-8px
- Corner radius: Half of height
- Inner shadow (recessed)

**Filled portion:**

- Fill: Lavender gradient (Lavender Deep → Lavender)
- Subtle glow
- Rim highlight on top

**Thumb:**

- Size: 20-22px diameter
- Fill: Lavender with vertical gradient (ceramic knob)
- Border: 2px in Surface Elevated (creates inset ring)
- Outer glow: Lavender
- Outer shadow: Dark, below
- Rim highlight: Top interior, white at ~30%
- Bottom shadow: Interior bottom, black at ~20%

### Status Indicator (Dot)

Small dots indicating connection status, running state, etc.

- Size: 7-8px diameter
- Fill: Status color (Success, Warning, Error)
- Outer glow: Same color, creates halo
- Inner shadow: Very subtle bottom interior shadow (ceramic depth)
- Animation: Gentle pulse for active states (opacity 100% → 50% → 100% over 3s)

### Status Pill

Container showing status with dot and label.

- Fill: Surface with gradient
- Border: 1px, status color at ~20-30%
- Corner radius: 12px
- Rim highlight: Top interior edge
- Padding: 10px vertical, 16px horizontal
- Contents: Status dot + label text (Cream Muted), 10px gap

### Info Card (Detected Hardware)

Displays detected/computed information.

- Fill: Surface with gradient (one step below panel)
- Border: 1px, Lavender Deep at ~6%
- Corner radius: 14px
- Rim highlight: Top interior edge
- Padding: 14px
- Row layout: Label (Cream Dim, left) — Value (Cream, right), 8px vertical gap

### Divider

Separates sections within panels. Should feel like a ceramic ridge, not just a line.

**Option A (ridge):**

- Height: 2px
- Fill: Vertical gradient — dark at top, transparent middle, subtle light at bottom
- Creates sense of a physical ridge

**Option B (glow line):**

- Height: 1px
- Fill: Horizontal gradient — transparent → Lavender Deep at ~25% → transparent
- Fades at edges

### Preview Area (Large Well)

Large recessed area for previews, visualizations.

- Fill: Well with gradient
- Border: 1px, near-black at ~20%
- Corner radius: 18px
- Inner shadow: Soft, from top
- Very subtle outer glow (Cream at ~3%)
- Corner marks (optional): L-shaped brackets in corners, Lavender Deep at ~30%, 2px thick

### Phase Indicator

The horizontal phase progression (Setup → Focus → Confirm → Run → Done).

**Container:**

- Fill: Surface with gradient
- Border: 1px, Lavender Deep at ~5%
- Corner radius: Pill (fully rounded)
- Rim highlight: Top interior
- Padding: 6px

**Phase pills:**

- Corner radius: 12px (slightly less than container)
- Padding: 8px vertical, 16px horizontal

**Inactive pill:**

- Fill: Transparent
- Text: Cream Dim

**Active pill:**

- Fill: Surface Elevated with gradient
- Shadow: Small outer shadow + rim highlight
- Text: Cream

---

## Animation & Motion

### Principles

**Purposeful**: Animation communicates state change, not decoration.

**Gentle**: Everything eases softly. No snappy or bouncy animations. The ceramic "thock" is satisfying but not jarring.

**Brief**: Interactions should feel responsive, not sluggish.

### Timing

| Type | Duration |
|------|----------|
| Micro interactions (hover, focus) | 150-200ms |
| State changes | 250ms |
| Panel transitions | 300-400ms |
| Fade in/out | 200-300ms |

### Easing

Use ease-out for most interactions (fast start, gentle end). Use ease-in-out for larger transitions.

Avoid linear timing and overly bouncy easing.

### What Animates

**Do animate:**

- Hover state transitions (background, border, glow)
- Focus states (glow intensity)
- Button press (subtle scale reduction to ~98%)
- Slider thumb following input
- Status indicator pulse (gentle opacity)
- Phase transitions (crossfade)
- Panel appearance (fade + slight vertical movement)
- Glow intensity changes

**Don't animate:**

- Text content changes (instant)
- Critical status changes (immediate)
- Data value updates (instant or very fast)

### Press Feedback

Buttons should have subtle "give" when pressed — scale down to ~98% quickly, then return to 100% with a gentle ease-out. This creates the tactile "thock" feeling.

### Pulse Animation

For active status indicators (camera connected, acquisition running):

- Opacity cycles from 100% → 50% → 100%
- Duration: ~3 seconds per cycle
- Easing: ease-in-out
- Continuous loop

---

## Dark Room Considerations

### Brightness Limits

- Maximum text brightness: Cream (`#f0e6dc`)
- Never use pure white (`#ffffff`) anywhere
- Glows should be noticeable but not harsh
- All colors should be muted, not saturated

### Touch Targets

- Minimum interactive element size: 44px
- Generous padding on all clickable elements
- Clear visual distinction for interactive vs. static elements

### Critical Element Visibility

- Primary actions (Start, Stop): Amber with strong glow
- Status indicators: Visible glow halos
- Error states: Noticeable but not alarming (dusty rose, not red)
- Current phase: Clearly distinguished

### Optional Dim Mode

Consider offering a toggle that reduces all color brightness by ~20% for especially sensitive situations. This would darken creams, reduce glow intensity, and make the interface even more subtle.

---

## Summary: The Three-Layer Model

Think of the UI as three conceptual layers:

**1. The Darkness (Background)**
Warm, plum-shifted black. The blanket you're wrapped in. Vast and comfortable.

**2. Ceramic Surfaces (Panels, Inputs, Buttons)**
Floating in the darkness with soft glows. They have mass and curvature. Light catches their glazed rims. They yield slightly when pressed.

**3. Light Sources (Text, Accents, Glows)**
Cream text glows like candlelight. Lavender provides ambient mood. Amber is the one warm beacon — the nightlight — used sparingly for primary actions.

---

## Quick Reference: Key Values

### Colors (Hex)

```
Background:       #13111a
Surface:          #1c1824
Surface Elevated: #241f2e
Well:             #0f0d14

Cream:            #f0e6dc
Cream Muted:      #c4b8ab
Cream Dim:        #7a7067

Lavender:         #b8a5c9
Lavender Dim:     #8677a3
Lavender Deep:    #6b5a8a

Amber:            #e8c49a
Amber Glow:       #d4a574

Success:          #a8c4b0
Warning:          #e8c49a
Error:            #c9a9a9
Info:             #b8a5c9
```

### Spacing (px)

```
XS:  4    SM:  8    MD: 12
LG: 16    XL: 20   2XL: 24   3XL: 32
```

### Radii (px)

```
SM:  6    MD: 12    LG: 14
XL: 16   2XL: 20   Pill: 9999
```

### Type Sizes (px)

```
Display: 24    Title: 20    Heading: 15
Body: 14       Caption: 13  Small: 12
Tiny: 11       Mono: 13
```
