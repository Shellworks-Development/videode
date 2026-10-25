//! The whole sequence as one node graph: every clip's chain feeds its track,
//! video tracks feed Composite, audio tracks feed Mix, both feed Sequence,
//! and Sequence feeds Output. This is the graph the "Shift+Tab" view shows
//! and the order the renderer evaluates.

use crate::node::{Category, NodeId, StreamKind};
use crate::project::{ClipId, Project, TrackId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GraphRef {
    ClipNode { clip: ClipId, node: NodeId },
    Track(TrackId),
    Composite,
    Mix,
    Sequence,
    Output,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphNode {
    pub id: GraphRef,
    pub label: String,
    pub summary: String,
    pub category: Category,
    pub bypass: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SequenceGraph {
    pub nodes: Vec<GraphNode>,
    /// `(from, to)` indices into `nodes`.
    pub edges: Vec<(usize, usize)>,
}

impl SequenceGraph {
    pub fn index_of(&self, id: GraphRef) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    /// Kahn's algorithm. Returns `None` if the graph has a cycle.
    pub fn topo_order(&self) -> Option<Vec<usize>> {
        let mut indegree = vec![0usize; self.nodes.len()];
        for &(_, to) in &self.edges {
            indegree[to] += 1;
        }
        let mut ready: Vec<usize> = (0..self.nodes.len())
            .filter(|&i| indegree[i] == 0)
            .collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(i) = ready.pop() {
            order.push(i);
            for &(from, to) in &self.edges {
                if from == i {
                    indegree[to] -= 1;
                    if indegree[to] == 0 {
                        ready.push(to);
                    }
                }
            }
        }
        (order.len() == self.nodes.len()).then_some(order)
    }
}

pub fn sequence_graph(p: &Project) -> SequenceGraph {
    let mut g = SequenceGraph::default();
    let add = |g: &mut SequenceGraph, id, label: String, summary: String, category, bypass| {
        g.nodes.push(GraphNode {
            id,
            label,
            summary,
            category,
            bypass,
        });
        g.nodes.len() - 1
    };

    let composite = add(
        &mut g,
        GraphRef::Composite,
        "Composite".into(),
        "stack video tracks".into(),
        Category::Composite,
        false,
    );
    let mix = add(
        &mut g,
        GraphRef::Mix,
        "Mix".into(),
        "sum audio tracks".into(),
        Category::Audio,
        false,
    );
    let seq = add(
        &mut g,
        GraphRef::Sequence,
        "Sequence".into(),
        p.sequence.name.clone(),
        Category::Composite,
        false,
    );
    let out = add(
        &mut g,
        GraphRef::Output,
        "Output".into(),
        format!("{} · {}", p.output.container, p.output.codec),
        Category::Output,
        false,
    );
    g.edges.push((composite, seq));
    g.edges.push((mix, seq));
    g.edges.push((seq, out));

    for t in &p.sequence.tracks {
        let (cat, bus) = match t.kind {
            StreamKind::Video => (Category::Composite, composite),
            StreamKind::Audio => (Category::Audio, mix),
        };
        let ti = add(
            &mut g,
            GraphRef::Track(t.id),
            format!("Track {}", t.label),
            t.name.clone(),
            cat,
            !t.enabled,
        );
        g.edges.push((ti, bus));
        for c in p.clips_on(t.id) {
            let mut prev = None;
            for n in &c.nodes {
                let ni = add(
                    &mut g,
                    GraphRef::ClipNode {
                        clip: c.id,
                        node: n.id,
                    },
                    n.kind.name().into(),
                    if n.kind == crate::node::NodeKind::Source {
                        p.clip_name(c)
                    } else {
                        n.summary()
                    },
                    n.kind.category(),
                    n.bypass,
                );
                if let Some(pi) = prev {
                    g.edges.push((pi, ni));
                }
                prev = Some(ni);
            }
            if let Some(last) = prev {
                g.edges.push((last, ti));
            }
        }
    }
    g
}

/// Node card size and spacing on the graph canvas, in logical pixels at 1×.
pub const NODE_W: f32 = 136.;
pub const NODE_H: f32 = 58.;
const STEP_X: f32 = 176.;
const GROUP_PAD: f32 = 12.;
const GROUP_LABEL: f32 = 18.;
const GROUP_GAP: f32 = 12.;
const TRACK_GAP: f32 = 18.;
const COLUMN_GAP: f32 = 40.;

/// A clip's chain, drawn as a labelled box around its nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipGroup {
    pub clip: ClipId,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where everything in the [`SequenceGraph`] sits: clips' chains in rows
/// grouped by track on the left, then tracks, buses, Sequence and Output.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphLayout {
    pub graph: SequenceGraph,
    /// Top-left of each node, indexed like `graph.nodes`.
    pub pos: Vec<(f32, f32)>,
    pub groups: Vec<ClipGroup>,
    pub width: f32,
    pub height: f32,
}

impl GraphLayout {
    /// Wire endpoints: the source's right edge to the target's left edge,
    /// both at mid-height, plus whether the target is bypassed.
    pub fn wires(&self) -> impl Iterator<Item = ((f32, f32), (f32, f32), bool)> + '_ {
        self.graph.edges.iter().map(|&(a, b)| {
            let (ax, ay) = self.pos[a];
            let (bx, by) = self.pos[b];
            (
                (ax + NODE_W, ay + NODE_H / 2.),
                (bx, by + NODE_H / 2.),
                self.graph.nodes[b].bypass,
            )
        })
    }

    /// The topmost node under a point, in layout coordinates.
    pub fn node_at(&self, x: f32, y: f32) -> Option<usize> {
        self.pos
            .iter()
            .rposition(|&(nx, ny)| x >= nx && x <= nx + NODE_W && y >= ny && y <= ny + NODE_H)
    }

    pub fn group_at(&self, x: f32, y: f32) -> Option<&ClipGroup> {
        self.groups
            .iter()
            .find(|g| x >= g.x && x <= g.x + g.w && y >= g.y && y <= g.y + g.h)
    }
}

pub fn layout(p: &Project) -> GraphLayout {
    let graph = sequence_graph(p);
    let mut pos = vec![(0., 0.); graph.nodes.len()];
    let mut groups = Vec::new();
    let idx = |r: GraphRef| graph.index_of(r).expect("every ref is in the graph");

    let longest = p
        .sequence
        .clips
        .iter()
        .map(|c| c.nodes.len())
        .max()
        .unwrap_or(1)
        .max(1);
    let track_x = longest as f32 * STEP_X + COLUMN_GAP + GROUP_PAD;
    let group_h = NODE_H + GROUP_PAD * 2. + GROUP_LABEL;

    let mut y = 0.;
    let mut track_mid = Vec::new();
    for t in &p.sequence.tracks {
        let top = y;
        let clips = p.clips_on(t.id);
        for c in &clips {
            let w = c.nodes.len() as f32 * STEP_X - (STEP_X - NODE_W) + GROUP_PAD * 2.;
            groups.push(ClipGroup {
                clip: c.id,
                x: 0.,
                y,
                w,
                h: group_h,
            });
            for (i, n) in c.nodes.iter().enumerate() {
                let at = idx(GraphRef::ClipNode {
                    clip: c.id,
                    node: n.id,
                });
                pos[at] = (GROUP_PAD + i as f32 * STEP_X, y + GROUP_PAD + GROUP_LABEL);
            }
            y += group_h + GROUP_GAP;
        }
        if clips.is_empty() {
            y += NODE_H + GROUP_GAP;
        }
        let mid = (top + y - GROUP_GAP) / 2.;
        pos[idx(GraphRef::Track(t.id))] = (track_x, mid - NODE_H / 2.);
        track_mid.push((t.kind, mid));
        y += TRACK_GAP;
    }

    let average = |kind: StreamKind| {
        let mids: Vec<f32> = track_mid
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, m)| *m)
            .collect();
        if mids.is_empty() {
            y / 2.
        } else {
            mids.iter().sum::<f32>() / mids.len() as f32
        }
    };
    let (video, audio) = (average(StreamKind::Video), average(StreamKind::Audio));
    let bus_x = track_x + STEP_X + COLUMN_GAP;
    let seq_x = bus_x + STEP_X + COLUMN_GAP;
    let out_x = seq_x + STEP_X + COLUMN_GAP;
    let seq_mid = (video + audio) / 2.;
    pos[idx(GraphRef::Composite)] = (bus_x, video - NODE_H / 2.);
    pos[idx(GraphRef::Mix)] = (bus_x, audio - NODE_H / 2.);
    pos[idx(GraphRef::Sequence)] = (seq_x, seq_mid - NODE_H / 2.);
    pos[idx(GraphRef::Output)] = (out_x, seq_mid - NODE_H / 2.);

    GraphLayout {
        graph,
        pos,
        groups,
        width: out_x + NODE_W,
        height: (y - TRACK_GAP).max(NODE_H),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_graph_shape() {
        let p = crate::demo::demo_project();
        let g = sequence_graph(&p);
        assert_eq!(g.nodes.len(), p.node_count());
        let order = g.topo_order().expect("acyclic");
        let pos = |r| {
            order
                .iter()
                .position(|&i| i == g.index_of(r).unwrap())
                .unwrap()
        };
        assert!(pos(GraphRef::Sequence) < pos(GraphRef::Output));
        assert!(pos(GraphRef::Composite) < pos(GraphRef::Sequence));
        // Every clip's generator comes before its track.
        for c in &p.sequence.clips {
            let generator = GraphRef::ClipNode {
                clip: c.id,
                node: c.nodes[0].id,
            };
            assert!(pos(generator) < pos(GraphRef::Track(c.track)));
        }
    }

    #[test]
    fn detects_cycles() {
        let mut g = sequence_graph(&Project::new("t"));
        g.edges.push((
            g.index_of(GraphRef::Output).unwrap(),
            g.index_of(GraphRef::Composite).unwrap(),
        ));
        assert!(g.topo_order().is_none());
    }

    #[test]
    fn layout_flows_left_to_right_without_overlap() {
        let p = crate::demo::demo_project();
        let l = layout(&p);
        assert_eq!(l.pos.len(), l.graph.nodes.len());
        for &(a, b) in &l.graph.edges {
            assert!(l.pos[a].0 < l.pos[b].0, "edge {a}->{b} runs backwards");
        }
        for (i, &(ax, ay)) in l.pos.iter().enumerate() {
            for &(bx, by) in &l.pos[i + 1..] {
                let apart = (ax - bx).abs() >= NODE_W || (ay - by).abs() >= NODE_H;
                assert!(apart, "nodes overlap at ({ax},{ay})");
            }
        }
        assert_eq!(l.groups.len(), p.sequence.clips.len());
        let out = l.graph.index_of(GraphRef::Output).unwrap();
        let (ox, oy) = l.pos[out];
        assert_eq!(l.node_at(ox + 1., oy + 1.), Some(out));
        assert!(l.width >= ox + NODE_W);
    }
}
