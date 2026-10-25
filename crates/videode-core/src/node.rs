//! Node kinds, their parameters, and node instances.
//!
//! A clip is a chain of nodes: a generator (Source or Text) followed by
//! processing nodes, feeding its track. Every node kind declares its
//! parameters so the inspector, the KDL format and validation all come
//! from one table.

use std::collections::BTreeMap;

/// Node categories. Each owns one bar colour in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Source,
    Transform,
    Look,
    Composite,
    Audio,
    Text,
    Output,
}

/// What a clip carries, which decides which nodes may be added to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StreamKind {
    Video,
    Audio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClipKind {
    Video,
    Audio,
    Text,
}

impl ClipKind {
    pub fn stream(self) -> StreamKind {
        match self {
            ClipKind::Audio => StreamKind::Audio,
            ClipKind::Video | ClipKind::Text => StreamKind::Video,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
}

impl Value {
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Text(_) => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            Value::Number(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    Number {
        min: f64,
        max: f64,
        step: f64,
        default: f64,
        /// Digits after the decimal point when displayed.
        decimals: u8,
        /// Show a leading `+` for positive values.
        signed: bool,
    },
    Text {
        default: &'static str,
    },
    Choice {
        options: &'static [&'static str],
        default: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
    pub kind: ParamKind,
}

impl ParamSpec {
    pub fn default_value(&self) -> Value {
        match self.kind {
            ParamKind::Number { default, .. } => Value::Number(default),
            ParamKind::Text { default } => Value::Text(default.into()),
            ParamKind::Choice { default, .. } => Value::Text(default.into()),
        }
    }

    /// Clamps numbers to range and step; rejects unknown choices and
    /// mismatched value types.
    pub fn validate(&self, value: Value) -> Result<Value, ParamError> {
        match (self.kind, value) {
            (ParamKind::Number { min, max, step, .. }, Value::Number(n)) => {
                if !n.is_finite() {
                    return Err(ParamError::Invalid(self.key));
                }
                let snapped = if step > 0.0 {
                    (n / step).round() * step
                } else {
                    n
                };
                // Round away float noise from the step multiply.
                let clean = (snapped.clamp(min, max) * 1e6).round() / 1e6;
                Ok(Value::Number(clean))
            }
            (ParamKind::Text { .. }, Value::Text(s)) => Ok(Value::Text(s)),
            (ParamKind::Choice { options, .. }, Value::Text(s)) => {
                if options.contains(&s.as_str()) {
                    Ok(Value::Text(s))
                } else {
                    Err(ParamError::Invalid(self.key))
                }
            }
            _ => Err(ParamError::WrongType(self.key)),
        }
    }

    /// Parses typed input. Numbers accept `−` or `-`, a leading `+` and a
    /// trailing unit; choices match case-insensitively. The result still goes
    /// through [`Self::validate`] when it is set.
    pub fn parse(&self, text: &str) -> Option<Value> {
        let t = text.trim();
        match self.kind {
            ParamKind::Number { .. } => {
                let t = t.replace('−', "-");
                let t = t
                    .trim_end_matches(|c: char| !(c.is_ascii_digit() || c == '.'))
                    .trim();
                let t = t.strip_prefix('+').unwrap_or(t);
                t.parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .map(Value::Number)
            }
            ParamKind::Text { .. } => Some(Value::Text(text.to_string())),
            ParamKind::Choice { options, .. } => options
                .iter()
                .find(|o| o.eq_ignore_ascii_case(t))
                .map(|o| Value::Text((*o).to_string())),
        }
    }

    /// A number moved by `steps` increments of its step, clamped to range.
    pub fn nudge(&self, value: &Value, steps: f64) -> Option<Value> {
        match (self.kind, value) {
            (ParamKind::Number { step, .. }, Value::Number(n)) => {
                self.validate(Value::Number(n + steps * step)).ok()
            }
            _ => None,
        }
    }

    pub fn is_default(&self, value: &Value) -> bool {
        *value == self.default_value()
    }

    /// Display string with units, e.g. `+0.35 EV`, `108 %`.
    pub fn format(&self, value: &Value) -> String {
        match (self.kind, value) {
            (
                ParamKind::Number {
                    decimals, signed, ..
                },
                Value::Number(n),
            ) => {
                let mut s = format!("{:.*}", decimals as usize, n);
                if signed && *n > 0.0 {
                    s.insert(0, '+');
                }
                let s = s.replace('-', "−");
                if self.unit.is_empty() {
                    s
                } else {
                    format!("{s} {}", self.unit)
                }
            }
            (_, Value::Text(s)) => s.clone(),
            (_, Value::Number(n)) => n.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParamError {
    #[error("{0} has no parameter `{1}`")]
    Unknown(&'static str, String),
    #[error("value for `{0}` has the wrong type")]
    WrongType(&'static str),
    #[error("value for `{0}` is not allowed")]
    Invalid(&'static str),
}

const fn num(
    key: &'static str,
    label: &'static str,
    unit: &'static str,
    (min, max, step, default): (f64, f64, f64, f64),
    decimals: u8,
    signed: bool,
) -> ParamSpec {
    ParamSpec {
        key,
        label,
        unit,
        kind: ParamKind::Number {
            min,
            max,
            step,
            default,
            decimals,
            signed,
        },
    }
}

const BLENDS: &[&str] = &["Normal", "Screen", "Multiply", "Overlay", "Add"];
const WEIGHTS: &[&str] = &["400", "500", "600"];
const DUCK_UNDER: &[&str] = &["A1", "A2", "A3", "A4"];

const TEXT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "text",
        label: "Text",
        unit: "",
        kind: ParamKind::Text { default: "Title" },
    },
    num("size", "Size", "px", (8., 400., 1., 96.), 0, false),
    ParamSpec {
        key: "weight",
        label: "Weight",
        unit: "",
        kind: ParamKind::Choice {
            options: WEIGHTS,
            default: "600",
        },
    },
];
const TRANSFORM_PARAMS: &[ParamSpec] = &[
    num("x", "Position X", "px", (-8192., 8192., 1., 0.), 0, true),
    num("y", "Position Y", "px", (-8192., 8192., 1., 0.), 0, true),
    num("scale", "Scale", "%", (1., 1000., 1., 100.), 0, false),
    num("rotation", "Rotation", "°", (-360., 360., 0.5, 0.), 1, true),
];
const CROP_PARAMS: &[ParamSpec] = &[
    num("left", "Left", "%", (0., 50., 0.5, 0.), 1, false),
    num("right", "Right", "%", (0., 50., 0.5, 0.), 1, false),
    num("top", "Top", "%", (0., 50., 0.5, 0.), 1, false),
    num("bottom", "Bottom", "%", (0., 50., 0.5, 0.), 1, false),
];
const GRADE_PARAMS: &[ParamSpec] = &[
    num("exposure", "Exposure", "EV", (-6., 6., 0.05, 0.), 2, true),
    num(
        "temperature",
        "Temperature",
        "K",
        (2000., 12000., 50., 6500.),
        0,
        false,
    ),
    num(
        "saturation",
        "Saturation",
        "%",
        (0., 200., 1., 100.),
        0,
        false,
    ),
    num("contrast", "Contrast", "%", (0., 200., 1., 100.), 0, false),
];
const BLUR_PARAMS: &[ParamSpec] = &[num("radius", "Radius", "px", (0., 200., 0.5, 4.), 1, false)];
const OPACITY_PARAMS: &[ParamSpec] = &[
    num("opacity", "Opacity", "%", (0., 100., 1., 100.), 0, false),
    ParamSpec {
        key: "blend",
        label: "Blend",
        unit: "",
        kind: ParamKind::Choice {
            options: BLENDS,
            default: "Normal",
        },
    },
];
const FADE_PARAMS: &[ParamSpec] = &[
    num("in", "Fade in", "f", (0., 2400., 1., 12.), 0, false),
    num("out", "Fade out", "f", (0., 2400., 1., 12.), 0, false),
];
const GAIN_PARAMS: &[ParamSpec] = &[num("gain", "Gain", "dB", (-96., 24., 0.5, 0.), 1, true)];
const DUCK_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "under",
        label: "Under track",
        unit: "",
        kind: ParamKind::Choice {
            options: DUCK_UNDER,
            default: "A1",
        },
    },
    num("amount", "Amount", "dB", (-60., 0., 0.5, -12.), 1, true),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Source,
    Text,
    Transform,
    Crop,
    Grade,
    Blur,
    Opacity,
    Fade,
    Gain,
    Duck,
}

impl NodeKind {
    pub const ALL: [NodeKind; 10] = [
        NodeKind::Source,
        NodeKind::Text,
        NodeKind::Transform,
        NodeKind::Crop,
        NodeKind::Grade,
        NodeKind::Blur,
        NodeKind::Opacity,
        NodeKind::Fade,
        NodeKind::Gain,
        NodeKind::Duck,
    ];

    pub fn name(self) -> &'static str {
        match self {
            NodeKind::Source => "Source",
            NodeKind::Text => "Text",
            NodeKind::Transform => "Transform",
            NodeKind::Crop => "Crop",
            NodeKind::Grade => "Grade",
            NodeKind::Blur => "Blur",
            NodeKind::Opacity => "Opacity",
            NodeKind::Fade => "Fade",
            NodeKind::Gain => "Gain",
            NodeKind::Duck => "Duck",
        }
    }

    /// The lowercase identifier used in project files.
    pub fn ident(self) -> &'static str {
        match self {
            NodeKind::Source => "source",
            NodeKind::Text => "text",
            NodeKind::Transform => "transform",
            NodeKind::Crop => "crop",
            NodeKind::Grade => "grade",
            NodeKind::Blur => "blur",
            NodeKind::Opacity => "opacity",
            NodeKind::Fade => "fade",
            NodeKind::Gain => "gain",
            NodeKind::Duck => "duck",
        }
    }

    pub fn from_ident(ident: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.ident() == ident)
    }

    pub fn category(self) -> Category {
        match self {
            NodeKind::Source => Category::Source,
            NodeKind::Text => Category::Text,
            NodeKind::Transform | NodeKind::Crop => Category::Transform,
            NodeKind::Grade | NodeKind::Blur => Category::Look,
            NodeKind::Opacity | NodeKind::Fade => Category::Composite,
            NodeKind::Gain | NodeKind::Duck => Category::Audio,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            NodeKind::Source => "Reads frames or samples from a file",
            NodeKind::Text => "Sets a line of type",
            NodeKind::Transform => "Position, scale, rotation",
            NodeKind::Crop => "Trim the frame edges",
            NodeKind::Grade => "Exposure, white balance, saturation",
            NodeKind::Blur => "Gaussian blur",
            NodeKind::Opacity => "Opacity and blend mode",
            NodeKind::Fade => "Fade in and out over frames",
            NodeKind::Gain => "Level in decibels",
            NodeKind::Duck => "Lower this clip under another track",
        }
    }

    pub fn params(self) -> &'static [ParamSpec] {
        match self {
            NodeKind::Source => &[],
            NodeKind::Text => TEXT_PARAMS,
            NodeKind::Transform => TRANSFORM_PARAMS,
            NodeKind::Crop => CROP_PARAMS,
            NodeKind::Grade => GRADE_PARAMS,
            NodeKind::Blur => BLUR_PARAMS,
            NodeKind::Opacity => OPACITY_PARAMS,
            NodeKind::Fade => FADE_PARAMS,
            NodeKind::Gain => GAIN_PARAMS,
            NodeKind::Duck => DUCK_PARAMS,
        }
    }

    pub fn param(self, key: &str) -> Option<&'static ParamSpec> {
        self.params().iter().find(|p| p.key == key)
    }

    /// Generators start a clip's chain; they have no input and are what the
    /// clip *is*, so they can't be removed, moved or bypassed.
    pub fn is_generator(self) -> bool {
        matches!(self, NodeKind::Source | NodeKind::Text)
    }

    /// Whether this processing node can be added to a clip of `kind`.
    pub fn applies_to(self, kind: ClipKind) -> bool {
        match self {
            NodeKind::Source | NodeKind::Text => false,
            NodeKind::Transform | NodeKind::Blur | NodeKind::Opacity => {
                matches!(kind, ClipKind::Video | ClipKind::Text)
            }
            NodeKind::Crop | NodeKind::Grade => kind == ClipKind::Video,
            NodeKind::Fade => true,
            NodeKind::Gain | NodeKind::Duck => kind == ClipKind::Audio,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u64);

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub params: BTreeMap<&'static str, Value>,
    pub bypass: bool,
}

impl Node {
    pub fn new(id: NodeId, kind: NodeKind) -> Self {
        let params = kind
            .params()
            .iter()
            .map(|p| (p.key, p.default_value()))
            .collect();
        Self {
            id,
            kind,
            params,
            bypass: false,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.params.get(key)
    }

    pub fn number(&self, key: &str) -> f64 {
        self.get(key).and_then(Value::as_number).unwrap_or(0.0)
    }

    pub fn text(&self, key: &str) -> &str {
        self.get(key).and_then(Value::as_text).unwrap_or("")
    }

    pub fn set(&mut self, key: &str, value: Value) -> Result<(), ParamError> {
        let spec = self
            .kind
            .param(key)
            .ok_or_else(|| ParamError::Unknown(self.kind.name(), key.to_string()))?;
        let value = spec.validate(value)?;
        self.params.insert(spec.key, value);
        Ok(())
    }

    /// One-line summary for node cards, e.g. `108% · 0, −42`.
    pub fn summary(&self) -> String {
        let fmt = |key: &str| {
            let spec = self.kind.param(key).expect("summary uses known params");
            spec.format(self.get(key).expect("params are always populated"))
        };
        match self.kind {
            NodeKind::Source => String::new(),
            NodeKind::Text => format!("“{}”", self.text("text")),
            NodeKind::Transform => format!(
                "{}% · {}, {}",
                self.number("scale"),
                fmt("x").trim_end_matches(" px"),
                fmt("y").trim_end_matches(" px")
            ),
            NodeKind::Crop => format!(
                "T{} B{} L{} R{}",
                self.number("top"),
                self.number("bottom"),
                self.number("left"),
                self.number("right")
            ),
            NodeKind::Grade => format!("{} · {}", fmt("exposure"), fmt("temperature")),
            NodeKind::Blur => fmt("radius"),
            NodeKind::Opacity => format!("{} · {}", fmt("opacity"), self.text("blend")),
            NodeKind::Fade => format!("in {}f · out {}f", self.number("in"), self.number("out")),
            NodeKind::Gain => fmt("gain"),
            NodeKind::Duck => format!("under {} · {}", self.text("under"), fmt("amount")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_node_has_defaults() {
        let n = Node::new(NodeId(1), NodeKind::Grade);
        assert_eq!(n.number("temperature"), 6500.0);
        assert_eq!(n.params.len(), 4);
    }

    #[test]
    fn set_clamps_and_snaps() {
        let mut n = Node::new(NodeId(1), NodeKind::Grade);
        n.set("exposure", Value::Number(0.337)).unwrap();
        assert_eq!(n.number("exposure"), 0.35);
        n.set("exposure", Value::Number(99.0)).unwrap();
        assert_eq!(n.number("exposure"), 6.0);
        assert!(matches!(
            n.set("nope", Value::Number(1.0)),
            Err(ParamError::Unknown(..))
        ));
        assert!(matches!(
            n.set("exposure", Value::Text("x".into())),
            Err(ParamError::WrongType(_))
        ));
        assert!(n.set("exposure", Value::Number(f64::NAN)).is_err());
    }

    #[test]
    fn choices_are_validated() {
        let mut n = Node::new(NodeId(1), NodeKind::Opacity);
        n.set("blend", Value::Text("Screen".into())).unwrap();
        assert!(n.set("blend", Value::Text("Dissolve".into())).is_err());
    }

    #[test]
    fn formats_values() {
        let spec = NodeKind::Grade.param("exposure").unwrap();
        assert_eq!(spec.format(&Value::Number(0.35)), "+0.35 EV");
        assert_eq!(spec.format(&Value::Number(-1.0)), "−1.00 EV");
        let mut t = Node::new(NodeId(2), NodeKind::Transform);
        t.set("scale", Value::Number(108.0)).unwrap();
        t.set("y", Value::Number(-42.0)).unwrap();
        assert_eq!(t.summary(), "108% · 0, −42");
    }

    #[test]
    fn node_compatibility() {
        assert!(NodeKind::Grade.applies_to(ClipKind::Video));
        assert!(!NodeKind::Grade.applies_to(ClipKind::Text));
        assert!(NodeKind::Gain.applies_to(ClipKind::Audio));
        assert!(!NodeKind::Gain.applies_to(ClipKind::Video));
        assert!(NodeKind::Fade.applies_to(ClipKind::Audio));
        assert!(!NodeKind::Source.applies_to(ClipKind::Video));
    }

    #[test]
    fn idents_round_trip() {
        for k in NodeKind::ALL {
            assert_eq!(NodeKind::from_ident(k.ident()), Some(k));
        }
    }

    #[test]
    fn parses_typed_values() {
        let exposure = NodeKind::Grade.param("exposure").unwrap();
        assert_eq!(exposure.parse("+0.35 EV"), Some(Value::Number(0.35)));
        assert_eq!(exposure.parse("−1.5"), Some(Value::Number(-1.5)));
        assert_eq!(exposure.parse(" 2ev "), Some(Value::Number(2.0)));
        assert_eq!(exposure.parse("bright"), None);
        let scale = NodeKind::Transform.param("scale").unwrap();
        assert_eq!(scale.parse("108 %"), Some(Value::Number(108.0)));
        let blend = NodeKind::Opacity.param("blend").unwrap();
        assert_eq!(blend.parse("screen"), Some(Value::Text("Screen".into())));
        assert_eq!(blend.parse("glow"), None);
    }

    #[test]
    fn nudges_by_step_within_range() {
        let exposure = NodeKind::Grade.param("exposure").unwrap();
        let v = exposure.nudge(&Value::Number(0.0), 3.0).unwrap();
        assert!(!exposure.is_default(&v));
        let huge = exposure.nudge(&Value::Number(0.0), 1e9).unwrap();
        let ParamKind::Number { max, .. } = exposure.kind else {
            unreachable!()
        };
        assert_eq!(huge, Value::Number(max));
    }
}
