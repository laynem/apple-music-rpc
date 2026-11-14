//! Draws the tray icon (a music note on a colored disc) so no image file is needed.

pub const SIZE: u32 = 32;

fn in_note(x: f32, y: f32) -> bool {
    let ellipse = |cx: f32, cy: f32| ((x - cx) / 3.7).powi(2) + ((y - cy) / 2.9).powi(2) <= 1.0;
    let beam_top = 9.0 - (x - 13.4) * (2.0 / 10.8);
    ellipse(11.4, 22.2)
        || ellipse(20.4, 20.2)
        || ((13.4..=15.2).contains(&x) && y >= beam_top && y <= 22.2)
        || ((22.4..=24.2).contains(&x) && y >= beam_top && y <= 20.2)
        || ((13.4..=24.2).contains(&x) && y >= beam_top && y <= beam_top + 3.6)
}

/// RGBA pixels, 4x4 supersampled for smooth edges.
pub fn rgba(color: (u8, u8, u8)) -> Vec<u8> {
    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for py in 0..SIZE {
        for px in 0..SIZE {
            let (mut disc, mut note) = (0u32, 0u32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = px as f32 + (sx as f32 + 0.5) / 4.0;
                    let y = py as f32 + (sy as f32 + 0.5) / 4.0;
                    if (x - 16.0).powi(2) + (y - 16.0).powi(2) <= 15.0f32.powi(2) {
                        disc += 1;
                        if in_note(x, y) {
                            note += 1;
                        }
                    }
                }
            }
            if disc == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let mix = |c: u8| ((c as u32 * (disc - note) + 255 * note) / disc) as u8;
            out.extend_from_slice(&[mix(color.0), mix(color.1), mix(color.2), (disc * 255 / 16) as u8]);
        }
    }
    out
}

pub const DOT: u32 = 16;

/// RGBA pixels of a small filled circle, used as a status light in the menu.
pub fn dot(color: (u8, u8, u8)) -> Vec<u8> {
    let mut out = Vec::with_capacity((DOT * DOT * 4) as usize);
    for py in 0..DOT {
        for px in 0..DOT {
            let mut hits = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = px as f32 + (sx as f32 + 0.5) / 4.0;
                    let y = py as f32 + (sy as f32 + 0.5) / 4.0;
                    if (x - 8.0).powi(2) + (y - 8.0).powi(2) <= 4.5f32.powi(2) {
                        hits += 1;
                    }
                }
            }
            out.extend_from_slice(&[color.0, color.1, color.2, (hits * 255 / 16) as u8]);
        }
    }
    out
}
