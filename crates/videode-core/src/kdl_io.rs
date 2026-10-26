//! `.vde` project files, written as KDL v2.
//!
//! ```kdl
//! videode version=1
//! project "Signal Study"
//! media {
//!     file id=1 path="media/dusk.mov" kind=video duration=384 codec="ProRes 422 HQ" format="3840×2160"
//! }
//! sequence "Main" fps=24 fps-den=1 width=1920 height=1080 {
//!     track V1 kind=video name=Picture {
//!         clip start=0 duration=168 media=1 in=0 {
//!             source
//!             grade exposure=-0.2 temperature=5200
//!         }
//!     }
//! }
//! output container=MP4 codec=H.264 resolution=Sequence quality=High hardware=Auto path="export.mp4"
//! ```
//!
//! Ids inside the program are regenerated on load; files refer to media by
//! the `id` written in the `media` block.

use std::collections::HashMap;

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};

use crate::node::{ClipKind, NodeKind, ParamKind, StreamKind, Value};
use crate::project::{Clip, ClipId, MediaId, MediaKind, OutputSettings, Project};
use crate::time::Fps;

pub const FORMAT_VERSION: i64 = 1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LoadError {
    #[error("not a valid KDL document: {0}")]
    Syntax(String),
    #[error("not a videode project (missing `videode` header)")]
    NotAProject,
    #[error("project format version {0} is newer than this videode supports")]
    TooNew(i64),
    #[error("{0}")]
    Invalid(String),
}

fn invalid<T>(msg: impl Into<String>) -> Result<T, LoadError> {
    Err(LoadError::Invalid(msg.into()))
}

/* ---------------- writing ---------------- */

fn number(n: f64) -> KdlValue {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        KdlValue::Integer(n as i128)
    } else {
        KdlValue::Float(n)
    }
}

fn prop(key: &str, value: impl Into<KdlValue>) -> KdlEntry {
    KdlEntry::new_prop(key, value.into())
}

pub fn to_kdl(p: &Project) -> String {
    let mut doc = KdlDocument::new();

    let mut header = KdlNode::new("videode");
    header.push(prop("version", FORMAT_VERSION as i128));
    doc.nodes_mut().push(header);

    let mut project = KdlNode::new("project");
    project.push(KdlEntry::new(p.name.as_str()));
    doc.nodes_mut().push(project);

    let mut media = KdlNode::new("media");
    let media_children = media.ensure_children();
    for m in &p.media {
        let mut f = KdlNode::new("file");
        f.push(prop("id", m.id.0 as i128));
        f.push(prop("path", m.path.to_string_lossy().as_ref()));
        f.push(prop("kind", m.kind.ident()));
        if let Some(d) = m.duration {
            f.push(prop("duration", d as i128));
        }
        if !m.codec.is_empty() {
            f.push(prop("codec", m.codec.as_str()));
        }
        if !m.format.is_empty() {
            f.push(prop("format", m.format.as_str()));
        }
        media_children.nodes_mut().push(f);
    }
    doc.nodes_mut().push(media);

    let s = &p.sequence;
    let mut seq = KdlNode::new("sequence");
    seq.push(KdlEntry::new(s.name.as_str()));
    seq.push(prop("fps", s.fps.num as i128));
    seq.push(prop("fps-den", s.fps.den as i128));
    seq.push(prop("width", s.width as i128));
    seq.push(prop("height", s.height as i128));
    let tracks = seq.ensure_children();
    for t in &s.tracks {
        let mut tn = KdlNode::new("track");
        tn.push(KdlEntry::new(t.label.as_str()));
        tn.push(prop(
            "kind",
            match t.kind {
                StreamKind::Video => "video",
                StreamKind::Audio => "audio",
            },
        ));
        tn.push(prop("name", t.name.as_str()));
        if !t.enabled {
            tn.push(prop("enabled", false));
        }
        if t.locked {
            tn.push(prop("locked", true));
        }
        for c in p.clips_on(t.id) {
            let mut cn = KdlNode::new("clip");
            cn.push(prop("start", c.start as i128));
            cn.push(prop("duration", c.duration as i128));
            if let Some(m) = c.media {
                cn.push(prop("media", m.0 as i128));
                cn.push(prop("in", c.source_in as i128));
            }
            let nodes = cn.ensure_children();
            for n in &c.nodes {
                let mut nn = KdlNode::new(n.kind.ident());
                for spec in n.kind.params() {
                    match n.get(spec.key) {
                        Some(Value::Number(v)) => nn.push(prop(spec.key, number(*v))),
                        Some(Value::Text(v)) => nn.push(prop(spec.key, v.as_str())),
                        None => {}
                    }
                }
                if n.bypass {
                    nn.push(prop("bypass", true));
                }
                nodes.nodes_mut().push(nn);
            }
            tn.ensure_children().nodes_mut().push(cn);
        }
        tracks.nodes_mut().push(tn);
    }
    doc.nodes_mut().push(seq);

    let o = &p.output;
    let mut out = KdlNode::new("output");
    out.push(prop("container", o.container.as_str()));
    out.push(prop("codec", o.codec.as_str()));
    out.push(prop("resolution", o.resolution.as_str()));
    out.push(prop("quality", o.quality.as_str()));
    out.push(prop("hardware", o.hardware.as_str()));
    out.push(prop("path", o.path.as_str()));
    doc.nodes_mut().push(out);

    doc.autoformat();
    doc.to_string()
}

/* ---------------- reading ---------------- */

fn get_int(node: &KdlNode, key: &str) -> Result<Option<i64>, LoadError> {
    match node.get(key) {
        None => Ok(None),
        Some(KdlValue::Integer(i)) => i64::try_from(*i)
            .map(Some)
            .or_else(|_| invalid(format!("`{key}` is out of range"))),
        Some(_) => invalid(format!(
            "`{}`: `{key}` must be a whole number",
            node.name().value()
        )),
    }
}

fn req_int(node: &KdlNode, key: &str) -> Result<i64, LoadError> {
    get_int(node, key)?
        .ok_or_else(|| LoadError::Invalid(format!("`{}` is missing `{key}`", node.name().value())))
}

fn get_str<'a>(node: &'a KdlNode, key: &str) -> Result<Option<&'a str>, LoadError> {
    match node.get(key) {
        None => Ok(None),
        Some(KdlValue::String(s)) => Ok(Some(s)),
        Some(_) => invalid(format!(
            "`{}`: `{key}` must be a string",
            node.name().value()
        )),
    }
}

fn get_bool(node: &KdlNode, key: &str, default: bool) -> Result<bool, LoadError> {
    match node.get(key) {
        None => Ok(default),
        Some(KdlValue::Bool(b)) => Ok(*b),
        Some(_) => invalid(format!(
            "`{}`: `{key}` must be #true or #false",
            node.name().value()
        )),
    }
}

fn arg_str(node: &KdlNode) -> Option<&str> {
    node.entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
}

fn children(node: &KdlNode) -> &[KdlNode] {
    node.children().map(|d| d.nodes()).unwrap_or(&[])
}

pub fn from_kdl(text: &str) -> Result<Project, LoadError> {
    let doc: KdlDocument = text
        .parse()
        .map_err(|e: kdl::KdlError| LoadError::Syntax(e.to_string()))?;
    let header = doc.get("videode").ok_or(LoadError::NotAProject)?;
    let version = get_int(header, "version")?.unwrap_or(0);
    if version > FORMAT_VERSION {
        return Err(LoadError::TooNew(version));
    }

    let name = doc
        .get("project")
        .and_then(arg_str)
        .unwrap_or("Untitled project");
    let mut p = Project::new(name);
    p.sequence.tracks.clear();

    let mut media_ids: HashMap<i64, MediaId> = HashMap::new();
    if let Some(media) = doc.get("media") {
        for f in children(media)
            .iter()
            .filter(|n| n.name().value() == "file")
        {
            let file_id = req_int(f, "id")?;
            let path = get_str(f, "path")?
                .ok_or_else(|| LoadError::Invalid("media file without `path`".into()))?;
            let kind_s = get_str(f, "kind")?.unwrap_or("video");
            let kind = MediaKind::from_ident(kind_s)
                .ok_or_else(|| LoadError::Invalid(format!("unknown media kind `{kind_s}`")))?;
            let duration = get_int(f, "duration")?;
            let id = p.add_media(
                path,
                kind,
                duration,
                get_str(f, "codec")?.unwrap_or(""),
                get_str(f, "format")?.unwrap_or(""),
            );
            if get_str(f, "status")? == Some("offline") {
                p.media.last_mut().expect("just added").offline = true;
            }
            if media_ids.insert(file_id, id).is_some() {
                return invalid(format!("media id {file_id} is used twice"));
            }
        }
    }

    let seq = doc
        .get("sequence")
        .ok_or_else(|| LoadError::Invalid("missing `sequence`".into()))?;
    p.sequence.name = arg_str(seq).unwrap_or("Main").into();
    let num = get_int(seq, "fps")?.unwrap_or(24);
    let den = get_int(seq, "fps-den")?.unwrap_or(1);
    if num <= 0 || den <= 0 {
        return invalid("frame rate must be positive");
    }
    p.sequence.fps = Fps::new(num as u32, den as u32);
    p.sequence.width = get_int(seq, "width")?.unwrap_or(1920).max(1) as u32;
    p.sequence.height = get_int(seq, "height")?.unwrap_or(1080).max(1) as u32;

    for tn in children(seq).iter().filter(|n| n.name().value() == "track") {
        let label =
            arg_str(tn).ok_or_else(|| LoadError::Invalid("track without a label".into()))?;
        let kind = match get_str(tn, "kind")?.unwrap_or("video") {
            "video" => StreamKind::Video,
            "audio" => StreamKind::Audio,
            other => return invalid(format!("unknown track kind `{other}`")),
        };
        let track = p.push_track(label, get_str(tn, "name")?.unwrap_or(""), kind);
        {
            let t = p.sequence.tracks.last_mut().expect("just pushed");
            t.enabled = get_bool(tn, "enabled", true)?;
            t.locked = get_bool(tn, "locked", false)?;
        }

        for cn in children(tn).iter().filter(|n| n.name().value() == "clip") {
            let start = req_int(cn, "start")?;
            let duration = req_int(cn, "duration")?;
            if start < 0 || duration < 1 {
                return invalid(format!(
                    "clip on {label} has a negative start or empty duration"
                ));
            }
            let media = match get_int(cn, "media")? {
                Some(fid) => Some(*media_ids.get(&fid).ok_or_else(|| {
                    LoadError::Invalid(format!("clip refers to unknown media {fid}"))
                })?),
                None => None,
            };
            let mut nodes = Vec::new();
            for nn in children(cn) {
                let ident = nn.name().value();
                let kind = NodeKind::from_ident(ident)
                    .ok_or_else(|| LoadError::Invalid(format!("unknown node `{ident}`")))?;
                let mut node = p.new_node(kind);
                for spec in kind.params() {
                    let value = match (spec.kind, nn.get(spec.key)) {
                        (_, None) => continue,
                        (ParamKind::Number { .. }, Some(KdlValue::Integer(i))) => {
                            Value::Number(*i as f64)
                        }
                        (ParamKind::Number { .. }, Some(KdlValue::Float(f))) => Value::Number(*f),
                        (_, Some(KdlValue::String(s))) => Value::Text(s.clone()),
                        _ => return invalid(format!("`{ident}`: bad value for `{}`", spec.key)),
                    };
                    node.set(spec.key, value)
                        .map_err(|e| LoadError::Invalid(format!("`{ident}`: {e}")))?;
                }
                node.bypass = get_bool(nn, "bypass", false)? && !kind.is_generator();
                nodes.push(node);
            }
            let kind = match (media, nodes.first().map(|n| n.kind)) {
                (Some(m), Some(NodeKind::Source)) => {
                    p.media(m).expect("mapped above").kind.clip_kind()
                }
                (None, Some(NodeKind::Text)) => ClipKind::Text,
                _ => {
                    return invalid(format!(
                        "clip on {label} must start with `source` (with media) or `text`"
                    ));
                }
            };
            if kind.stream() != kind_of_track(&p, track) {
                return invalid(format!("{label} can't hold a {kind:?} clip"));
            }
            if nodes[1..].iter().any(|n| !n.kind.applies_to(kind)) {
                return invalid(format!(
                    "clip on {label} has a node that doesn't apply to it"
                ));
            }
            if p.place(track, start, duration, None) != Some(start) {
                return invalid(format!("clips overlap on {label} at frame {start}"));
            }
            let id = ClipId(p.alloc());
            p.sequence.clips.push(Clip {
                id,
                track,
                kind,
                start,
                duration,
                media,
                source_in: get_int(cn, "in")?.unwrap_or(0).max(0),
                nodes,
            });
        }
    }

    if let Some(o) = doc.get("output") {
        let d = OutputSettings::default();
        let s = |key: &str, default: &str| -> Result<String, LoadError> {
            Ok(get_str(o, key)?.unwrap_or(default).to_string())
        };
        p.output = OutputSettings {
            container: s("container", &d.container)?,
            codec: s("codec", &d.codec)?,
            resolution: s("resolution", &d.resolution)?,
            quality: s("quality", &d.quality)?,
            hardware: s("hardware", &d.hardware)?,
            path: s("path", &d.path)?,
        };
    }
    Ok(p)
}

fn kind_of_track(p: &Project, id: crate::project::TrackId) -> StreamKind {
    p.track(id).expect("track exists").kind
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Projects compare equal after a round trip, apart from regenerated ids.
    fn normalized(p: &Project) -> String {
        to_kdl(p)
    }

    #[test]
    fn demo_round_trips() {
        let p = crate::demo::demo_project();
        let text = to_kdl(&p);
        let back = from_kdl(&text).expect("re-parse");
        assert_eq!(normalized(&p), normalized(&back));
        assert_eq!(back.sequence.clips.len(), p.sequence.clips.len());
        assert!(
            text.contains("grade exposure=-0.2 temperature=5200"),
            "{text}"
        );
    }

    #[test]
    fn empty_project_round_trips() {
        let p = Project::new("Empty");
        let back = from_kdl(&to_kdl(&p)).unwrap();
        assert_eq!(back.name, "Empty");
        assert_eq!(back.sequence.tracks.len(), 4);
    }

    #[test]
    fn rejects_bad_files() {
        assert!(matches!(from_kdl("{{{"), Err(LoadError::Syntax(_))));
        assert_eq!(from_kdl("project \"x\""), Err(LoadError::NotAProject));
        assert_eq!(from_kdl("videode version=99"), Err(LoadError::TooNew(99)));
        let overlap = r#"
            videode version=1
            sequence "Main" {
                track V2 kind=video { clip start=0 duration=10 { text }; clip start=5 duration=10 { text } }
            }"#;
        assert!(matches!(from_kdl(overlap), Err(LoadError::Invalid(m)) if m.contains("overlap")));
        let bad_node = r#"
            videode version=1
            sequence "Main" { track A1 kind=audio { clip start=0 duration=10 { text } } }"#;
        assert!(from_kdl(bad_node).is_err());
        let unknown_media = r#"
            videode version=1
            sequence "Main" { track V1 kind=video { clip start=0 duration=10 media=7 { source } } }"#;
        assert!(
            matches!(from_kdl(unknown_media), Err(LoadError::Invalid(m)) if m.contains("unknown media"))
        );
    }

    #[test]
    fn clamps_params_on_load() {
        let text = r#"
            videode version=1
            sequence "Main" { track V2 kind=video { clip start=0 duration=10 { text size=9000; transform scale=50.5 } } }"#;
        let p = from_kdl(text).unwrap();
        let c = &p.sequence.clips[0];
        assert_eq!(c.nodes[0].number("size"), 400.0);
        assert_eq!(c.nodes[1].number("scale"), 51.0);
    }
}
