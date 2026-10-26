//! A project being edited: undo/redo history, drag gestures and the saved
//! state used to show unsaved changes.
//!
//! History stores whole-project snapshots. Projects hold no media data, so a
//! snapshot is small; this keeps every edit trivially undoable.

use std::path::PathBuf;

use crate::edit::EditResult;
use crate::project::Project;

const HISTORY_LIMIT: usize = 500;

#[derive(Clone, Debug)]
struct Entry {
    label: String,
    project: Project,
}

#[derive(Debug)]
pub struct Document {
    pub project: Project,
    pub path: Option<PathBuf>,
    saved: Project,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    /// Consecutive edits with the same key merge into one undo step.
    coalesce: Option<String>,
    gesture: Option<Project>,
}

impl Document {
    pub fn new(project: Project, path: Option<PathBuf>) -> Self {
        Self {
            saved: project.clone(),
            project,
            path,
            undo: Vec::new(),
            redo: Vec::new(),
            coalesce: None,
            gesture: None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.project != self.saved
    }

    /// Re-checks which media files are missing. Being offline is a fact
    /// about the disk, not an edit, so this changes neither the undo history
    /// nor whether the document counts as unsaved.
    pub fn refresh_offline(&mut self, missing: impl Fn(&crate::project::Media) -> bool) -> usize {
        let mut offline = 0;
        for m in &mut self.project.media {
            m.offline = missing(m);
            offline += m.offline as usize;
        }
        let flags: Vec<_> = self
            .project
            .media
            .iter()
            .map(|m| (m.id, m.offline))
            .collect();
        for project in std::iter::once(&mut self.saved)
            .chain(self.undo.iter_mut().map(|e| &mut e.project))
            .chain(self.redo.iter_mut().map(|e| &mut e.project))
        {
            for m in &mut project.media {
                if let Some((_, off)) = flags.iter().find(|(id, _)| *id == m.id) {
                    m.offline = *off;
                }
            }
        }
        offline
    }

    /// Rewrites media paths everywhere they're kept, history included, so
    /// undo never brings back a path that no longer resolves. Not an edit.
    pub fn rebase_media(&mut self, mut f: impl FnMut(&mut crate::project::Media)) {
        for project in std::iter::once(&mut self.project)
            .chain(std::iter::once(&mut self.saved))
            .chain(self.undo.iter_mut().map(|e| &mut e.project))
            .chain(self.redo.iter_mut().map(|e| &mut e.project))
        {
            project.media.iter_mut().for_each(&mut f);
        }
    }

    /// Ends the current run of coalesced edits, so the next one starts a new
    /// undo step even if it has the same key (one per drag gesture).
    pub fn end_coalescing(&mut self) {
        self.coalesce = None;
    }

    pub fn mark_saved(&mut self, path: PathBuf) {
        self.coalesce = None;
        self.saved = self.project.clone();
        self.path = Some(path);
    }

    fn push(&mut self, label: &str, before: Project) {
        self.undo.push(Entry {
            label: label.into(),
            project: before,
        });
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Runs `f` as one undoable step. On error the project is left unchanged.
    pub fn edit<R>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Project) -> EditResult<R>,
    ) -> EditResult<R> {
        self.coalesce = None;
        let before = self.project.clone();
        match f(&mut self.project) {
            Ok(r) => {
                if self.project != before {
                    self.push(label, before);
                }
                Ok(r)
            }
            Err(e) => {
                self.project = before;
                Err(e)
            }
        }
    }

    /// Like [`Self::edit`], but repeated calls with the same `key` (for
    /// example scrubbing one parameter) collapse into a single undo step.
    pub fn edit_coalesced<R>(
        &mut self,
        label: &str,
        key: &str,
        f: impl FnOnce(&mut Project) -> EditResult<R>,
    ) -> EditResult<R> {
        let merging = self.coalesce.as_deref() == Some(key);
        let before = self.project.clone();
        match f(&mut self.project) {
            Ok(r) => {
                if self.project != before && !merging {
                    self.push(label, before);
                }
                self.coalesce = Some(key.into());
                Ok(r)
            }
            Err(e) => {
                self.project = before;
                Err(e)
            }
        }
    }

    /// Starts a continuous edit such as a drag. Each [`Self::gesture`] call
    /// replays from the starting state; [`Self::end_gesture`] records one
    /// undo step if anything changed.
    pub fn begin_gesture(&mut self) {
        self.coalesce = None;
        self.gesture = Some(self.project.clone());
    }

    pub fn in_gesture(&self) -> bool {
        self.gesture.is_some()
    }

    pub fn gesture<R>(&mut self, f: impl FnOnce(&mut Project) -> EditResult<R>) -> EditResult<R> {
        let base = self.gesture.clone().unwrap_or_else(|| self.project.clone());
        let mut next = base;
        let r = f(&mut next)?;
        self.project = next;
        Ok(r)
    }

    /// Returns whether the gesture changed the project.
    pub fn end_gesture(&mut self, label: &str) -> bool {
        match self.gesture.take() {
            Some(before) if before != self.project => {
                self.push(label, before);
                true
            }
            _ => false,
        }
    }

    pub fn cancel_gesture(&mut self) {
        if let Some(before) = self.gesture.take() {
            self.project = before;
        }
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    /// Returns the label of the step undone.
    pub fn undo(&mut self) -> Option<String> {
        self.coalesce = None;
        let e = self.undo.pop()?;
        let current = std::mem::replace(&mut self.project, e.project);
        self.redo.push(Entry {
            label: e.label.clone(),
            project: current,
        });
        Some(e.label)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.coalesce = None;
        let e = self.redo.pop()?;
        let current = std::mem::replace(&mut self.project, e.project);
        self.undo.push(Entry {
            label: e.label.clone(),
            project: current,
        });
        Some(e.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::EditError;
    use crate::node::{NodeKind, Value};
    use crate::project::MediaKind;

    fn doc() -> (Document, crate::project::ClipId) {
        let mut p = Project::new("t");
        let m = p.add_media("a.mov", MediaKind::Video, Some(100), "", "");
        let c = p.add_media_clip(m, None, 0).unwrap();
        (Document::new(p, None), c)
    }

    #[test]
    fn undo_redo() {
        let (mut d, c) = doc();
        assert!(!d.is_dirty());
        d.edit("split", |p| p.split(c, 50)).unwrap();
        assert_eq!(d.project.sequence.clips.len(), 2);
        assert!(d.is_dirty());
        assert_eq!(d.undo().as_deref(), Some("split"));
        assert_eq!(d.project.sequence.clips.len(), 1);
        assert!(!d.is_dirty());
        assert_eq!(d.redo().as_deref(), Some("split"));
        assert_eq!(d.project.sequence.clips.len(), 2);
        assert_eq!(d.redo(), None);
    }

    #[test]
    fn failed_edit_leaves_no_trace() {
        let (mut d, c) = doc();
        let r = d.edit("split", |p| p.split(c, 500));
        assert_eq!(r, Err(EditError::OutsideClip));
        assert_eq!(d.undo_label(), None);
    }

    #[test]
    fn coalesced_edits_merge() {
        let (mut d, c) = doc();
        let g = d.edit("add", |p| p.add_node(c, NodeKind::Grade)).unwrap();
        for v in [0.1, 0.2, 0.3] {
            d.edit_coalesced("exposure", "g.exposure", |p| {
                p.set_param(c, g, "exposure", Value::Number(v))
            })
            .unwrap();
        }
        assert_eq!(d.undo().as_deref(), Some("exposure"));
        assert_eq!(
            d.project
                .clip(c)
                .unwrap()
                .node(g)
                .unwrap()
                .number("exposure"),
            0.0
        );
        assert_eq!(d.undo().as_deref(), Some("add"));
    }

    #[test]
    fn gestures_record_one_step() {
        let (mut d, c) = doc();
        let v1 = d.project.clip(c).unwrap().track;
        d.begin_gesture();
        for x in [10, 20, 30] {
            d.gesture(|p| p.move_clip(c, v1, x)).unwrap();
        }
        assert!(d.end_gesture("move"));
        assert_eq!(d.project.clip(c).unwrap().start, 30);
        d.undo();
        assert_eq!(d.project.clip(c).unwrap().start, 0);

        d.begin_gesture();
        d.gesture(|p| p.move_clip(c, v1, 0)).unwrap();
        assert!(!d.end_gesture("move"));
    }

    #[test]
    fn offline_refresh_is_not_an_edit() {
        let (mut d, _) = doc();
        assert_eq!(d.refresh_offline(|_| true), 1);
        assert!(d.project.media[0].offline);
        assert!(!d.is_dirty());
        assert_eq!(d.undo_label(), None);
        d.edit("split", |p| {
            let c = p.sequence.clips[0].id;
            p.split(c, 50)
        })
        .unwrap();
        d.refresh_offline(|_| false);
        d.undo();
        assert!(
            !d.project.media[0].offline,
            "undo doesn't bring back stale offline flags"
        );
    }

    #[test]
    fn rebased_paths_survive_undo() {
        let (mut d, _) = doc();
        d.edit("split", |p| {
            let c = p.sequence.clips[0].id;
            p.split(c, 50)
        })
        .unwrap();
        d.rebase_media(|m| m.path = PathBuf::from("/abs").join(&m.path));
        d.undo();
        assert!(d.project.media[0].path.starts_with("/abs"));
        d.redo();
        assert!(d.project.media[0].path.starts_with("/abs"));
    }

    #[test]
    fn ending_coalescing_starts_a_new_step() {
        let (mut d, c) = doc();
        let g = d.edit("add", |p| p.add_node(c, NodeKind::Grade)).unwrap();
        for v in [0.1, 0.2] {
            d.edit_coalesced("exposure", "g.exposure", |p| {
                p.set_param(c, g, "exposure", Value::Number(v))
            })
            .unwrap();
            d.end_coalescing();
        }
        d.undo();
        let exposure = d
            .project
            .clip(c)
            .unwrap()
            .node(g)
            .unwrap()
            .number("exposure");
        assert_eq!(exposure, 0.1, "only the second gesture is undone");
    }
}
