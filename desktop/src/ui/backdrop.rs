//! The web app's backdrop and glass window, pre-composed into one image per window size.
//!
//! The web draws an atmospheric scene (`body::before`), and on top of it a floating window
//! (`.messenger`) with a soft shadow, a frosted copy of the scene, a top sheen and a hairline
//! rim. Both scenes are rendered from `frontend/src/index.css` by `scripts/gen-backdrop.mjs`.
//! Compositing them here (off the UI thread, once per window size) makes the result identical
//! on the GPU and the CPU renderer, and leaves only a single image blit per frame.

use std::sync::LazyLock;

use iced::widget::image::Handle;
use iced::{Point, Rectangle, Size};
use image::imageops::{self, FilterType};
use image::{GenericImageView, RgbImage};

/// Where the app sits in the OS window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub rect: Rectangle,
    pub radius: f32,
}

impl Frame {
    /// The app fills the whole OS window (the web's edge-to-edge layout); the floating
    /// inset window is not used on the desktop.
    pub fn for_window(size: Size) -> Self {
        Frame { rect: Rectangle::new(Point::ORIGIN, size), radius: 0.0 }
    }

    pub fn padding(&self, window: Size) -> iced::Padding {
        iced::Padding {
            top: self.rect.y,
            left: self.rect.x,
            right: (window.width - self.rect.x - self.rect.width).max(0.0),
            bottom: (window.height - self.rect.y - self.rect.height).max(0.0),
        }
    }
}

const SHARP_BYTES: &[u8] = include_bytes!("../../assets/backdrop.jpg");
const FROSTED_BYTES: &[u8] = include_bytes!("../../assets/backdrop-frosted.jpg");

static SHARP: LazyLock<RgbImage> = LazyLock::new(|| decode(SHARP_BYTES));
static FROSTED: LazyLock<RgbImage> = LazyLock::new(|| decode(FROSTED_BYTES));

/// The plain scene (sign-in and loading screens), stretched to the window like the web's.
pub static SCENE: LazyLock<Handle> = LazyLock::new(|| Handle::from_bytes(SHARP_BYTES));

fn decode(bytes: &[u8]) -> RgbImage {
    image::load_from_memory(bytes).map(|image| image.to_rgb8()).unwrap_or_else(|_| RgbImage::from_pixel(16, 10, image::Rgb([13, 13, 15])))
}

/// Web `--shadow-window`: `0 40px 120px -30px rgba(0,0,0,.75), 0 10px 30px -10px rgba(0,0,0,.45)`.
const SHADOWS: [(f32, f32, f32, f32); 2] = [(40.0, 120.0, -30.0, 0.75), (10.0, 30.0, -10.0, 0.45)];

/// Composes the backdrop for a window of `logical` size at `scale` (physical px per logical px).
pub fn compose(logical: Size, scale: f32) -> Handle {
    let scale = if scale.is_finite() && scale > 0.0 { scale.min(4.0) } else { 1.0 };
    let width = ((logical.width * scale).round() as u32).clamp(1, 8192);
    let height = ((logical.height * scale).round() as u32).clamp(1, 8192);
    let frame = Frame::for_window(logical);
    let rect = Rectangle { x: frame.rect.x * scale, y: frame.rect.y * scale, width: frame.rect.width * scale, height: frame.rect.height * scale };
    let radius = frame.radius * scale;

    // The frosted scene is pinned to the viewport (`background-attachment: fixed`), so the
    // window shows the part of it that lies behind the window.
    let frosted = if frame.radius > 0.0 {
        let (sw, sh) = FROSTED.dimensions();
        let (fx, fy) = (sw as f32 / width as f32, sh as f32 / height as f32);
        let x = ((rect.x * fx) as u32).min(sw - 1);
        let y = ((rect.y * fy) as u32).min(sh - 1);
        let w = ((rect.width * fx) as u32).clamp(1, sw - x);
        let h = ((rect.height * fy) as u32).clamp(1, sh - y);
        imageops::resize(&FROSTED.view(x, y, w, h).to_image(), (rect.width as u32).max(1), (rect.height as u32).max(1), FilterType::Triangle)
    } else {
        imageops::resize(&*FROSTED, width, height, FilterType::Triangle)
    };

    let mut out = if frame.radius > 0.0 { imageops::resize(&*SHARP, width, height, FilterType::Triangle) } else { frosted.clone() };
    let (ox, oy) = (rect.x.round() as u32, rect.y.round() as u32);
    let (half_w, half_h) = (rect.width / 2.0, rect.height / 2.0);
    let (cx, cy) = (rect.x + half_w, rect.y + half_h);

    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let mut rgb = [pixel[0] as f32, pixel[1] as f32, pixel[2] as f32];
        let inside = if frame.radius > 0.0 {
            let d = rounded_box_sdf(px - cx, py - cy, half_w, half_h, radius);
            if d > -1.0 {
                // Outside (or on the edge of) the window: the shadow falls on the scene.
                for (offset, blur, spread, alpha) in SHADOWS {
                    let (offset, blur, spread) = (offset * scale, blur * scale, spread * scale);
                    let ds = rounded_box_sdf(px - cx, py - cy - offset, half_w + spread, half_h + spread, (radius + spread).max(0.0));
                    // CSS blur radius b ≈ a Gaussian with σ = b/2.
                    let a = alpha * 0.5 * (1.0 - erf(ds / (blur / 2.0 * std::f32::consts::SQRT_2)));
                    rgb = rgb.map(|c| c * (1.0 - a));
                }
            }
            (0.5 - d).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if inside > 0.0 {
            let (wx, wy) = (x.saturating_sub(ox).min(frosted.width() - 1), y.saturating_sub(oy).min(frosted.height() - 1));
            let glass = frosted.get_pixel(wx, wy);
            let mut g = [glass[0] as f32, glass[1] as f32, glass[2] as f32];
            // Sheen: linear-gradient(180deg, rgba(255,255,255,.035), transparent 30%).
            let t = (py - rect.y) / rect.height.max(1.0);
            if t < 0.3 {
                let a = 0.035 * (1.0 - t / 0.3);
                g = g.map(|c| c + (255.0 - c) * a);
            }
            if frame.radius > 0.0 {
                // 1px rim: rgba(255,255,255,.09) border, plus the inset top highlight (.06).
                let d = rounded_box_sdf(px - cx, py - cy, half_w, half_h, radius);
                let rim = ((0.5 - d).clamp(0.0, 1.0) - (0.5 - d - scale).clamp(0.0, 1.0)).clamp(0.0, 1.0);
                if rim > 0.0 {
                    g = g.map(|c| c + (255.0 - c) * 0.09 * rim);
                }
                let below_top = py - rect.y - scale;
                if (0.0..scale).contains(&below_top) && px > rect.x + radius && px < rect.x + rect.width - radius {
                    g = g.map(|c| c + (255.0 - c) * 0.06);
                }
            }
            rgb = [0, 1, 2].map(|i| rgb[i] + (g[i] - rgb[i]) * inside);
        }
        *pixel = image::Rgb(rgb.map(|c| c.round().clamp(0.0, 255.0) as u8));
    }

    let rgba: Vec<u8> = out.pixels().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    Handle::from_rgba(width, height, rgba)
}

/// Signed distance from a point (relative to the box centre) to a rounded box.
fn rounded_box_sdf(x: f32, y: f32, half_w: f32, half_h: f32, radius: f32) -> f32 {
    let radius = radius.min(half_w).min(half_h).max(0.0);
    let qx = x.abs() - half_w + radius;
    let qy = y.abs() - half_h + radius;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

/// Abramowitz–Stegun 7.1.26 (max error 1.5e-7).
fn erf(x: f32) -> f32 {
    let sign = x.signum();
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0 - (((((1.061_405_4 * t - 1.453_152_1) * t) + 1.421_413_8) * t - 0.284_496_74) * t + 0.254_829_6) * t * (-x * x).exp();
    sign * y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_fills_the_window() {
        for size in [Size::new(1280.0, 800.0), Size::new(1000.0, 700.0)] {
            let frame = Frame::for_window(size);
            assert_eq!(frame.rect, Rectangle::new(Point::ORIGIN, size));
            assert_eq!(frame.radius, 0.0);
        }
    }

    #[test]
    fn sdf_and_erf() {
        assert!((rounded_box_sdf(0.0, 0.0, 10.0, 10.0, 2.0) + 10.0).abs() < 1e-4);
        assert!((rounded_box_sdf(15.0, 0.0, 10.0, 10.0, 2.0) - 5.0).abs() < 1e-4);
        assert!(erf(0.0).abs() < 1e-6 && (erf(3.0) - 1.0).abs() < 1e-4 && (erf(-1.0) + 0.842_700_8).abs() < 1e-5);
    }

    #[test]
    fn composes_both_layouts() {
        let _ = compose(Size::new(1280.0, 800.0), 1.0);
        let _ = compose(Size::new(900.0, 600.0), 1.25);
    }
}
