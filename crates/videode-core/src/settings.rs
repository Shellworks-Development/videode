//! User settings, stored as KDL in the platform config folder.
//!
//! ```kdl
//! videode-settings version=1
//! appearance {
//!     animations system
//!     splash #true
//! }
//! editing {
//!     snap #true
//!     zoom-step 1.4
//! }
//! ```
//!
//! Loading is forgiving: a missing or invalid value falls back to its default
//! and is reported, so a hand-edited file never stops the app from starting.

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};

pub const SETTINGS_VERSION: i64 = 1;
pub const ZOOM_STEPS: [f64; 3] = [1.25, 1.4, 2.0];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Animations {
    /// Follow the operating system's reduce-motion preference.
    #[default]
    System,
    On,
    Off,
}

impl Animations {
    pub const ALL: [Animations; 3] = [Animations::System, Animations::On, Animations::Off];

    pub fn ident(self) -> &'static str {
        match self {
            Animations::System => "system",
            Animations::On => "on",
            Animations::Off => "off",
        }
    }

    pub fn from_ident(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.ident() == s)
    }

    /// Whether motion should play, given the system preference.
    pub fn enabled(self, system_reduces_motion: bool) -> bool {
        match self {
            Animations::System => !system_reduces_motion,
            Animations::On => true,
            Animations::Off => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub animations: Animations,
    /// Play the launch splash (only when animations are enabled).
    pub splash: bool,
    /// Snapping state for new windows.
    pub snap: bool,
    /// Zoom factor for one zoom-in or zoom-out step.
    pub zoom_step: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            animations: Animations::System,
            splash: true,
            snap: true,
            zoom_step: 1.4,
        }
    }
}

fn arg(name: &str, value: impl Into<KdlValue>) -> KdlNode {
    let mut n = KdlNode::new(name);
    n.push(KdlEntry::new(value.into()));
    n
}

fn group(name: &str, nodes: Vec<KdlNode>) -> KdlNode {
    let mut g = KdlNode::new(name);
    g.ensure_children().nodes_mut().extend(nodes);
    g
}

impl Settings {
    pub fn to_kdl(&self) -> String {
        let mut doc = KdlDocument::new();
        let mut header = KdlNode::new("videode-settings");
        header.push(KdlEntry::new_prop("version", SETTINGS_VERSION as i128));
        doc.nodes_mut().push(header);
        doc.nodes_mut().push(group(
            "appearance",
            vec![
                arg("animations", self.animations.ident()),
                arg("splash", self.splash),
            ],
        ));
        doc.nodes_mut().push(group(
            "editing",
            vec![arg("snap", self.snap), arg("zoom-step", self.zoom_step)],
        ));
        doc.autoformat();
        doc.to_string()
    }

    /// Parses settings, returning them along with a note for every value
    /// that was ignored. Only a file that isn't KDL at all is an error.
    pub fn from_kdl(text: &str) -> Result<(Settings, Vec<String>), String> {
        let doc: KdlDocument = text.parse().map_err(|e: kdl::KdlError| e.to_string())?;
        let mut s = Settings::default();
        let mut notes = Vec::new();
        for g in doc.nodes() {
            let section = g.name().value();
            if section == "videode-settings" {
                continue;
            }
            let Some(children) = g.children() else {
                notes.push(format!("ignored `{section}`"));
                continue;
            };
            for n in children.nodes() {
                let key = n.name().value();
                let value = n
                    .entries()
                    .first()
                    .filter(|e| e.name().is_none())
                    .map(KdlEntry::value);
                let ok = match (section, key, value) {
                    ("appearance", "animations", Some(KdlValue::String(v))) => {
                        Animations::from_ident(v)
                            .map(|a| s.animations = a)
                            .is_some()
                    }
                    ("appearance", "splash", Some(KdlValue::Bool(b))) => {
                        s.splash = *b;
                        true
                    }
                    ("editing", "snap", Some(KdlValue::Bool(b))) => {
                        s.snap = *b;
                        true
                    }
                    ("editing", "zoom-step", Some(v)) => {
                        match v.as_float().or(v.as_integer().map(|i| i as f64)) {
                            Some(f) if (1.05..=4.0).contains(&f) => {
                                s.zoom_step = f;
                                true
                            }
                            _ => false,
                        }
                    }
                    _ => false,
                };
                if !ok {
                    notes.push(format!("ignored `{section} {key}`"));
                }
            }
        }
        Ok((s, notes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let s = Settings {
            animations: Animations::Off,
            splash: false,
            snap: false,
            zoom_step: 2.0,
        };
        let text = s.to_kdl();
        assert!(text.contains("animations off"), "{text}");
        assert_eq!(Settings::from_kdl(&text).unwrap(), (s, vec![]));
    }

    #[test]
    fn defaults_round_trip() {
        let text = Settings::default().to_kdl();
        assert_eq!(Settings::from_kdl(&text).unwrap().0, Settings::default());
    }

    #[test]
    fn bad_values_fall_back_with_notes() {
        let text = "appearance {\n animations sometimes\n splash #false\n}\nediting {\n zoom-step 99\n snap 3\n colour red\n}";
        let (s, notes) = Settings::from_kdl(text).unwrap();
        assert_eq!(s.animations, Animations::System);
        assert!(!s.splash);
        assert_eq!(s.zoom_step, 1.4);
        assert!(s.snap);
        assert_eq!(notes.len(), 4, "{notes:?}");
    }

    #[test]
    fn not_kdl_is_an_error() {
        assert!(Settings::from_kdl("appearance {").is_err());
    }

    #[test]
    fn system_mode_follows_os() {
        assert!(Animations::System.enabled(false));
        assert!(!Animations::System.enabled(true));
        assert!(Animations::On.enabled(true));
        assert!(!Animations::Off.enabled(false));
    }
}
