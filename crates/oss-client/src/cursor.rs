//! The game's mouse pointer: a small dart, cyan-edged with a dark core, so the
//! pointer itself says the screens take the mouse. Drawn here rather than
//! shipped as a file, so it needs nothing from the asset folder.

/// Its size in pixels (square).
pub const SIZE: u16 = 20;
/// Where it points: the dart's tip.
pub const HOTSPOT: (u16, u16) = (1, 1);

/// The dart's outline, in pixels: the tip, the right wing, the notch at the
/// back and the lower wing. Symmetric about the diagonal through the tip.
const SHAPE: [(f32, f32); 4] = [(1.0, 1.0), (17.0, 8.0), (9.5, 9.5), (8.0, 17.0)];

const EDGE: [f32; 3] = [0.31, 0.89, 1.0];
const CORE: [f32; 3] = [0.03, 0.075, 0.12];
/// How thick the cyan edge is, in pixels.
const EDGE_W: f32 = 1.3;
/// The dark ring outside the edge, so it shows on a bright background too.
const HALO_W: f32 = 1.0;

/// The pointer as RGBA (straight alpha), `SIZE` by `SIZE`, row by row.
pub fn rgba() -> Vec<u8> {
    const SS: usize = 4;
    let n = SIZE as usize;
    let mut out = vec![0u8; n * n * 4];
    for py in 0..n {
        for px in 0..n {
            // Premultiplied sums over the samples.
            let mut acc = [0.0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let p = (
                        px as f32 + (sx as f32 + 0.5) / SS as f32,
                        py as f32 + (sy as f32 + 0.5) / SS as f32,
                    );
                    let d = signed_distance(p);
                    let (rgb, a) = if d <= -EDGE_W {
                        (CORE, 0.88)
                    } else if d <= 0.0 {
                        (EDGE, 1.0)
                    } else if d <= HALO_W {
                        ([0.0; 3], 0.5)
                    } else {
                        continue;
                    };
                    for k in 0..3 {
                        acc[k] += rgb[k] * a;
                    }
                    acc[3] += a;
                }
            }
            let samples = (SS * SS) as f32;
            let a = acc[3] / samples;
            let at = (py * n + px) * 4;
            if a > 0.0 {
                for k in 0..3 {
                    out[at + k] = (acc[k] / acc[3] * 255.0).round() as u8;
                }
                out[at + 3] = (a * 255.0).round() as u8;
            }
        }
    }
    out
}

/// Distance from `p` to the dart's outline: negative inside, positive outside.
fn signed_distance(p: (f32, f32)) -> f32 {
    let mut best = f32::MAX;
    let mut inside = false;
    for i in 0..SHAPE.len() {
        let a = SHAPE[i];
        let b = SHAPE[(i + 1) % SHAPE.len()];
        best = best.min(segment_distance(p, a, b));
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    if inside { -best } else { best }
}

fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let (cx, cy) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    (cx * cx + cy * cy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(img: &[u8], x: usize, y: usize) -> [u8; 4] {
        let at = (y * SIZE as usize + x) * 4;
        img[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn the_tip_is_at_the_hotspot_and_the_corners_are_clear() {
        let img = rgba();
        assert_eq!(img.len(), SIZE as usize * SIZE as usize * 4);
        let tip = pixel(&img, HOTSPOT.0 as usize, HOTSPOT.1 as usize);
        assert!(tip[3] > 60, "tip {tip:?}");
        assert_eq!(pixel(&img, SIZE as usize - 1, 0)[3], 0);
        assert_eq!(pixel(&img, 0, SIZE as usize - 1)[3], 0);
    }

    #[test]
    fn a_cyan_edge_round_a_dark_core() {
        let img = rgba();
        let core = pixel(&img, 7, 7);
        assert!(core[3] > 200 && core[2] < 60, "core {core:?}");
        // Along the top edge, between the tip and the wing.
        let edge = pixel(&img, 9, 5);
        assert!(edge[1] > 150 && edge[2] > 180, "edge {edge:?}");
    }
}
