//! Image cache: downloads media, decodes and downsizes it off the UI thread, and
//! keeps a bounded LRU of ready-to-draw handles. Loading is triggered lazily when an
//! image scrolls into view (`sensor`), so long chats never decode off-screen media.

use std::collections::{HashMap, VecDeque};

use iced::widget::image::Handle;

use crate::api::Client;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Size {
    /// Avatars: tiny, cached generously.
    Avatar,
    /// Inline message previews.
    Thumb,
    /// Image viewer.
    Full,
}

impl Size {
    fn max_dimension(self) -> u32 {
        match self {
            Size::Avatar => 128,
            Size::Thumb => 640,
            Size::Full => 2560,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub url: String,
    pub size: Size,
}

impl Key {
    pub fn new(url: impl Into<String>, size: Size) -> Self {
        Self { url: url.into(), size }
    }
}

#[derive(Debug, Clone)]
pub struct Image {
    pub handle: Handle,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
enum Entry {
    Loading,
    Ready(Image),
    Failed,
}

/// Limits per size class (rough memory bound: thumbs ≤ 640² RGBA ≈ 1.6 MB each).
fn capacity(size: Size) -> usize {
    match size {
        Size::Avatar => 400,
        Size::Thumb => 60,
        Size::Full => 3,
    }
}

#[derive(Debug, Default)]
pub struct Cache {
    entries: HashMap<Key, Entry>,
    recency: VecDeque<Key>,
}

impl Cache {
    pub fn get(&self, key: &Key) -> Option<&Image> {
        match self.entries.get(key) {
            Some(Entry::Ready(image)) => Some(image),
            _ => None,
        }
    }

    pub fn failed(&self, key: &Key) -> bool {
        matches!(self.entries.get(key), Some(Entry::Failed))
    }

    /// Marks `key` as loading; returns false when it is already loading/loaded.
    pub fn begin(&mut self, key: &Key) -> bool {
        match self.entries.get(key) {
            Some(Entry::Loading | Entry::Ready(_)) => {
                self.touch(key);
                false
            }
            _ => {
                self.entries.insert(key.clone(), Entry::Loading);
                true
            }
        }
    }

    pub fn finish(&mut self, key: Key, result: Result<Image, String>) {
        match result {
            Ok(image) => {
                self.entries.insert(key.clone(), Entry::Ready(image));
                self.touch(&key);
                self.evict(key.size);
            }
            Err(error) => {
                log::info!("media {} failed: {error}", key.url);
                self.entries.insert(key, Entry::Failed);
            }
        }
    }

    /// Forget failures so that a retry (e.g. after reconnecting) is possible.
    pub fn clear_failures(&mut self) {
        self.entries.retain(|_, entry| !matches!(entry, Entry::Failed));
    }

    fn touch(&mut self, key: &Key) {
        if let Some(position) = self.recency.iter().position(|item| item == key) {
            self.recency.remove(position);
        }
        self.recency.push_back(key.clone());
    }

    fn evict(&mut self, size: Size) {
        let limit = capacity(size);
        let mut count = self.recency.iter().filter(|key| key.size == size).count();
        while count > limit {
            let Some(position) = self.recency.iter().position(|key| key.size == size) else { break };
            if let Some(key) = self.recency.remove(position) {
                self.entries.remove(&key);
            }
            count -= 1;
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Downloads and decodes an image, scaled to fit the size class.
pub async fn load(client: Client, key: Key) -> Result<Image, String> {
    let bytes = client.download(&key.url).await.map_err(|error| error.message)?;
    let max = key.size.max_dimension();
    tokio::task::spawn_blocking(move || decode(&bytes, max)).await.map_err(|error| error.to_string())?
}

pub fn decode(bytes: &[u8], max: u32) -> Result<Image, String> {
    let decoded = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    let decoded = if decoded.width() > max || decoded.height() > max {
        decoded.resize(max, max, image::imageops::FilterType::Triangle)
    } else {
        decoded
    };
    let rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(Image { handle: Handle::from_rgba(width, height, rgba.into_raw()), width, height })
}

/// Decodes a local file (attachment previews).
pub async fn load_file(path: std::path::PathBuf, size: Size) -> Result<Image, String> {
    let bytes = tokio::fs::read(&path).await.map_err(|error| error.to_string())?;
    let max = size.max_dimension();
    tokio::task::spawn_blocking(move || decode(&bytes, max)).await.map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn decodes_and_downscales() {
        let image = decode(&png(2000, 1000), 640).unwrap();
        assert_eq!((image.width, image.height), (640, 320));
        let small = decode(&png(20, 10), 640).unwrap();
        assert_eq!((small.width, small.height), (20, 10));
        assert!(decode(b"not an image", 640).is_err());
    }

    #[test]
    fn lru_evicts_per_size_class() {
        let mut cache = Cache::default();
        let image = decode(&png(4, 4), 64).unwrap();
        for index in 0..(capacity(Size::Full) + 2) {
            let key = Key::new(format!("u{index}"), Size::Full);
            assert!(cache.begin(&key));
            cache.finish(key, Ok(image.clone()));
        }
        assert_eq!(cache.len(), capacity(Size::Full));
        assert!(cache.get(&Key::new("u0", Size::Full)).is_none());
        let latest = Key::new(format!("u{}", capacity(Size::Full) + 1), Size::Full);
        assert!(cache.get(&latest).is_some());
        assert!(!cache.begin(&latest));

        let broken = Key::new("x", Size::Thumb);
        cache.begin(&broken);
        cache.finish(broken.clone(), Err("boom".into()));
        assert!(cache.failed(&broken));
        cache.clear_failures();
        assert!(cache.begin(&broken));
    }
}
