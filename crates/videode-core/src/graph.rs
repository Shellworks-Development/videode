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
}
