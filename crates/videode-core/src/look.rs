//! What a clip looks like at one frame: where it sits (Transform, Crop),
//! how see-through it is (Opacity, Fade), and what is done to its pixels
//! (Grade, Blur, rotation).

use crate::node::NodeKind;
use crate::project::Clip;
use crate::time::Frame;

/// One Grade node's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grade {
    /// Stops, applied in linear light.
    pub exposure: f64,
    /// White balance in kelvin; 6500 is neutral, higher is warmer.
    pub temperature: f64,
    /// Percent; 100 is unchanged.
    pub saturation: f64,
    /// Percent; 100 is unchanged.
    pub contrast: f64,
}

impl Grade {
    pub fn is_neutral(&self) -> bool {
        self.exposure == 0.
            && self.temperature == 6500.
            && self.saturation == 100.
            && self.contrast == 100.
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// 0–1, after Opacity and Fade.
    pub opacity: f64,
    /// 1.0 is the clip's natural size in the frame.
    pub scale: f64,
    /// Offset of the clip's centre from the frame's centre, in sequence pixels.
    pub x: f64,
    pub y: f64,
    /// Fractions (0–0.5) cut from each edge: left, right, top, bottom.
    pub crop: [f64; 4],
    /// Degrees clockwise.
    pub rotation: f64,
    /// Grades in chain order.
    pub grades: Vec<Grade>,
    /// Blur radius in sequence pixels (several Blurs add up).
    pub blur: f64,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            opacity: 1.,
            scale: 1.,
            x: 0.,
            y: 0.,
            crop: [0.; 4],
            rotation: 0.,
            grades: Vec::new(),
            blur: 0.,
        }
    }
}

/// The look of `clip` at sequence frame `at`, combining its enabled nodes in
/// chain order (two Transforms add their offsets and multiply their scales).
pub fn look_at(clip: &Clip, at: Frame) -> Look {
    let mut l = Look::default();
    let local = (at - clip.start).clamp(0, clip.duration);
    for n in clip.nodes.iter().filter(|n| !n.bypass) {
        match n.kind {
            NodeKind::Transform => {
                l.x = l.x * n.number("scale") / 100. + n.number("x");
                l.y = l.y * n.number("scale") / 100. + n.number("y");
                l.scale *= n.number("scale") / 100.;
                l.rotation += n.number("rotation");
            }
            NodeKind::Grade => {
                let g = Grade {
                    exposure: n.number("exposure"),
                    temperature: n.number("temperature"),
                    saturation: n.number("saturation"),
                    contrast: n.number("contrast"),
                };
                if !g.is_neutral() {
                    l.grades.push(g);
                }
            }
            NodeKind::Blur => l.blur += n.number("radius"),
            NodeKind::Crop => {
                for (i, key) in ["left", "right", "top", "bottom"].into_iter().enumerate() {
                    l.crop[i] = (l.crop[i] + n.number(key) / 100.).min(0.5);
                }
            }
            NodeKind::Opacity => l.opacity *= n.number("opacity") / 100.,
            NodeKind::Fade => {
                let fade_in = n.number("in");
                let fade_out = n.number("out");
                let from_end = (clip.duration - local) as f64;
                if fade_in > 0. {
                    l.opacity *= (local as f64 / fade_in).min(1.);
                }
                if fade_out > 0. {
                    l.opacity *= (from_end / fade_out).min(1.);
                }
            }
            _ => {}
        }
    }
    l.opacity = l.opacity.clamp(0., 1.);
    l
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Value;
    use crate::project::{MediaKind, Project};

    fn clip_with(kinds: &[(NodeKind, &[(&str, f64)])]) -> Clip {
        let mut p = Project::new("t");
        let m = p.add_media("a.mov", MediaKind::Video, Some(100), "", "");
        let c = p.add_media_clip(m, None, 0).unwrap();
        for (kind, params) in kinds {
            let n = p.add_node(c, *kind).unwrap();
            for (k, v) in *params {
                p.set_param(c, n, k, Value::Number(*v)).unwrap();
            }
        }
        p.clip(c).unwrap().clone()
    }

    #[test]
    fn plain_clip_is_untouched() {
        assert_eq!(look_at(&clip_with(&[]), 50), Look::default());
    }

    #[test]
    fn fades_ramp_at_both_ends() {
        let c = clip_with(&[(NodeKind::Fade, &[("in", 10.), ("out", 20.)])]);
        assert_eq!(look_at(&c, 0).opacity, 0.);
        assert_eq!(look_at(&c, 5).opacity, 0.5);
        assert_eq!(look_at(&c, 50).opacity, 1.);
        assert_eq!(look_at(&c, 90).opacity, 0.5);
        assert_eq!(look_at(&c, 100).opacity, 0.);
    }

    #[test]
    fn opacity_transform_and_crop_combine() {
        let c = clip_with(&[
            (NodeKind::Opacity, &[("opacity", 50.)]),
            (NodeKind::Transform, &[("scale", 200.), ("x", 10.)]),
            (NodeKind::Transform, &[("scale", 50.), ("y", -4.)]),
            (NodeKind::Crop, &[("left", 10.), ("bottom", 60.)]),
        ]);
        let l = look_at(&c, 50);
        assert_eq!(l.opacity, 0.5);
        assert_eq!(l.scale, 1.);
        assert_eq!((l.x, l.y), (5., -4.));
        assert_eq!(l.crop, [0.1, 0., 0., 0.5]);
    }

    #[test]
    fn pixel_effects_are_collected_in_order() {
        let c = clip_with(&[
            (NodeKind::Grade, &[("exposure", 1.)]),
            (NodeKind::Grade, &[]),
            (NodeKind::Blur, &[("radius", 3.)]),
            (NodeKind::Transform, &[("rotation", 10.)]),
            (NodeKind::Grade, &[("saturation", 0.)]),
            (NodeKind::Transform, &[("rotation", 5.)]),
        ]);
        let l = look_at(&c, 0);
        let exposures: Vec<_> = l
            .grades
            .iter()
            .map(|g| (g.exposure, g.saturation))
            .collect();
        assert_eq!(
            exposures,
            [(1., 100.), (0., 0.)],
            "neutral grades are skipped"
        );
        assert_eq!(l.blur, 3.);
        assert_eq!(l.rotation, 15.);
    }

    #[test]
    fn bypassed_nodes_do_nothing() {
        let mut c = clip_with(&[(NodeKind::Opacity, &[("opacity", 10.)])]);
        c.nodes[1].bypass = true;
        assert_eq!(look_at(&c, 50).opacity, 1.);
    }
}
