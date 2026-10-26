//! Builds whole output frames: every enabled video track, bottom first,
//! each clip with its look, over black.

use std::collections::HashMap;
use std::path::PathBuf;

use videode_core::look::{Look, look_at};
use videode_core::{Clip, Frame, MediaId, MediaKind, NodeKind, Project, StreamKind};
use videode_media::{MediaError, Picture, VideoDecoder};
use videode_render::Fx;

use crate::text::{self, TextStyle};

pub struct Compositor<'a> {
    project: &'a Project,
    paths: &'a HashMap<MediaId, PathBuf>,
    width: u32,
    height: u32,
    decoders: HashMap<PathBuf, VideoDecoder>,
    /// Stills decode once per size.
    stills: HashMap<(PathBuf, u32, u32), Picture>,
}

/// Where the sequence frame sits in the output: its scale and top-left.
#[derive(Clone, Copy)]
struct Stage {
    k: f64,
    ox: f64,
    oy: f64,
    /// The sequence frame in output pixels (x0, y0, x1, y1); nothing draws
    /// outside it.
    clip: (i32, i32, i32, i32),
}

impl<'a> Compositor<'a> {
    /// `paths` holds every online media file; clips of other media are left
    /// out. The output is `width × height`; a sequence of another shape is
    /// fitted inside with black bars.
    pub fn new(
        project: &'a Project,
        paths: &'a HashMap<MediaId, PathBuf>,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            project,
            paths,
            width,
            height,
            decoders: HashMap::new(),
            stills: HashMap::new(),
        }
    }

    fn stage(&self) -> Stage {
        let seq = &self.project.sequence;
        let (sw, sh) = (seq.width.max(1) as f64, seq.height.max(1) as f64);
        let (w, h) = (self.width as f64, self.height as f64);
        let k = (w / sw).min(h / sh);
        let (ox, oy) = ((w - sw * k) / 2., (h - sh * k) / 2.);
        Stage {
            k,
            ox,
            oy,
            clip: (
                ox.round() as i32,
                oy.round() as i32,
                (ox + sw * k).round() as i32,
                (oy + sh * k).round() as i32,
            ),
        }
    }

    /// The output picture at sequence frame `at`, opaque BGRA.
    pub fn frame(&mut self, at: Frame) -> Result<Picture, MediaError> {
        let p = self.project;
        let fps = p.sequence.fps;
        let mut canvas = [0, 0, 0, 255].repeat((self.width * self.height) as usize);
        let stage = self.stage();
        let tracks = p
            .sequence
            .tracks
            .iter()
            .rev()
            .filter(|t| t.enabled && t.kind == StreamKind::Video);
        for t in tracks {
            let Some(clip) = p.clip_at(t.id, at) else {
                continue;
            };
            let look = look_at(clip, at);
            if look.opacity <= 0. {
                continue;
            }
            if let Some(node) = clip.first(NodeKind::Text) {
                self.draw_text(&mut canvas, &stage, &look, node);
                continue;
            }
            let Some((media, path)) = clip
                .media
                .and_then(|id| Some((p.media(id)?, self.paths.get(&id)?)))
            else {
                continue;
            };
            if media.offline {
                continue;
            }
            let secs = match media.kind {
                MediaKind::Image => 0.,
                _ => fps.frames_to_seconds(clip.source_in + (at - clip.start)),
            };
            let still = media.kind == MediaKind::Image;
            self.draw_picture(&mut canvas, &stage, clip, &look, path.clone(), secs, still)?;
        }
        Ok(Picture {
            width: self.width,
            height: self.height,
            bgra: canvas,
            time: fps.frames_to_seconds(at),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_picture(
        &mut self,
        canvas: &mut [u8],
        stage: &Stage,
        _clip: &Clip,
        l: &Look,
        path: PathBuf,
        secs: f64,
        still: bool,
    ) -> Result<(), MediaError> {
        let seq = &self.project.sequence;
        let (fw, fh) = (seq.width as f64 * stage.k, seq.height as f64 * stage.k);
        // Decode at the size it will show, never more than the file has.
        let max_w = (fw * l.scale).ceil().clamp(2., 16384.) as u32;
        let max_h = (fh * l.scale).ceil().clamp(2., 16384.) as u32;
        let pic = if still {
            let key = (path.clone(), max_w, max_h);
            if !self.stills.contains_key(&key) {
                let pic = VideoDecoder::open(&path)?.frame_at(0., max_w, max_h)?;
                self.stills.insert(key.clone(), pic);
            }
            self.stills[&key].clone()
        } else {
            let decoder = match self.decoders.entry(path.clone()) {
                std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(VideoDecoder::open(&path)?)
                }
            };
            decoder.frame_at(secs, max_w, max_h)?
        };
        // Output pixels per picture pixel: contain in the frame, then scale.
        let (pw, ph) = (pic.width.max(1) as f64, pic.height.max(1) as f64);
        let fit = (fw / pw).min(fh / ph) * l.scale;
        let fx = Fx {
            grades: l.grades.clone(),
            // Blur is set in sequence pixels.
            blur: l.blur * stage.k / fit,
            rotation: l.rotation,
            crop: if l.rotation != 0. { l.crop } else { [0.; 4] },
        };
        let pic = if fx.is_identity() {
            pic
        } else {
            videode_render::apply(&pic, &fx)
        };
        let cx = stage.ox + (seq.width as f64 / 2. + l.x) * stage.k;
        let cy = stage.oy + (seq.height as f64 / 2. + l.y) * stage.k;
        let (dw, dh) = (pic.width as f64 * fit, pic.height as f64 * fit);
        let (left, top) = (cx - dw / 2., cy - dh / 2.);
        // Crop is already in a turned picture.
        let [cl, cr, ct, cb] = if l.rotation != 0. { [0.; 4] } else { l.crop };
        let area = (
            left + dw * cl,
            top + dh * ct,
            left + dw * (1. - cr),
            top + dh * (1. - cb),
        );
        blit(
            canvas,
            self.width,
            stage.clip,
            &pic,
            (left, top, fit),
            area,
            l.opacity as f32,
        );
        Ok(())
    }

    fn draw_text(&self, canvas: &mut [u8], stage: &Stage, l: &Look, node: &videode_core::Node) {
        let seq = &self.project.sequence;
        let style = TextStyle {
            size: (node.number("size") * l.scale * stage.k) as f32,
            weight: node.text("weight").parse().unwrap_or(500.),
            centre: (
                (stage.ox + (seq.width as f64 / 2. + l.x) * stage.k) as f32,
                (stage.oy + (seq.height as f64 / 2. + l.y) * stage.k) as f32,
            ),
            opacity: l.opacity as f32,
        };
        text::draw(canvas, self.width, stage.clip, node.text("text"), &style);
    }
}

/// Draws `pic` scaled by `fit` with its top-left at (`left`, `top`),
/// limited to `area` and `clip`, blending with straight alpha.
fn blit(
    canvas: &mut [u8],
    width: u32,
    clip: (i32, i32, i32, i32),
    pic: &Picture,
    (left, top, fit): (f64, f64, f64),
    area: (f64, f64, f64, f64),
    opacity: f32,
) {
    let x0 = (area.0.round() as i32).max(clip.0);
    let y0 = (area.1.round() as i32).max(clip.1);
    let x1 = (area.2.round() as i32).min(clip.2);
    let y1 = (area.3.round() as i32).min(clip.3);
    if x0 >= x1 || y0 >= y1 || fit <= 0. {
        return;
    }
    let (pw, ph) = (pic.width as i64, pic.height as i64);
    let texel = |x: i64, y: i64| {
        let o = ((y.clamp(0, ph - 1) * pw + x.clamp(0, pw - 1)) * 4) as usize;
        [
            pic.bgra[o],
            pic.bgra[o + 1],
            pic.bgra[o + 2],
            pic.bgra[o + 3],
        ]
    };
    let inv = 1. / fit;
    let stride = width as usize * 4;
    // Rows are independent: split them across the cores.
    let rows = &mut canvas[y0 as usize * stride..y1 as usize * stride];
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let per = (y1 - y0) as usize / threads + 1;
    std::thread::scope(|s| {
        for (n, chunk) in rows.chunks_mut(per * stride).enumerate() {
            let first = y0 + (n * per) as i32;
            s.spawn(move || {
                for (k, line) in chunk.chunks_mut(stride).enumerate() {
                    blit_row(
                        line,
                        first + k as i32,
                        x0,
                        x1,
                        &texel,
                        (left, top, inv),
                        opacity,
                    );
                }
            });
        }
    });
}

fn blit_row(
    line: &mut [u8],
    y: i32,
    x0: i32,
    x1: i32,
    texel: &(impl Fn(i64, i64) -> [u8; 4] + Sync),
    (left, top, inv): (f64, f64, f64),
    opacity: f32,
) {
    {
        let v = (y as f64 + 0.5 - top) * inv - 0.5;
        let fy = v.floor();
        let ty = (v - fy) as f32;
        let iy = fy as i64;
        for x in x0..x1 {
            let u = (x as f64 + 0.5 - left) * inv - 0.5;
            let fx = u.floor();
            let tx = (u - fx) as f32;
            let ix = fx as i64;
            // Bilinear in premultiplied terms, so clear texels don't darken
            // the edges of a turned picture.
            let w = [
                (1. - tx) * (1. - ty),
                tx * (1. - ty),
                (1. - tx) * ty,
                tx * ty,
            ];
            let s = [
                texel(ix, iy),
                texel(ix + 1, iy),
                texel(ix, iy + 1),
                texel(ix + 1, iy + 1),
            ];
            let mut acc = [0f32; 3];
            let mut alpha = 0f32;
            for (t, wt) in s.iter().zip(w) {
                let a = t[3] as f32 * wt;
                alpha += a;
                for c in 0..3 {
                    acc[c] += t[c] as f32 * a;
                }
            }
            if alpha <= 0. {
                continue;
            }
            let a = (alpha / 255.) * opacity;
            let o = x as usize * 4;
            for c in 0..3 {
                let src = acc[c] / alpha;
                let dst = line[o + c] as f32;
                line[o + c] = (src * a + dst * (1. - a)).round().clamp(0., 255.) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use videode_core::Value;

    fn flat(w: u32, h: u32, bgra: [u8; 4]) -> Picture {
        Picture {
            width: w,
            height: h,
            bgra: bgra.repeat((w * h) as usize),
            time: 0.,
        }
    }

    fn px(canvas: &[u8], w: u32, x: u32, y: u32) -> [u8; 3] {
        let o = ((y * w + x) * 4) as usize;
        [canvas[o], canvas[o + 1], canvas[o + 2]]
    }

    #[test]
    fn blit_scales_crops_and_blends() {
        let (w, h) = (40, 20);
        let mut canvas = [0, 0, 0, 255].repeat((w * h) as usize);
        let red = flat(10, 5, [0, 0, 255, 255]);
        // Twice the size, at (10, 5), right half cut, half opacity.
        blit(
            &mut canvas,
            w,
            (0, 0, w as i32, h as i32),
            &red,
            (10., 5., 2.),
            (10., 5., 20., 15.),
            0.5,
        );
        assert_eq!(px(&canvas, w, 9, 10), [0, 0, 0], "left of the picture");
        assert_eq!(px(&canvas, w, 15, 10), [0, 0, 128], "half red");
        assert_eq!(px(&canvas, w, 25, 10), [0, 0, 0], "cropped away");
        assert_eq!(px(&canvas, w, 15, 16), [0, 0, 0], "below it");
    }

    #[test]
    fn text_clips_draw_without_media() {
        let mut p = Project::new("t");
        p.sequence.width = 320;
        p.sequence.height = 180;
        let v1 = p.track_by_label("V1").unwrap().id;
        let c = p.add_text_clip(v1, 0, 24, "Hi").unwrap();
        let text = p.clip(c).unwrap().nodes[0].id;
        p.set_param(c, text, "size", Value::Number(60.)).unwrap();
        let paths = HashMap::new();
        // A 640×360 output is the sequence at twice the size.
        let mut comp = Compositor::new(&p, &paths, 640, 360);
        let f = comp.frame(0).unwrap();
        let lit = f.bgra.chunks(4).filter(|px| px[1] > 200).count();
        assert!(lit > 500, "{lit} white pixels");
        assert_eq!(
            comp.frame(100)
                .unwrap()
                .bgra
                .chunks(4)
                .filter(|px| px[1] > 0)
                .count(),
            0
        );
    }
}
