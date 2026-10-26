//! Text clips drawn the way the viewer shows them: Geist, white, one line
//! per row, centred on the clip's position.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};

static REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Geist-Regular.ttf");
static MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Geist-Medium.ttf");
static SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Geist-SemiBold.ttf");

/// The viewer's line height, as a multiple of the text size.
const LINE_HEIGHT: f32 = 1.1;

pub(crate) struct TextStyle {
    /// Text size (em) in output pixels.
    pub size: f32,
    /// CSS-style weight; the nearest of 400, 500 and 600 is used.
    pub weight: f32,
    /// Centre of the text block, in output pixels.
    pub centre: (f32, f32),
    pub opacity: f32,
}

fn font(weight: f32) -> FontRef<'static> {
    let bytes = if weight < 450. {
        REGULAR
    } else if weight < 550. {
        MEDIUM
    } else {
        SEMIBOLD
    };
    FontRef::try_from_slice(bytes).expect("bundled font parses")
}

/// Draws white `text` onto an opaque BGRA canvas, inside `clip`
/// (x0, y0, x1, y1).
pub(crate) fn draw(
    canvas: &mut [u8],
    width: u32,
    clip: (i32, i32, i32, i32),
    text: &str,
    style: &TextStyle,
) {
    if style.size < 0.5 || style.opacity <= 0. || text.is_empty() {
        return;
    }
    let font = font(style.weight);
    let em = font.units_per_em().unwrap_or(1000.);
    let scale = PxScale::from(style.size * font.height_unscaled() / em);
    let sf = font.as_scaled(scale);
    let line_h = style.size * LINE_HEIGHT;
    let lines: Vec<&str> = text.lines().collect();
    let block_top = style.centre.1 - line_h * lines.len() as f32 / 2.;
    // Same as CSS: the glyph box sits in the middle of the line box.
    let baseline_in_line = (line_h - (sf.ascent() - sf.descent())) / 2. + sf.ascent();

    for (row, line) in lines.iter().enumerate() {
        let mut x = 0f32;
        let mut prev = None;
        let mut glyphs = Vec::new();
        for ch in line.chars() {
            let id = font.glyph_id(ch);
            if let Some(p) = prev {
                x += sf.kern(p, id);
            }
            glyphs.push((id, x));
            x += sf.h_advance(id);
            prev = Some(id);
        }
        let left = style.centre.0 - x / 2.;
        let baseline = block_top + row as f32 * line_h + baseline_in_line;
        for (id, gx) in glyphs {
            let g = id.with_scale_and_position(scale, point(left + gx, baseline));
            let Some(outline) = font.outline_glyph(g) else {
                continue;
            };
            let b = outline.px_bounds();
            outline.draw(|px, py, coverage| {
                let x = b.min.x as i32 + px as i32;
                let y = b.min.y as i32 + py as i32;
                if x < clip.0 || y < clip.1 || x >= clip.2 || y >= clip.3 {
                    return;
                }
                let a = coverage.clamp(0., 1.) * style.opacity;
                let o = ((y as u32 * width + x as u32) * 4) as usize;
                for c in &mut canvas[o..o + 3] {
                    *c = (*c as f32 + (255. - *c as f32) * a).round() as u8;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_centred_and_white() {
        let (w, h) = (200u32, 100u32);
        let mut canvas = [0, 0, 0, 255].repeat((w * h) as usize);
        let style = TextStyle {
            size: 40.,
            weight: 500.,
            centre: (100., 50.),
            opacity: 1.,
        };
        draw(&mut canvas, w, (0, 0, w as i32, h as i32), "HH", &style);
        let lit: Vec<(u32, u32)> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|(x, y)| canvas[((y * w + x) * 4) as usize] > 128)
            .collect();
        assert!(!lit.is_empty());
        let mx = lit.iter().map(|p| p.0 as f32).sum::<f32>() / lit.len() as f32;
        let my = lit.iter().map(|p| p.1 as f32).sum::<f32>() / lit.len() as f32;
        assert!((mx - 100.).abs() < 3., "centred across: {mx}");
        assert!((my - 50.).abs() < 6., "centred down: {my}");
        // White on black: every channel lights up together.
        let o = ((lit[lit.len() / 2].1 * w + lit[lit.len() / 2].0) * 4) as usize;
        assert_eq!(canvas[o], canvas[o + 2]);
    }
}
