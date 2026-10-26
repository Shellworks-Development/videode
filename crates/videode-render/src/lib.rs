//! Pixel effects on decoded pictures: Grade, Blur, and rotation with crop.
//! This is the CPU path the viewer uses until the GPU effects pass; the
//! results define what the GPU version must match.

use videode_core::look::Grade;
use videode_media::Picture;

/// What to do to a picture's pixels, in the picture's own pixel units.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fx {
    pub grades: Vec<Grade>,
    /// Blur radius in picture pixels.
    pub blur: f64,
    /// Degrees clockwise. When non-zero, `crop` is applied here too, since
    /// cropping must happen before the corners turn.
    pub rotation: f64,
    /// Fractions cut from left, right, top, bottom (with rotation only).
    pub crop: [f64; 4],
}

impl Fx {
    pub fn is_identity(&self) -> bool {
        self.grades.is_empty() && self.blur < 0.5 && self.rotation == 0.
    }
}

/// Applies `fx` to `p`. With rotation the result is the bounding box of the
/// turned picture, transparent outside it.
pub fn apply(p: &Picture, fx: &Fx) -> Picture {
    let mut out = p.clone();
    for g in &fx.grades {
        grade(&mut out.bgra, g);
    }
    if fx.blur >= 0.5 {
        blur(&mut out, fx.blur);
    }
    if fx.rotation != 0. {
        out = rotate(&out, fx.rotation, fx.crop);
    }
    out
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}

/// Per-channel gains for a white balance in kelvin: warmer adds red and
/// takes away blue.
fn temperature_gains(kelvin: f64) -> [f32; 3] {
    let w = if kelvin >= 6500. {
        (kelvin - 6500.) / 5500.
    } else {
        (kelvin - 6500.) / 4500.
    }
    .clamp(-1., 1.) as f32;
    [1. + 0.25 * w, 1. + 0.03 * w, 1. - 0.25 * w]
}

/// Grades BGRA pixels in place: exposure and white balance in linear light,
/// then contrast around mid-grey and saturation around luma, in display
/// values.
pub fn grade(bgra: &mut [u8], g: &Grade) {
    let gains = temperature_gains(g.temperature);
    let stops = 2f32.powf(g.exposure as f32);
    let contrast = (g.contrast / 100.) as f32;
    let saturation = (g.saturation / 100.) as f32;
    // Exposure, balance and contrast work per channel, so they fold into one
    // table per channel (RGB order).
    let tables: [[f32; 256]; 3] = std::array::from_fn(|ch| {
        std::array::from_fn(|i| {
            let lin = srgb_to_linear(i as f32 / 255.) * stops * gains[ch];
            let v = linear_to_srgb(lin.max(0.));
            (v - 0.5) * contrast + 0.5
        })
    });
    for px in bgra.as_chunks_mut::<4>().0 {
        let r = tables[0][px[2] as usize];
        let gr = tables[1][px[1] as usize];
        let b = tables[2][px[0] as usize];
        let y = 0.2126 * r + 0.7152 * gr + 0.0722 * b;
        let s = |c: f32| ((y + (c - y) * saturation).clamp(0., 1.) * 255. + 0.5) as u8;
        px[2] = s(r);
        px[1] = s(gr);
        px[0] = s(b);
    }
}

/// An approximate Gaussian blur with standard deviation near `radius`:
/// three box passes each way.
pub fn blur(p: &mut Picture, radius: f64) {
    let (w, h) = (p.width as usize, p.height as usize);
    let r = ((radius * 0.8).round() as usize).max(1);
    let mut tmp = vec![0u8; p.bgra.len()];
    for _ in 0..3 {
        box_pass(&p.bgra, &mut tmp, w, h, r, true);
        box_pass(&tmp, &mut p.bgra, w, h, r, false);
    }
}

/// One box blur along rows (`horizontal`) or columns, edges clamped.
fn box_pass(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize, horizontal: bool) {
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let at = |line: usize, i: usize| {
        if horizontal {
            (line * w + i) * 4
        } else {
            (i * w + line) * 4
        }
    };
    let n = (2 * r + 1) as u32;
    for line in 0..lines {
        // The window -r..=r around index 0, edges clamped.
        let mut sum = [0u32; 4];
        for k in 0..=2 * r {
            let i = (k as isize - r as isize).clamp(0, len as isize - 1) as usize;
            let o = at(line, i);
            for c in 0..4 {
                sum[c] += src[o + c] as u32;
            }
        }
        for i in 0..len {
            let o = at(line, i);
            for c in 0..4 {
                dst[o + c] = ((sum[c] + n / 2) / n) as u8;
            }
            let out = at(line, i.saturating_sub(r));
            let inn = at(line, (i + r + 1).min(len - 1));
            for c in 0..4 {
                sum[c] = sum[c] + src[inn + c] as u32 - src[out + c] as u32;
            }
        }
    }
}

/// Crops then turns the picture `degrees` clockwise about its centre, into
/// the bounding box of the result. Samples bilinearly; outside is clear.
pub fn rotate(p: &Picture, degrees: f64, crop: [f64; 4]) -> Picture {
    let (sw, sh) = (p.width as f64, p.height as f64);
    let [cl, cr, ct, cb] = crop;
    let (x0, x1) = (sw * cl, sw * (1. - cr));
    let (y0, y1) = (sh * ct, sh * (1. - cb));
    let (cw, ch) = ((x1 - x0).max(1.), (y1 - y0).max(1.));
    let (sin, cos) = degrees.to_radians().sin_cos();
    // Trim float noise so a quarter turn of 4×2 is exactly 2×4.
    let ow = (cw * cos.abs() + ch * sin.abs() - 1e-6).ceil().max(1.) as u32;
    let oh = (cw * sin.abs() + ch * cos.abs() - 1e-6).ceil().max(1.) as u32;
    let (ocx, ocy) = (ow as f64 / 2., oh as f64 / 2.);
    let (scx, scy) = (x0 + cw / 2., y0 + ch / 2.);
    let mut bgra = vec![0u8; (ow * oh * 4) as usize];
    let src = |x: i64, y: i64| -> [f32; 4] {
        if x < x0.floor() as i64
            || y < y0.floor() as i64
            || x >= x1.ceil() as i64
            || y >= y1.ceil() as i64
        {
            return [0.; 4];
        }
        if x < 0 || y < 0 || x >= p.width as i64 || y >= p.height as i64 {
            return [0.; 4];
        }
        let o = ((y as u32 * p.width + x as u32) * 4) as usize;
        let s = &p.bgra[o..o + 4];
        // Premultiply so transparent edges don't darken.
        let a = s[3] as f32 / 255.;
        [
            s[0] as f32 * a,
            s[1] as f32 * a,
            s[2] as f32 * a,
            s[3] as f32,
        ]
    };
    for oy in 0..oh {
        for ox in 0..ow {
            // Inverse rotation from output to source.
            let dx = ox as f64 + 0.5 - ocx;
            let dy = oy as f64 + 0.5 - ocy;
            let sx = cos * dx + sin * dy + scx - 0.5;
            let sy = -sin * dx + cos * dy + scy - 0.5;
            let (fx, fy) = (sx.floor(), sy.floor());
            let (tx, ty) = ((sx - fx) as f32, (sy - fy) as f32);
            let (ix, iy) = (fx as i64, fy as i64);
            let a = src(ix, iy);
            let b = src(ix + 1, iy);
            let c = src(ix, iy + 1);
            let d = src(ix + 1, iy + 1);
            let o = ((oy * ow + ox) * 4) as usize;
            let alpha =
                (a[3] * (1. - tx) + b[3] * tx) * (1. - ty) + (c[3] * (1. - tx) + d[3] * tx) * ty;
            if alpha <= 0. {
                continue;
            }
            for k in 0..3 {
                let v = (a[k] * (1. - tx) + b[k] * tx) * (1. - ty)
                    + (c[k] * (1. - tx) + d[k] * tx) * ty;
                bgra[o + k] = (v / (alpha / 255.)).round().clamp(0., 255.) as u8;
            }
            bgra[o + 3] = alpha.round().clamp(0., 255.) as u8;
        }
    }
    Picture {
        width: ow,
        height: oh,
        bgra,
        time: p.time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: u32, h: u32, bgra: [u8; 4]) -> Picture {
        Picture {
            width: w,
            height: h,
            bgra: bgra.repeat((w * h) as usize),
            time: 0.,
        }
    }

    fn neutral() -> Grade {
        Grade {
            exposure: 0.,
            temperature: 6500.,
            saturation: 100.,
            contrast: 100.,
        }
    }

    #[test]
    fn neutral_grade_changes_nothing() {
        let mut px: Vec<u8> = (0..=255).flat_map(|v| [v, 255 - v, v / 2, 255]).collect();
        let before = px.clone();
        grade(&mut px, &neutral());
        assert_eq!(px, before);
    }

    #[test]
    fn exposure_doubles_linear_light() {
        let mut px = vec![0x80, 0x80, 0x80, 255];
        grade(
            &mut px,
            &Grade {
                exposure: 1.,
                ..neutral()
            },
        );
        // sRGB 128 is 0.216 linear; doubled is 0.432, which is sRGB ~176.
        assert!((px[0] as i32 - 176).abs() <= 1, "{px:?}");
        assert_eq!(px[3], 255, "alpha untouched");
    }

    #[test]
    fn zero_saturation_is_grey_and_warmth_tilts_red() {
        let mut px = vec![200, 40, 10, 255];
        grade(
            &mut px,
            &Grade {
                saturation: 0.,
                ..neutral()
            },
        );
        assert!(px[0] == px[1] && px[1] == px[2], "{px:?}");

        let mut px = vec![128, 128, 128, 255];
        grade(
            &mut px,
            &Grade {
                temperature: 9000.,
                ..neutral()
            },
        );
        assert!(px[2] > px[0], "warmer means redder than blue: {px:?}");
    }

    #[test]
    fn blur_keeps_flat_areas_and_softens_edges() {
        let mut p = flat(32, 8, [100, 100, 100, 255]);
        blur(&mut p, 3.);
        assert!(p.bgra.iter().step_by(4).all(|&v| v == 100));

        let mut edge = flat(32, 1, [0, 0, 0, 255]);
        for x in 16..32 {
            edge.bgra[x * 4..x * 4 + 3].fill(255);
        }
        blur(&mut edge, 3.);
        let at = |x: usize| edge.bgra[x * 4];
        assert!(
            at(0) == 0 && at(31) == 255,
            "far from the edge is unchanged"
        );
        assert!(at(15) > 0 && at(16) < 255, "the edge is softened");
    }

    #[test]
    fn quarter_turn_swaps_sides_and_keeps_pixels() {
        let mut p = flat(4, 2, [0, 0, 0, 255]);
        // Mark the top-left pixel.
        p.bgra[..3].fill(255);
        let r = rotate(&p, 90., [0.; 4]);
        assert_eq!((r.width, r.height), (2, 4));
        // Clockwise, top-left ends at top-right.
        let px = |x: u32, y: u32| r.bgra[((y * r.width + x) * 4) as usize];
        assert_eq!(px(1, 0), 255);
        assert_eq!(px(0, 0), 0);
        assert!(
            r.bgra.chunks(4).all(|c| c[3] == 255),
            "a quarter turn has no empty corners"
        );
    }

    #[test]
    fn rotation_crops_first_and_clears_corners() {
        let p = flat(100, 100, [50, 60, 70, 255]);
        let r = rotate(&p, 45., [0.25, 0.25, 0., 0.]);
        // A 50×100 strip turned 45° has a ~106 px square bounding box.
        assert!((r.width as i32 - 107).abs() <= 1 && (r.height as i32 - 107).abs() <= 1);
        assert_eq!(r.bgra[3], 0, "corner is transparent");
        let mid = ((r.height / 2 * r.width + r.width / 2) * 4) as usize;
        assert_eq!(&r.bgra[mid..mid + 4], &[50, 60, 70, 255]);
    }

    #[test]
    fn identity_fx_is_detected() {
        assert!(Fx::default().is_identity());
        assert!(
            !Fx {
                rotation: 1.,
                ..Fx::default()
            }
            .is_identity()
        );
    }
}
