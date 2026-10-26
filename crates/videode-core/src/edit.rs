//! Editing operations. Each validates against locks, track kinds, media
//! bounds and overlaps, and either applies fully or returns an error
//! without changing the project.

use std::path::PathBuf;

use crate::node::{ClipKind, NodeId, NodeKind, ParamError, Value};
use crate::project::{Clip, ClipId, MediaId, OutputField, Project, TrackId, container_extension};
use crate::time::Frame;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("that clip no longer exists")]
    NoSuchClip,
    #[error("that track no longer exists")]
    NoSuchTrack,
    #[error("that node no longer exists")]
    NoSuchNode,
    #[error("that media no longer exists")]
    NoSuchMedia,
    #[error("{0} is locked")]
    TrackLocked(String),
    #[error("{0} can't hold this kind of clip")]
    WrongTrackKind(String),
    #[error("no room on {0} for this clip")]
    NoRoom(String),
    #[error("the frame isn't inside the clip")]
    OutsideClip,
    #[error("{node} doesn't work on {kind} clips")]
    NotApplicable {
        node: &'static str,
        kind: &'static str,
    },
    #[error("{0} is what the clip is made from; delete the clip instead")]
    GeneratorFixed(&'static str),
    #[error("{0} can't be {1}")]
    InvalidOutput(&'static str, String),
    #[error(transparent)]
    Param(#[from] ParamError),
}

pub type EditResult<T> = Result<T, EditError>;

fn kind_name(kind: ClipKind) -> &'static str {
    match kind {
        ClipKind::Video => "video",
        ClipKind::Audio => "audio",
        ClipKind::Text => "text",
    }
}

/// Default length for clips of media without a duration (stills, text).
pub const DEFAULT_STILL_SECONDS: i64 = 5;

impl Project {
    fn editable_track(&self, id: TrackId) -> EditResult<&crate::project::Track> {
        let t = self.track(id).ok_or(EditError::NoSuchTrack)?;
        if t.locked {
            return Err(EditError::TrackLocked(t.label.clone()));
        }
        Ok(t)
    }

    fn editable_clip(&self, id: ClipId) -> EditResult<&Clip> {
        let c = self.clip(id).ok_or(EditError::NoSuchClip)?;
        self.editable_track(c.track)?;
        Ok(c)
    }

    /// Free intervals `[start, end)` on a track, ignoring `exclude`. The last
    /// interval is open-ended (`Frame::MAX`).
    pub fn free_gaps(&self, track: TrackId, exclude: Option<ClipId>) -> Vec<(Frame, Frame)> {
        let mut gaps = Vec::new();
        let mut cursor = 0;
        for c in self.clips_on(track) {
            if Some(c.id) == exclude {
                continue;
            }
            if c.start > cursor {
                gaps.push((cursor, c.start));
            }
            cursor = cursor.max(c.end());
        }
        gaps.push((cursor, Frame::MAX));
        gaps
    }

    /// The start closest to `desired` where `duration` frames fit on
    /// `track` without overlapping anything except `exclude`.
    pub fn place(
        &self,
        track: TrackId,
        desired: Frame,
        duration: Frame,
        exclude: Option<ClipId>,
    ) -> Option<Frame> {
        self.free_gaps(track, exclude)
            .into_iter()
            .filter(|(s, e)| e.saturating_sub(*s) >= duration)
            .map(|(s, e)| desired.clamp(s, e.saturating_sub(duration)))
            .min_by_key(|start| (start - desired).abs())
    }

    fn check_track_kind(&self, track: TrackId, kind: ClipKind) -> EditResult<()> {
        let t = self.track(track).ok_or(EditError::NoSuchTrack)?;
        if t.kind != kind.stream() {
            return Err(EditError::WrongTrackKind(t.label.clone()));
        }
        Ok(())
    }

    /// Adds a clip for `media`. With no track given, picks the first
    /// unlocked track of the right kind with room at `at`, then any with room.
    pub fn add_media_clip(
        &mut self,
        media: MediaId,
        track: Option<TrackId>,
        at: Frame,
    ) -> EditResult<ClipId> {
        let m = self.media(media).ok_or(EditError::NoSuchMedia)?;
        let kind = m.kind.clip_kind();
        let duration = m
            .duration
            .unwrap_or(DEFAULT_STILL_SECONDS * self.sequence.fps.nominal());
        let at = at.max(0);
        let (track, start) = match track {
            Some(t) => {
                self.editable_track(t)?;
                self.check_track_kind(t, kind)?;
                let label = self.track(t).map(|t| t.label.clone()).unwrap_or_default();
                (
                    t,
                    self.place(t, at, duration, None)
                        .ok_or(EditError::NoRoom(label))?,
                )
            }
            None => {
                let candidates: Vec<TrackId> = self
                    .sequence
                    .tracks
                    .iter()
                    .filter(|t| t.kind == kind.stream() && !t.locked)
                    .map(|t| t.id)
                    .collect();
                let exact = candidates
                    .iter()
                    .find(|t| self.place(**t, at, duration, None) == Some(at));
                match exact.or(candidates.first()) {
                    Some(&t) => (t, self.place(t, at, duration, None).unwrap_or(at)),
                    None => return Err(EditError::WrongTrackKind("any unlocked track".into())),
                }
            }
        };
        let source = self.new_node(NodeKind::Source);
        let id = ClipId(self.alloc());
        self.sequence.clips.push(Clip {
            id,
            track,
            kind,
            start,
            duration,
            media: Some(media),
            source_in: 0,
            nodes: vec![source],
        });
        Ok(id)
    }

    pub fn add_text_clip(
        &mut self,
        track: TrackId,
        at: Frame,
        duration: Frame,
        text: &str,
    ) -> EditResult<ClipId> {
        self.editable_track(track)?;
        self.check_track_kind(track, ClipKind::Text)?;
        let label = self
            .track(track)
            .map(|t| t.label.clone())
            .unwrap_or_default();
        let duration = duration.max(1);
        let start = self
            .place(track, at.max(0), duration, None)
            .ok_or(EditError::NoRoom(label))?;
        let mut generator = self.new_node(NodeKind::Text);
        generator.set("text", Value::Text(text.into()))?;
        let id = ClipId(self.alloc());
        self.sequence.clips.push(Clip {
            id,
            track,
            kind: ClipKind::Text,
            start,
            duration,
            media: None,
            source_in: 0,
            nodes: vec![generator],
        });
        Ok(id)
    }

    /// Moves a clip as close to `desired` on `track` as fits. Returns the
    /// start actually used.
    pub fn move_clip(&mut self, id: ClipId, track: TrackId, desired: Frame) -> EditResult<Frame> {
        let c = self.editable_clip(id)?;
        let (kind, duration) = (c.kind, c.duration);
        self.editable_track(track)?;
        self.check_track_kind(track, kind)?;
        let label = self
            .track(track)
            .map(|t| t.label.clone())
            .unwrap_or_default();
        let start = self
            .place(track, desired.max(0), duration, Some(id))
            .ok_or(EditError::NoRoom(label))?;
        let c = self.clip_mut(id).expect("checked above");
        c.track = track;
        c.start = start;
        Ok(start)
    }

    /// Bounds a clip's edges may be trimmed to: neighbours on its track and,
    /// for media with a length, the media itself.
    fn trim_bounds(&self, c: &Clip) -> (Frame, Frame) {
        let gaps = self.free_gaps(c.track, Some(c.id));
        let (gap_s, gap_e) = gaps
            .into_iter()
            .find(|(s, e)| *s <= c.start && *e >= c.end())
            .unwrap_or((c.start, c.end()));
        let media_len = c.media.and_then(|m| self.media(m)).and_then(|m| m.duration);
        match media_len {
            Some(len) => (
                gap_s.max(c.start - c.source_in),
                gap_e.min(c.start - c.source_in + len),
            ),
            None => (gap_s, gap_e),
        }
    }

    /// Moves the clip's in point, keeping its end fixed. Returns the new start.
    pub fn trim_start(&mut self, id: ClipId, desired: Frame) -> EditResult<Frame> {
        let c = self.editable_clip(id)?;
        let (lo, _) = self.trim_bounds(c);
        let new_start = desired.clamp(lo, c.end() - 1);
        let c = self.clip_mut(id).expect("checked above");
        let delta = new_start - c.start;
        c.start = new_start;
        c.duration -= delta;
        if c.media.is_some() {
            c.source_in += delta;
        }
        Ok(new_start)
    }

    /// Moves the clip's out point, keeping its start fixed. Returns the new end.
    pub fn trim_end(&mut self, id: ClipId, desired: Frame) -> EditResult<Frame> {
        let c = self.editable_clip(id)?;
        let (_, hi) = self.trim_bounds(c);
        let new_end = desired.clamp(c.start + 1, hi);
        let c = self.clip_mut(id).expect("checked above");
        c.duration = new_end - c.start;
        Ok(new_end)
    }

    /// Cuts a clip in two at `at`. The right half gets fresh node ids; fades
    /// are cleared on the cut side of each half. Returns the right half.
    pub fn split(&mut self, id: ClipId, at: Frame) -> EditResult<ClipId> {
        let c = self.editable_clip(id)?;
        if at <= c.start || at >= c.end() {
            return Err(EditError::OutsideClip);
        }
        let mut right = c.clone();
        right.id = ClipId(self.alloc());
        for n in &mut right.nodes {
            n.id = NodeId(self.alloc());
        }
        let offset = at - right.start;
        right.start = at;
        right.duration -= offset;
        if right.media.is_some() {
            right.source_in += offset;
        }
        clear_fade(&mut right, "in");
        let left = self.clip_mut(id).expect("checked above");
        left.duration = offset;
        clear_fade(left, "out");
        let right_id = right.id;
        self.sequence.clips.push(right);
        Ok(right_id)
    }

    pub fn delete_clip(&mut self, id: ClipId) -> EditResult<()> {
        self.editable_clip(id)?;
        self.sequence.clips.retain(|c| c.id != id);
        Ok(())
    }

    /// Deletes a clip and closes the gap by pulling later clips on the same
    /// track left.
    pub fn ripple_delete(&mut self, id: ClipId) -> EditResult<()> {
        let c = self.editable_clip(id)?;
        let (track, end, duration) = (c.track, c.end(), c.duration);
        self.sequence.clips.retain(|c| c.id != id);
        for c in self.sequence.clips.iter_mut() {
            if c.track == track && c.start >= end {
                c.start -= duration;
            }
        }
        Ok(())
    }

    pub fn add_node(&mut self, clip: ClipId, kind: NodeKind) -> EditResult<NodeId> {
        let c = self.editable_clip(clip)?;
        if !kind.applies_to(c.kind) {
            return Err(EditError::NotApplicable {
                node: kind.name(),
                kind: kind_name(c.kind),
            });
        }
        let node = self.new_node(kind);
        let id = node.id;
        self.clip_mut(clip).expect("checked above").nodes.push(node);
        Ok(id)
    }

    fn processing_node_index(&self, clip: ClipId, node: NodeId) -> EditResult<usize> {
        let c = self.editable_clip(clip)?;
        let i = c.node_index(node).ok_or(EditError::NoSuchNode)?;
        if c.nodes[i].kind.is_generator() {
            return Err(EditError::GeneratorFixed(c.nodes[i].kind.name()));
        }
        Ok(i)
    }

    /// Points a media item at a different file; clips using it follow.
    pub fn relink(&mut self, media: MediaId, path: PathBuf) -> EditResult<()> {
        let m = self
            .media
            .iter_mut()
            .find(|m| m.id == media)
            .ok_or(EditError::NoSuchMedia)?;
        m.path = path;
        Ok(())
    }

    pub fn remove_node(&mut self, clip: ClipId, node: NodeId) -> EditResult<()> {
        let i = self.processing_node_index(clip, node)?;
        self.clip_mut(clip).expect("checked above").nodes.remove(i);
        Ok(())
    }

    /// Moves a node within the chain. Index 0 belongs to the generator, so
    /// the target is clamped to `1..len`.
    pub fn move_node(&mut self, clip: ClipId, node: NodeId, index: usize) -> EditResult<usize> {
        let i = self.processing_node_index(clip, node)?;
        let c = self.clip_mut(clip).expect("checked above");
        let index = index.clamp(1, c.nodes.len() - 1);
        let n = c.nodes.remove(i);
        c.nodes.insert(index, n);
        Ok(index)
    }

    pub fn set_bypass(&mut self, clip: ClipId, node: NodeId, bypass: bool) -> EditResult<()> {
        let i = self.processing_node_index(clip, node)?;
        self.clip_mut(clip).expect("checked above").nodes[i].bypass = bypass;
        Ok(())
    }

    pub fn set_param(
        &mut self,
        clip: ClipId,
        node: NodeId,
        key: &str,
        value: Value,
    ) -> EditResult<()> {
        let c = self.editable_clip(clip)?;
        let i = c.node_index(node).ok_or(EditError::NoSuchNode)?;
        self.clip_mut(clip).expect("checked above").nodes[i].set(key, value)?;
        Ok(())
    }

    /// Changes one Output setting. Changing the container also fixes up a
    /// known file extension on the output path.
    pub fn set_output(&mut self, field: OutputField, value: &str) -> EditResult<()> {
        let options = field.options();
        let value = value.trim();
        let ok = if options.is_empty() {
            !value.is_empty()
        } else {
            options.contains(&value)
        };
        if !ok {
            return Err(EditError::InvalidOutput(field.label(), value.to_string()));
        }
        let o = &mut self.output;
        match field {
            OutputField::Container => {
                if let (Some(ext), Some((stem, old))) =
                    (container_extension(value), o.path.rsplit_once('.'))
                    && ["mp4", "mov", "mkv", "webm"].contains(&old.to_ascii_lowercase().as_str())
                {
                    o.path = format!("{stem}.{ext}");
                }
                o.container = value.into();
            }
            OutputField::Codec => o.codec = value.into(),
            OutputField::Resolution => o.resolution = value.into(),
            OutputField::Quality => o.quality = value.into(),
            OutputField::Path => o.path = value.into(),
        }
        Ok(())
    }

    pub fn set_track_enabled(&mut self, track: TrackId, enabled: bool) -> EditResult<()> {
        let t = self
            .sequence
            .tracks
            .iter_mut()
            .find(|t| t.id == track)
            .ok_or(EditError::NoSuchTrack)?;
        t.enabled = enabled;
        Ok(())
    }

    pub fn set_track_locked(&mut self, track: TrackId, locked: bool) -> EditResult<()> {
        let t = self
            .sequence
            .tracks
            .iter_mut()
            .find(|t| t.id == track)
            .ok_or(EditError::NoSuchTrack)?;
        t.locked = locked;
        Ok(())
    }

    /// Frames a dragged edge may snap to: sequence start, clip edges (except
    /// `exclude`) and any `extra` points such as the playhead.
    pub fn snap_points(&self, exclude: Option<ClipId>, extra: &[Frame]) -> Vec<Frame> {
        let mut v = vec![0];
        v.extend(extra);
        for c in &self.sequence.clips {
            if Some(c.id) != exclude {
                v.push(c.start);
                v.push(c.end());
            }
        }
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// The nearest point within `tolerance` frames, if any.
pub fn snap(frame: Frame, points: &[Frame], tolerance: Frame) -> Option<Frame> {
    points
        .iter()
        .copied()
        .filter(|p| (p - frame).abs() <= tolerance)
        .min_by_key(|p| (p - frame).abs())
}

fn clear_fade(clip: &mut Clip, side: &str) {
    for n in clip.nodes.iter_mut().filter(|n| n.kind == NodeKind::Fade) {
        let _ = n.set(side, Value::Number(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::MediaKind;

    /// V1 holds a (0..100) and b (150..250) from 400-frame media; A1 is empty.
    fn fixture() -> (Project, ClipId, ClipId, TrackId, TrackId, MediaId) {
        let mut p = Project::new("t");
        let v1 = p.track_by_label("V1").unwrap().id;
        let a1 = p.track_by_label("A1").unwrap().id;
        let m = p.add_media("a.mov", MediaKind::Video, Some(400), "H.264", "1920×1080");
        let a = p.add_media_clip(m, Some(v1), 0).unwrap();
        p.trim_end(a, 100).unwrap();
        let b = p.add_media_clip(m, Some(v1), 150).unwrap();
        p.trim_end(b, 250).unwrap();
        (p, a, b, v1, a1, m)
    }

    #[test]
    fn relink_points_media_at_a_new_file() {
        let (mut p, _, _, _, _, m) = fixture();
        p.relink(m, "/new/a.mov".into()).unwrap();
        assert_eq!(p.media(m).unwrap().path, PathBuf::from("/new/a.mov"));
        assert_eq!(
            p.relink(MediaId(999), "x".into()),
            Err(EditError::NoSuchMedia)
        );
    }

    #[test]
    fn add_picks_track_and_room() {
        let (mut p, _, _, v1, _, m) = fixture();
        // At 50 V1 is occupied, so the clip goes to V2.
        let c = p.add_media_clip(m, None, 50).unwrap();
        assert_eq!(p.track(p.clip(c).unwrap().track).unwrap().label, "V2");
        // Forcing V1 places it in the nearest gap that fits 400 frames.
        let d = p.add_media_clip(m, Some(v1), 50).unwrap();
        assert_eq!(p.clip(d).unwrap().start, 250);
    }

    #[test]
    fn add_rejects_wrong_track_kind() {
        let (mut p, _, _, _, a1, m) = fixture();
        assert_eq!(
            p.add_media_clip(m, Some(a1), 0),
            Err(EditError::WrongTrackKind("A1".into()))
        );
    }

    #[test]
    fn move_clamps_to_neighbours() {
        let (mut p, a, _, v1, a1, _) = fixture();
        // b starts at 150; a is 100 long so it can go up to 50.
        assert_eq!(p.move_clip(a, v1, 120).unwrap(), 50);
        assert_eq!(p.move_clip(a, v1, -30).unwrap(), 0);
        // Far right: jumps past b into the open gap.
        assert_eq!(p.move_clip(a, v1, 400).unwrap(), 400);
        assert!(matches!(
            p.move_clip(a, a1, 0),
            Err(EditError::WrongTrackKind(_))
        ));
    }

    #[test]
    fn trims_respect_media_and_neighbours() {
        let (mut p, a, b, _, _, _) = fixture();
        // a's end can grow to b's start.
        assert_eq!(p.trim_end(a, 500).unwrap(), 150);
        // b's start can't pass a's end now.
        assert_eq!(p.trim_start(b, 100).unwrap(), 150);
        // Trimming b's start right moves its source in point too.
        assert_eq!(p.trim_start(b, 170).unwrap(), 170);
        let bc = p.clip(b).unwrap();
        assert_eq!((bc.source_in, bc.duration), (20, 80));
        // b's end is limited by the media length (400 - 20 source frames left).
        assert_eq!(p.trim_end(b, 9_999).unwrap(), 170 - 20 + 400);
        // Can't trim to zero length.
        assert_eq!(p.trim_end(b, 0).unwrap(), 171);
    }

    #[test]
    fn split_and_fades() {
        let (mut p, a, _, _, _, _) = fixture();
        p.add_node(a, NodeKind::Fade).unwrap();
        let r = p.split(a, 40).unwrap();
        let (l, rc) = (p.clip(a).unwrap(), p.clip(r).unwrap());
        assert_eq!((l.start, l.duration), (0, 40));
        assert_eq!((rc.start, rc.duration, rc.source_in), (40, 60, 40));
        assert_eq!(l.first(NodeKind::Fade).unwrap().number("out"), 0.0);
        assert_eq!(rc.first(NodeKind::Fade).unwrap().number("in"), 0.0);
        assert_ne!(l.nodes[0].id, rc.nodes[0].id);
        assert_eq!(p.split(a, 0), Err(EditError::OutsideClip));
        assert_eq!(p.split(a, 40), Err(EditError::OutsideClip));
    }

    #[test]
    fn ripple_delete_closes_gap() {
        let (mut p, a, b, _, _, _) = fixture();
        p.ripple_delete(a).unwrap();
        assert_eq!(p.clip(b).unwrap().start, 50);
        assert!(p.clip(a).is_none());
    }

    #[test]
    fn locked_tracks_refuse_edits() {
        let (mut p, a, _, v1, _, _) = fixture();
        p.set_track_locked(v1, true).unwrap();
        assert_eq!(p.delete_clip(a), Err(EditError::TrackLocked("V1".into())));
        assert!(p.split(a, 10).is_err());
        assert!(p.add_node(a, NodeKind::Grade).is_err());
    }

    #[test]
    fn node_chain_ops() {
        let (mut p, a, _, _, _, _) = fixture();
        let g = p.add_node(a, NodeKind::Grade).unwrap();
        let t = p.add_node(a, NodeKind::Transform).unwrap();
        assert_eq!(p.move_node(a, t, 0).unwrap(), 1);
        let kinds: Vec<_> = p.clip(a).unwrap().nodes.iter().map(|n| n.kind).collect();
        assert_eq!(
            kinds,
            [NodeKind::Source, NodeKind::Transform, NodeKind::Grade]
        );
        let src = p.clip(a).unwrap().nodes[0].id;
        assert!(matches!(
            p.remove_node(a, src),
            Err(EditError::GeneratorFixed(_))
        ));
        assert!(matches!(
            p.add_node(a, NodeKind::Gain),
            Err(EditError::NotApplicable { .. })
        ));
        p.set_param(a, g, "exposure", Value::Number(1.0)).unwrap();
        p.set_bypass(a, g, true).unwrap();
        let gn = p.clip(a).unwrap().node(g).unwrap();
        assert!(gn.bypass && gn.number("exposure") == 1.0);
        p.remove_node(a, g).unwrap();
        assert_eq!(p.clip(a).unwrap().nodes.len(), 2);
    }

    #[test]
    fn text_clips() {
        let mut p = Project::new("t");
        let v2 = p.track_by_label("V2").unwrap().id;
        let a2 = p.track_by_label("A2").unwrap().id;
        let c = p.add_text_clip(v2, 10, 48, "Hello").unwrap();
        assert_eq!(p.clip_name(p.clip(c).unwrap()), "“Hello”");
        assert!(p.add_text_clip(a2, 0, 10, "x").is_err());
        assert!(p.add_node(c, NodeKind::Grade).is_err());
        assert!(p.add_node(c, NodeKind::Transform).is_ok());
    }

    #[test]
    fn snapping() {
        let (p, a, _, _, _, _) = fixture();
        let pts = p.snap_points(Some(a), &[77]);
        assert_eq!(pts, vec![0, 77, 150, 250]);
        assert_eq!(snap(153, &pts, 5), Some(150));
        assert_eq!(snap(160, &pts, 5), None);
    }

    #[test]
    fn output_settings_validate_and_fix_extension() {
        let mut p = Project::new("t");
        p.set_output(OutputField::Container, "MOV").unwrap();
        assert_eq!(p.output.container, "MOV");
        assert_eq!(p.output.path, "export.mov");
        assert!(p.set_output(OutputField::Codec, "DivX").is_err());
        assert!(p.set_output(OutputField::Path, "  ").is_err());
        p.set_output(OutputField::Path, "final cut.final").unwrap();
        p.set_output(OutputField::Container, "MKV").unwrap();
        // Unknown extensions are left alone.
        assert_eq!(p.output.path, "final cut.final");
    }

    #[test]
    fn node_chain_edits() {
        let (mut p, a, ..) = fixture();
        let t = p.add_node(a, NodeKind::Transform).unwrap();
        let g = p.add_node(a, NodeKind::Grade).unwrap();
        assert_eq!(
            p.move_node(a, g, 0).unwrap(),
            1,
            "can't go before the generator"
        );
        let kinds = |p: &Project| {
            p.clip(a)
                .unwrap()
                .nodes
                .iter()
                .map(|n| n.kind)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            kinds(&p),
            [NodeKind::Source, NodeKind::Grade, NodeKind::Transform]
        );
        p.set_bypass(a, t, true).unwrap();
        assert!(p.clip(a).unwrap().node(t).unwrap().bypass);
        let source = p.clip(a).unwrap().nodes[0].id;
        assert!(matches!(
            p.remove_node(a, source),
            Err(EditError::GeneratorFixed(_))
        ));
        assert!(matches!(
            p.add_node(a, NodeKind::Gain),
            Err(EditError::NotApplicable { .. })
        ));
    }
}
