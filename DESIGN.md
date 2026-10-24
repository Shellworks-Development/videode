---
name: videode
description: Native node-based video editor; a familiar NLE wrapped around a node graph.
colors:
  pluge-sub: "#09090a"
  pluge-black: "#111113"
  pluge-raised: "#19191c"
  pluge-control: "#222226"
  pluge-hover: "#2b2b30"
  line: "#2a2a2f"
  line-strong: "#3a3a41"
  ink: "#e8e7e2"
  ink-dim: "#a3a29d"
  ink-faint: "#8a8984"
  bone: "#f1ede0"
  bar-source: "#bdbdb8"
  bar-transform: "#c9c43c"
  bar-look: "#3cc4c4"
  bar-composite: "#45c45a"
  bar-audio: "#c43cc4"
  bar-text: "#d0463f"
  bar-output: "#4a62d0"
  state-warn: "#e0a03a"
  state-error: "#e05a4f"
typography:
  body:
    fontFamily: "Geist, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.45
  title:
    fontFamily: "Geist, ui-sans-serif, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 600
    letterSpacing: "-0.01em"
  measure:
    fontFamily: "Geist Mono, ui-monospace, SF Mono, Cascadia Mono, monospace"
    fontSize: "12px"
    fontWeight: 400
    fontFeature: "tnum"
  keycap:
    fontFamily: "Geist Mono, ui-monospace, monospace"
    fontSize: "11px"
    fontWeight: 500
rounded:
  swatch: "2px"
  clip: "4px"
  control: "5px"
  field: "6px"
  menu: "8px"
  panel: "10px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
components:
  button-primary:
    backgroundColor: "{colors.bone}"
    textColor: "{colors.pluge-sub}"
    rounded: "{rounded.control}"
    height: "26px"
    padding: "0 10px"
  button-default:
    backgroundColor: "{colors.pluge-raised}"
    textColor: "{colors.ink}"
    rounded: "{rounded.control}"
    height: "26px"
    padding: "0 10px"
  button-icon:
    textColor: "{colors.ink-dim}"
    rounded: "{rounded.control}"
    size: "28px"
  field:
    backgroundColor: "{colors.pluge-sub}"
    textColor: "{colors.ink}"
    typography: "{typography.measure}"
    rounded: "{rounded.control}"
    height: "24px"
    padding: "0 7px"
  clip:
    backgroundColor: "{colors.pluge-control}"
    textColor: "{colors.ink}"
    rounded: "{rounded.clip}"
  node:
    backgroundColor: "{colors.pluge-black}"
    textColor: "{colors.ink}"
    rounded: "{rounded.field}"
    width: "136px"
  peek-panel:
    backgroundColor: "{colors.pluge-raised}"
    rounded: "{rounded.panel}"
---

# Design System: videode

## Overview

**Creative North Star: "The Test Card"** *(named from the chosen direction; not separately confirmed)*

videode's world comes from SMPTE colour bars and the PLUGE strip. The interface is a set of near-black steps, the same blacks a colourist uses to set a monitor's black level. Colour belongs only to what the picture is made of. Each of the seven node categories owns one 75%-bar colour, and that colour appears wherever the node appears: as the cap on a node, a segment in a clip's chain strip, the swatch in an inspector section, the swatch in the palette. The chrome itself never carries a category colour.

The layout is the standard NLE: media bin on the left, viewer in the centre, inspector on the right, timeline below. The node graph sits underneath that layout, and the chain strip under each clip is what connects the two. Tab unfolds the strip into the clip's graph in place. Shift+Tab expands it to the whole sequence graph. Density is pro-tool tight. Everything is reachable from the keyboard and from the Ctrl/Cmd+K palette.

**Key Characteristics:**
- Three PLUGE surface steps; no tinted chrome.
- Seven bar colours, used only for node-category identity.
- Line form carries state: solid means live, dashed means offline or bypassed, struck means muted, double means locked.
- Two type sizes; rank comes from weight, tone and case.
- Monospace is used only for measurement: timecode, values, keycaps.

## Colors

The neutrals are true near-blacks. The seven bar colours are reserved for identifying nodes.

- **PLUGE Sub-black** (`pluge-sub`): the viewer and timeline ground, and the inside of input fields.
- **PLUGE Black** (`pluge-black`): panes, top bar, status bar.
- **PLUGE Raised** (`pluge-raised`): peek panel, palette, menus, default buttons.
- **Bone** (`bone`): selection, focus, the playhead, the primary button. It is the only "accent", and it means "this one".
- **Bars** (`bar-*`): Source, Transform, Color & filter, Composite, Audio, Text, Output.

**The Bars-Are-Nodes Rule.** A bar colour may appear only where it identifies a node category: a node cap, a chain-strip segment, a category swatch, or an audio waveform (the audio node's data). It never appears on chrome, buttons or focus.

**The Warn-Is-State Rule.** `state-warn` marks offline media, always together with a dashed line. `state-error` appears only in the meter's clip zone.

## Typography

**Body:** Geist 12px/1.45. **Title:** Geist 13px/600 (pane titles, inspector heading, palette input). **Measure:** Geist Mono 12px with tabular numerals. **Keycap:** Geist Mono 11px/500.

**The Two-Size Rule.** The shell uses exactly 12px and 13px; keycaps at 11px are the only exception. Rank comes from weight (400, 500, 600) and tone (ink, dim, faint), never from size.

**The Measurement-Only Mono Rule.** Mono is used for timecode, numeric values, file and codec facts, and keycaps. It is never decoration.

## Layout

The app shell is a grid: top bar 38px, workspace, splitter 5px, timeline (default 340px, resizable from 180px), status bar 26px. Workspace columns are bin 264px, viewer `1fr`, inspector 304px. Below 1280px wide these become 224px and 272px. The minimum window width is 1024px.

The spacing rhythm is 4/8/12/16. Pane header bars are 32px, field rows 28px, clip rows 48–64px.

The full sequence graph takes over the workspace and timeline. The inspector stays docked.

## Elevation & Depth

Depth comes from tonal steps. Shadows are reserved for floating layers: peek, palette, menus, tooltips and drag ghosts. They use an offset and a soft blur, for example `0 18px 48px rgba(0,0,0,.6)`. A selected clip adds a 1px bone ring plus a small drop shadow.

## Shapes

Radii are small and functional:
- 2px on swatches
- 4px on clips
- 5px on controls
- 6px on fields and nodes
- 8px on menus
- 10px on the peek panel and palette

Pills are used only for the track pin in peek.

## Components

- **Clip:** has a name row (17px), a filmstrip or waveform, and a 5px **chain strip** along the bottom with one segment per node in category colour. Bypassed segments drop to 25% opacity. Clips have 7px trim handles on each edge. Fades are shown as darkened corner triangles.
- **Node:** 136px wide, with a 4px category cap, the title at 600 weight, and a one-line mono summary. In/out ports are 9px rings. A bypassed node gets a dashed border and a struck title.
- **Peek panel:** opens directly below the clip when there is room, otherwise above it, and never covers the transport. It unfolds from the chain strip's rectangle with a 300ms `cubic-bezier(.16,1,.3,1)` motion. The nodes inside stagger in at 35ms intervals.
- **Inspector section:** an 8px swatch, the node name, and bypass and remove buttons. Params use a 96px label column. Dragging the label scrubs the value; Shift multiplies the step by 10 and Alt divides it by 10. Double-clicking the label resets it.
- **Command palette:** results grouped as Recent, Actions, Add node, Clips and Media. Fuzzy matches are highlighted in bone. Disabled items say why they're unavailable.
- **Buttons:** there is one primary button (bone, for Export/Render). Default buttons are raised. Ghost buttons sit in toolbars. Every tooltip shows its shortcut as a keycap.

## Do's and Don'ts

- Do show every action's shortcut where the action lives (tooltip, menu, palette).
- Do pair every state colour with a line form (dashed offline, struck muted).
- Don't put bar colours on chrome, buttons or selection.
- Don't add a third type size to the shell.
- Don't use side-stripe accents on list rows; use the 8px category swatch.
- Don't hide the transport behind floating panels.
