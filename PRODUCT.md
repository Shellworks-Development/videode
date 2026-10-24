# Product

<!-- impeccable:product-schema 1 -->

## Platform

desktop (native, Windows / macOS / Linux). Built with Rust + GPUI; HTML is used only for mockups.

## Stack

Rust workspace, GPUI for UI, FFmpeg (dynamically linked) for decode/encode, cpal for audio. Project files are KDL (`.vde`). No JSON for project data.

## Users

Equal weight across:
- **Newcomers / creators** cutting vlogs, tutorials, shorts. Must be able to import, cut, and export without ever seeing a node.
- **Pro editors / post** coming from Resolve, Premiere, Nuke. Expect dense info, full keyboard control, command palette, precise numeric entry.
- **Motion / generative artists** who want to live in the graph.

## Purpose & mechanism

videode is a video editor where **everything is a node**. Media, clips, effects, tracks, transitions, the sequence itself and the export are all nodes in one graph. The familiar NLE interface (media bin, viewer, timeline, inspector) is a wrapper that edits that graph.

- **Peek on demand.** Nodes stay hidden by default. Any clip, track or sequence can be "peeked" with Tab or a peek affordance, which reveals the subgraph behind it. A docked split view is available for those who want it permanently.
- Every edit in the wrapper UI is a graph edit. Every graph edit is reflected in the wrapper.

## Terminology

- **Node**: the unit of computation (Source, Trim, Transform, Grade, Blur, Mix, Text, Output, ...).
- **Clip**: the wrapper's view of a Source → ... → track-input chain.
- **Peek**: opening the graph behind a wrapper element in place.
- **Sequence**: a graph whose output is a timeline composite. Sequences can nest as nodes.

## Constraints

- Keyboard-first: every action reachable via command palette (Ctrl/Cmd+K) and rebindable shortcuts.
- Must stay approachable: no node jargon is required for basic editing.
- Accessibility: visible focus, sufficient contrast on dark UI, no color-only signalling for track/clip state.
