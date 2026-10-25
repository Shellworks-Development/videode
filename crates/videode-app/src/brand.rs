//! The Merge mark, painted natively so it is crisp at any size and can be
//! animated: two ports (picture, sound) flow down bone wires into one Output
//! node. Geometry matches `assets/brand/videode.svg` on its 1024 grid.

use gpui_kit::prelude::*;
use gpui_kit::{
    BorderStyle, Bounds, Hsla, PathBuilder, Pixels, Point, Window, canvas, point, px, quad, size,
};

use crate::motion::{ease_out, lerp};
use crate::theme::{Category, CategoryExt, c, hex};

/// The glyph's box on the 1024 grid (the mark without its tile).
const GLYPH_X: f32 = 192.;
const GLYPH_Y: f32 = 184.;
const GLYPH_W: f32 = 640.;

const WIRE: f32 = 64.;
const PORT_R: f32 = 74.;
const PORT_RING: f32 = 44.;

/// Splash timing, in seconds from launch.
pub const SPLASH_MARK: f32 = 0.7;
pub const SPLASH_WORD: (f32, f32) = (0.45, 0.8);
pub const SPLASH_FADE: (f32, f32) = (1.05, 1.3);

fn phase(t: f32, start: f32, end: f32) -> f32 {
    ease_out(((t - start) / (end - start)).clamp(0., 1.))
}

type P = (f32, f32);

fn mix(a: P, b: P, t: f32) -> P {
    (lerp(a.0, b.0, t), lerp(a.1, b.1, t))
}

/// The part of a cubic Bézier from 0 to `t`, as a new cubic (de Casteljau).
fn cubic_head(p0: P, p1: P, p2: P, p3: P, t: f32) -> (P, P, P, P) {
    let (a, b, c) = (mix(p0, p1, t), mix(p1, p2, t), mix(p2, p3, t));
    let (d, e) = (mix(a, b, t), mix(b, c, t));
    (p0, a, d, mix(d, e, t))
}

/// The mark as an element `size` px square. `progress` runs 0→1 through the
/// build: ports pop in, wires draw down, the Output node drops into place.
pub fn mark(size_px: f32, progress: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| paint_mark(window, bounds, progress),
    )
    .w(px(size_px))
    .h(px(size_px))
    .flex_none()
}

fn paint_mark(window: &mut Window, bounds: Bounds<Pixels>, t: f32) {
    let k = f32::from(bounds.size.width) / GLYPH_W;
    let at = |x: f32, y: f32| -> Point<Pixels> {
        point(
            bounds.origin.x + px((x - GLYPH_X) * k),
            bounds.origin.y + px((y - GLYPH_Y) * k),
        )
    };
    let bone = c(hex::BONE);
    let circle =
        |window: &mut Window, cx: f32, cy: f32, r: f32, fill: Hsla, ring: f32, ring_color: Hsla| {
            let o = at(cx - r, cy - r);
            window.paint_quad(quad(
                Bounds::new(o, size(px(2. * r * k), px(2. * r * k))),
                px(r * k),
                fill,
                px(ring * k),
                ring_color,
                BorderStyle::Solid,
            ));
        };

    let ports = phase(t, 0., 0.35);
    let wires = phase(t, 0.2, 0.7);
    let node = phase(t, 0.55, 1.);

    // Wires, drawn as polylines up to `wires` along each curve, with round caps.
    if wires > 0. {
        for side in [300., 724.] {
            let curve = ((side, 300.), (side, 520.), (512., 560.), (512., 724.));
            let (p0, c0, c1, end) = cubic_head(curve.0, curve.1, curve.2, curve.3, wires);
            let mut path = PathBuilder::stroke(px(WIRE * k));
            path.move_to(at(p0.0, p0.1));
            path.cubic_bezier_to(at(end.0, end.1), at(c0.0, c0.1), at(c1.0, c1.1));
            if let Ok(path) = path.build() {
                window.paint_path(path, bone);
            }
            circle(window, end.0, end.1, WIRE / 2., bone, 0., bone);
        }
    }

    // Ports scale up from their centres.
    if ports > 0. {
        let s = ports;
        let outer = (PORT_R + PORT_RING / 2.) * s;
        for (x, cat) in [(300., Category::Look), (724., Category::Audio)] {
            circle(
                window,
                x,
                300.,
                outer,
                c(hex::PLUGE_BLACK),
                PORT_RING * s,
                cat.color(),
            );
        }
    }

    // The Output node drops 40 units into place as it fades in.
    if node > 0. {
        let dy = lerp(-40., 0., node);
        let o = at(402., 648. + dy);
        let mut blue = Category::Output.color();
        blue.a = node;
        window.paint_quad(quad(
            Bounds::new(o, size(px(220. * k), px(152. * k))),
            px(34. * k),
            blue,
            px(0.),
            blue,
            BorderStyle::Solid,
        ));
        let mut sheen: Hsla = c(0x6f84e6);
        sheen.a = 0.55 * node;
        window.paint_quad(quad(
            Bounds::new(o, size(px(220. * k), px(26. * k))),
            px(13. * k),
            sheen,
            px(0.),
            sheen,
            BorderStyle::Solid,
        ));
    }
}
