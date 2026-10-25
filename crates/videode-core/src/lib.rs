//! videode core: the project model, node chains, editing operations with
//! undo, the sequence graph, and `.vde` (KDL) project files. No UI.

pub mod demo;
pub mod document;
pub mod edit;
pub mod graph;
pub mod kdl_io;
pub mod node;
pub mod project;
pub mod settings;
pub mod time;

pub use document::Document;
pub use edit::{EditError, EditResult, snap};
pub use node::{
    Category, ClipKind, Node, NodeId, NodeKind, ParamKind, ParamSpec, StreamKind, Value,
};
pub use project::{Clip, ClipId, Media, MediaId, MediaKind, Project, Track, TrackId};
pub use settings::{Animations, Settings};
pub use time::{Fps, Frame};
