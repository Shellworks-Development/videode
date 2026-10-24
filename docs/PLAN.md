# videode plan

Approved 2026-10-25, together with `mockup/videode.html` and `DESIGN.md`.

## Stack

| Concern | Choice |
|---|---|
| UI | GPUI via `gpui-kit` 0.8 (pins a matching GPUI snapshot; adds components, dock, menus) |
| Decode / encode | FFmpeg via `ffmpeg-next`, dynamically linked (LGPL) |
| Audio out | `cpal` |
| Compositing | CPU RGBA first, behind a `Compositor` trait; `wgpu` later for effects |
| Time | Rational: integer frames/ticks over a timebase |
| Project files | `.vde` in **KDL** (`kdl` crate), versioned schema. No JSON. |
| Concurrency | Decode, audio and export on worker threads; channels back to the UI |

## Architecture: everything is a node

`videode-core` is a node graph engine: typed nodes with typed ports, lazy pull-based
evaluation in topological order, and per-node caching. Source, Trim, Transform, Grade,
Blur, Opacity, Fade, Text, Gain, Duck, Track, Composite, Mix, Sequence and Output are all
node types. A clip is a chain of nodes feeding a track; a sequence is a node and can nest.

The timeline, inspector, peek panel and graph view are different views of the same graph.
Every UI edit is a graph edit through one command stack (undo/redo).

## Crates

```
videode-core     graph engine, project model, commands/undo, KDL load/save
videode-media    probe, decoder pool, seeking, frame cache, thumbnails, waveforms
videode-render   evaluates the graph at time t into a frame (Compositor trait)
videode-audio    mixer, cpal output, master clock
videode-export   render to file through FFmpeg, progress, cancel
videode-app      GPUI app: shell, timeline, inspector, peek, graph, palette
```

Crates are added in the milestone that needs them.

## Milestones

- **M0 Skeleton.** Workspace, GPUI window with the full shell layout, Test Card theme,
  bundled fonts, headless snapshot tool, CI on Linux/macOS/Windows.
- **M1 Core + timeline.** Graph engine, project model, undo stack, KDL save/load. Timeline
  rendering and editing: move, trim, split, delete, ripple delete, snapping, zoom.
- **M2 Node views.** Chain strips on clips, Tab peek panel (reorder, add, remove, bypass),
  Shift+Tab sequence graph, inspector driven by node params, command palette.
- **M3 Media + preview.** Import, probe, bin with thumbnails, decoder pool, frame-accurate
  scrubbing in the viewer.
- **M4 Playback + audio.** cpal output, audio-master clock, J/K/L, meters, waveforms.
- **M5 Export.** Output node renders H.264/AAC MP4 (plus H.265/ProRes when available).
- **M6 Polish + packaging.** Rebindable shortcuts, autosave and recovery, recent projects,
  installers (MSI, DMG, AppImage/Flatpak) with FFmpeg bundled.

Later: wgpu effects, transitions and keyframes, proxies, hardware decode, multiple sequences.
