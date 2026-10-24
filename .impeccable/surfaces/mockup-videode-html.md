---
version: 1
slug: "mockup-videode-html"
primary_target: "mockup/videode.html"
related_targets: []
---

# Surface: videode editor mockup

Scope: the main editor window (single self-contained HTML mockup of the native GPUI app). Mode: Operate.
Audience: newcomers, pro editors and node/motion artists, weighted equally. Job: cut, arrange, preview, export; peek into the node graph behind anything.
Constraints: keyboard-first (Ctrl/Cmd+K palette, rebindable shortcuts), nodes hidden until peeked, no colour-only state.
Open decisions: icon library for the native app; final face licensing for bundling.

## Direction contract

THESIS: Everything is a node, and the wrapper shows it without making you learn it. Refuses the category default of a grey NLE with one blue accent and a separate "Fusion page".
OWN-WORLD: SMPTE bars/PLUGE. Surfaces are three near-black PLUGE steps (#09090a / #111113 / #19191c); seven 75%-bar colours belong only to node categories (Source white, Transform yellow, Grade cyan, Composite green, Audio magenta, Text red, Output blue). Geist + Geist Mono for measurement only. Two type sizes. State = line form: solid live, dashed offline, struck muted.
STORY: A newcomer sees a familiar editor and cuts. A curious user sees coloured chain strips under clips, hits Tab, and the strip unfolds into the clip's graph in place. A power user drives everything from the palette and can expand the peek to the full sequence graph.
FIRST VIEWPORT: Top bar (bars logo, breadcrumb, palette field) 36px; media bin 280 left, viewer centre, inspector 300 right with sections coloured by node category; timeline ~40% bottom with guide-word ruler, 4 tracks, chain strips, playhead; status bar with snap/graph counts.
FORM: Test Card, my list #5 (assigned). seed: none (unavailable). Signature interaction: chain strip unfolds into the peek graph (Tab), and peek expands to full sequence graph (Shift+Tab).
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
