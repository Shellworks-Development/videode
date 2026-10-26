//! The project model: media, one sequence of tracks and clips, and the
//! output settings. Clips are chains of nodes (see [`crate::node`]).

use std::path::{Path, PathBuf};

use crate::node::{ClipKind, Node, NodeId, NodeKind, StreamKind};
use crate::time::{Fps, Frame};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClipId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Video,
    Audio,
    Image,
}

impl MediaKind {
    pub fn clip_kind(self) -> ClipKind {
        match self {
            MediaKind::Audio => ClipKind::Audio,
            MediaKind::Video | MediaKind::Image => ClipKind::Video,
        }
    }

    pub fn ident(self) -> &'static str {
        match self {
            MediaKind::Video => "video",
            MediaKind::Audio => "audio",
            MediaKind::Image => "image",
        }
    }

    pub fn from_ident(s: &str) -> Option<Self> {
        match s {
            "video" => Some(MediaKind::Video),
            "audio" => Some(MediaKind::Audio),
            "image" => Some(MediaKind::Image),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Media {
    pub id: MediaId,
    pub path: PathBuf,
    pub kind: MediaKind,
    /// Length in sequence frames; `None` for stills, which can be any length.
    pub duration: Option<Frame>,
    pub codec: String,
    /// Free-form format description, e.g. `1920×1080` or `Stereo`.
    pub format: String,
    /// The file could not be found when the project was opened.
    pub offline: bool,
}

impl Media {
    pub fn file_name(&self) -> String {
        file_name(&self.path)
    }

    pub fn stem(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.file_name())
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: TrackId,
    /// Short label such as `V1` or `A2`.
    pub label: String,
    pub name: String,
    pub kind: StreamKind,
    pub enabled: bool,
    pub locked: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: ClipId,
    pub track: TrackId,
    pub kind: ClipKind,
    pub start: Frame,
    pub duration: Frame,
    pub media: Option<MediaId>,
    /// First source frame used, for media clips.
    pub source_in: Frame,
    /// The clip's node chain; `nodes[0]` is always its generator.
    pub nodes: Vec<Node>,
}

impl Clip {
    pub fn end(&self) -> Frame {
        self.start + self.duration
    }

    pub fn contains(&self, frame: Frame) -> bool {
        frame >= self.start && frame < self.end()
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_index(&self, id: NodeId) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    pub fn first(&self, kind: NodeKind) -> Option<&Node> {
        self.nodes.iter().find(|n| n.kind == kind)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sequence {
    pub name: String,
    pub fps: Fps,
    pub width: u32,
    pub height: u32,
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
}

/// One setting on the Output node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutputField {
    Container,
    Codec,
    Resolution,
    Quality,
    Hardware,
    Path,
}

impl OutputField {
    pub const ALL: [OutputField; 6] = [
        OutputField::Container,
        OutputField::Codec,
        OutputField::Resolution,
        OutputField::Quality,
        OutputField::Hardware,
        OutputField::Path,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OutputField::Container => "Container",
            OutputField::Codec => "Codec",
            OutputField::Resolution => "Resolution",
            OutputField::Quality => "Quality",
            OutputField::Hardware => "Hardware",
            OutputField::Path => "File",
        }
    }

    /// Every value the field can take; empty for free text. Which codecs
    /// fit depends on the container, see [`OutputSettings::choices`].
    pub fn options(self) -> &'static [&'static str] {
        match self {
            OutputField::Container => &["MP4", "MOV", "MKV", "WebM", "PNG sequence", "WAV"],
            OutputField::Codec => &[
                "H.264",
                "H.265",
                "AV1",
                "VP9",
                "ProRes 422",
                "ProRes 422 HQ",
                "PNG",
                "PCM",
            ],
            OutputField::Resolution => &[
                "Sequence",
                "3840×2160",
                "1920×1080",
                "1280×720",
                "1080×1920",
            ],
            OutputField::Quality => &["Draft", "Standard", "High", "Lossless"],
            OutputField::Hardware => &["Auto", "Off"],
            OutputField::Path => &[],
        }
    }
}

/// The codecs a container can hold, the default first.
pub fn container_codecs(container: &str) -> &'static [&'static str] {
    match container {
        "MP4" => &["H.264", "H.265", "AV1"],
        "MOV" => &["H.264", "H.265", "ProRes 422", "ProRes 422 HQ"],
        "MKV" => &[
            "H.264",
            "H.265",
            "AV1",
            "VP9",
            "ProRes 422",
            "ProRes 422 HQ",
        ],
        "WebM" => &["VP9", "AV1"],
        "PNG sequence" => &["PNG"],
        "WAV" => &["PCM"],
        _ => &[],
    }
}

/// File extension for a container name.
pub fn container_extension(container: &str) -> Option<&'static str> {
    match container {
        "MP4" => Some("mp4"),
        "MOV" => Some("mov"),
        "MKV" => Some("mkv"),
        "WebM" => Some("webm"),
        "PNG sequence" => Some("png"),
        "WAV" => Some("wav"),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputSettings {
    pub container: String,
    pub codec: String,
    pub resolution: String,
    pub quality: String,
    /// `Auto` uses a hardware encoder when there is one.
    pub hardware: String,
    pub path: String,
}

impl OutputSettings {
    pub fn get(&self, field: OutputField) -> &str {
        match field {
            OutputField::Container => &self.container,
            OutputField::Codec => &self.codec,
            OutputField::Resolution => &self.resolution,
            OutputField::Quality => &self.quality,
            OutputField::Hardware => &self.hardware,
            OutputField::Path => &self.path,
        }
    }

    /// The values a field can take now: codecs follow the container.
    pub fn choices(&self, field: OutputField) -> &'static [&'static str] {
        match field {
            OutputField::Codec => container_codecs(&self.container),
            _ => field.options(),
        }
    }

    /// Whether the output has a picture (WAV is sound only).
    pub fn has_video(&self) -> bool {
        self.container != "WAV"
    }
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            container: "MP4".into(),
            codec: "H.264".into(),
            resolution: "Sequence".into(),
            quality: "High".into(),
            hardware: "Auto".into(),
            path: "export.mp4".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub name: String,
    pub media: Vec<Media>,
    pub sequence: Sequence,
    pub output: OutputSettings,
    pub(crate) next_id: u64,
}

impl Default for Project {
    fn default() -> Self {
        Self::new("Untitled project")
    }
}

impl Project {
    /// An empty project with the standard four tracks (V2, V1, A1, A2).
    pub fn new(name: impl Into<String>) -> Self {
        let mut p = Project {
            name: name.into(),
            media: Vec::new(),
            sequence: Sequence {
                name: "Main".into(),
                fps: Fps::FPS_24,
                width: 1920,
                height: 1080,
                tracks: Vec::new(),
                clips: Vec::new(),
            },
            output: OutputSettings::default(),
            next_id: 1,
        };
        for (label, name, kind) in [
            ("V2", "Titles", StreamKind::Video),
            ("V1", "Picture", StreamKind::Video),
            ("A1", "Voice", StreamKind::Audio),
            ("A2", "Music", StreamKind::Audio),
        ] {
            p.push_track(label, name, kind);
        }
        p
    }

    pub(crate) fn alloc(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn push_track(&mut self, label: &str, name: &str, kind: StreamKind) -> TrackId {
        let id = TrackId(self.alloc());
        self.sequence.tracks.push(Track {
            id,
            label: label.into(),
            name: name.into(),
            kind,
            enabled: true,
            locked: false,
        });
        id
    }

    pub fn add_media(
        &mut self,
        path: impl Into<PathBuf>,
        kind: MediaKind,
        duration: Option<Frame>,
        codec: impl Into<String>,
        format: impl Into<String>,
    ) -> MediaId {
        let id = MediaId(self.alloc());
        self.media.push(Media {
            id,
            path: path.into(),
            kind,
            duration,
            codec: codec.into(),
            format: format.into(),
            offline: false,
        });
        id
    }

    pub fn new_node(&mut self, kind: NodeKind) -> Node {
        Node::new(NodeId(self.alloc()), kind)
    }

    pub fn media(&self, id: MediaId) -> Option<&Media> {
        self.media.iter().find(|m| m.id == id)
    }

    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.sequence.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_by_label(&self, label: &str) -> Option<&Track> {
        self.sequence.tracks.iter().find(|t| t.label == label)
    }

    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.sequence.clips.iter().find(|c| c.id == id)
    }

    pub(crate) fn clip_mut(&mut self, id: ClipId) -> Option<&mut Clip> {
        self.sequence.clips.iter_mut().find(|c| c.id == id)
    }

    /// Clips on a track, in time order.
    pub fn clips_on(&self, track: TrackId) -> Vec<&Clip> {
        let mut v: Vec<&Clip> = self
            .sequence
            .clips
            .iter()
            .filter(|c| c.track == track)
            .collect();
        v.sort_by_key(|c| c.start);
        v
    }

    pub fn clip_at(&self, track: TrackId, frame: Frame) -> Option<&Clip> {
        self.sequence
            .clips
            .iter()
            .find(|c| c.track == track && c.contains(frame))
    }

    /// Display name: the media file stem, or the quoted text of a text clip.
    pub fn clip_name(&self, clip: &Clip) -> String {
        match clip.media.and_then(|m| self.media(m)) {
            Some(m) => m.stem(),
            None => match clip.first(NodeKind::Text) {
                Some(t) => format!("“{}”", t.text("text")),
                None => "Clip".into(),
            },
        }
    }

    /// The last frame covered by any clip.
    pub fn end(&self) -> Frame {
        self.sequence.clips.iter().map(Clip::end).max().unwrap_or(0)
    }

    pub fn node_count(&self) -> usize {
        // Clip nodes plus one per track and Composite, Mix, Sequence, Output.
        self.sequence
            .clips
            .iter()
            .map(|c| c.nodes.len())
            .sum::<usize>()
            + self.sequence.tracks.len()
            + 4
    }

    /// Every clip boundary, sorted and deduplicated.
    pub fn edit_points(&self) -> Vec<Frame> {
        let mut v: Vec<Frame> = self
            .sequence
            .clips
            .iter()
            .flat_map(|c| [c.start, c.end()])
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}
