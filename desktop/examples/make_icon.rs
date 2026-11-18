//! Regenerates assets/icon.ico (16–256 px) from assets/brand-mark.webp.
//! Usage: cargo run --example make_icon
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;

fn main() {
    let source = image::open("assets/brand-mark.webp").expect("brand mark decodes");
    let frames: Vec<IcoFrame> = [16u32, 24, 32, 48, 64, 128, 256]
        .iter()
        .map(|&size| {
            let resized = source.resize_exact(size, size, FilterType::Lanczos3).into_rgba8();
            // PNG-compressed frames are valid in .ico files on every Windows since Vista.
            IcoFrame::as_png(resized.as_raw(), size, size, image::ExtendedColorType::Rgba8).expect("png frame")
        })
        .collect();
    let file = std::fs::File::create("assets/icon.ico").expect("icon.ico writable");
    IcoEncoder::new(file).encode_images(&frames).expect("ico encodes");
    println!("wrote assets/icon.ico");
}

