//! Interconnect Engine: In-Memory Direct Link, Dynamic Composition Linking, and Frame/Clipboard History.
//!
//! Enables zero-latency data, frame, and layer exchange between:
//! - PhotoCraft (Image Editing, Layers, Retouching, PSDs)
//! - FilmCraft (Video Sequences, Timeline, Audio, Color)
//! - EffectCraft (Compositions, Motion Graphics, VFX)

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedAsset {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub kind: AssetKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetKind {
    Image,
    Video,
    Audio,
    Psd,
    Composition,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct CapturedFrame {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub png_bytes: Vec<u8>,
    pub time_secs: f64,
}

/// A captured frame stored in the Studio Clipboard and Frame History.
#[derive(Clone)]
pub struct HistoryFrame {
    pub id: String,
    pub name: String,
    pub source_label: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub png_bytes: Vec<u8>,
    pub thumb_width: u32,
    pub thumb_height: u32,
    pub thumb_rgba: Vec<u8>,
    pub time_formatted: String,
}

/// A Dynamic Link between an EffectCraft composition and a FilmCraft sequence / clip.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct LinkedComposition {
    pub comp_id: u64,
    pub comp_name: String,
    pub file_path: PathBuf,
    pub film_item_id: Option<u64>,
    pub film_clip_id: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub duration_secs: f64,
    pub last_sync_time: std::time::Instant,
}

pub struct InterconnectBridge {
    pub shared_assets: Arc<Mutex<Vec<SharedAsset>>>,
    pub pending_photo_frame: Arc<Mutex<Option<CapturedFrame>>>,
    pub frame_history: Arc<Mutex<Vec<HistoryFrame>>>,
    pub linked_compositions: Arc<Mutex<Vec<LinkedComposition>>>,
    pub status_message: Arc<Mutex<Option<(String, std::time::Instant)>>>,
}

impl Default for InterconnectBridge {
    fn default() -> Self {
        Self {
            shared_assets: Arc::new(Mutex::new(Vec::new())),
            pending_photo_frame: Arc::new(Mutex::new(None)),
            frame_history: Arc::new(Mutex::new(Vec::new())),
            linked_compositions: Arc::new(Mutex::new(Vec::new())),
            status_message: Arc::new(Mutex::new(Some((
                "⚡ Dynamic Link active (in-memory zero copy)".to_string(),
                std::time::Instant::now(),
            )))),
        }
    }
}

impl InterconnectBridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a transient status message displayed on the top studio ribbon.
    pub fn set_status(&self, msg: impl Into<String>) {
        if let Ok(mut status) = self.status_message.lock() {
            *status = Some((msg.into(), std::time::Instant::now()));
        }
    }

    /// Retrieve the current active status message (if not timed out).
    pub fn current_status(&self) -> Option<String> {
        let lock = self.status_message.lock().ok()?;
        if let Some((msg, time)) = lock.as_ref() {
            if time.elapsed() < std::time::Duration::from_secs(6) {
                return Some(msg.clone());
            }
        }
        None
    }

    /// Add a captured frame to Studio History, encode to PNG, and automatically copy to the OS clipboard.
    pub fn add_frame_to_history(
        &self,
        name: String,
        source_label: String,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<HistoryFrame, String> {
        // 1. Encode to PNG
        let mut png_bytes = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        image::ImageEncoder::write_image(
            encoder,
            &rgba,
            width,
            height,
            image::ExtendedColorType::Rgba8,
        ).map_err(|e| format!("PNG encode failed: {e}"))?;

        // 2. Downsample thumbnail (max 180px)
        let step = ((width.max(height) as f32) / 180.0).max(1.0) as u32;
        let thumb_width = (width.div_ceil(step)).max(1);
        let thumb_height = (height.div_ceil(step)).max(1);
        let mut thumb_rgba = Vec::with_capacity((thumb_width * thumb_height * 4) as usize);

        for y in 0..thumb_height {
            for x in 0..thumb_width {
                let src_x = (x * step).min(width.saturating_sub(1));
                let src_y = (y * step).min(height.saturating_sub(1));
                let idx = ((src_y * width + src_x) * 4) as usize;
                if idx + 4 <= rgba.len() {
                    thumb_rgba.extend_from_slice(&rgba[idx..idx + 4]);
                } else {
                    thumb_rgba.extend_from_slice(&[0, 0, 0, 255]);
                }
            }
        }

        // 3. Copy to OS System Clipboard via arboard
        self.copy_to_system_clipboard(&rgba, width, height);

        let time_formatted = chrono::Local::now().format("%H:%M:%S").to_string();
        let id = format!("frame-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0));

        let frame = HistoryFrame {
            id,
            name: name.clone(),
            source_label,
            width,
            height,
            rgba,
            png_bytes,
            thumb_width,
            thumb_height,
            thumb_rgba,
            time_formatted,
        };

        if let Ok(mut hist) = self.frame_history.lock() {
            // Keep at most 40 recent captures
            if hist.len() >= 40 {
                hist.remove(0);
            }
            hist.push(frame.clone());
        }

        self.set_status(format!("📸 Captured '{}' ({width}x{height}) -> Copied to Clipboard & Studio History", name));
        Ok(frame)
    }

    /// Copy raw RGBA directly into OS System Clipboard.
    pub fn copy_to_system_clipboard(&self, rgba: &[u8], width: u32, height: u32) {
        if let Ok(mut clip) = arboard::Clipboard::new() {
            let img = arboard::ImageData {
                width: width as usize,
                height: height as usize,
                bytes: std::borrow::Cow::Borrowed(rgba),
            };
            let _ = clip.set_image(img);
        }
    }

    /// Send an image frame directly to PhotoCraft for paint/retouch.
    #[allow(dead_code)]
    pub fn queue_frame_to_photo(&self, name: String, width: u32, height: u32, rgba: &[u8], time_secs: f64) -> Result<(), String> {
        let mut png_bytes = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        image::ImageEncoder::write_image(
            encoder,
            rgba,
            width,
            height,
            image::ExtendedColorType::Rgba8,
        ).map_err(|e| format!("PNG encode failed: {e}"))?;

        if let Ok(mut pending) = self.pending_photo_frame.lock() {
            *pending = Some(CapturedFrame {
                name: name.clone(),
                width,
                height,
                png_bytes,
                time_secs,
            });
        }

        self.set_status(format!("Captured frame '{name}' @ {time_secs:.2}s -> Opened in PhotoCraft"));
        Ok(())
    }

    /// Queue a frame from history into PhotoCraft.
    pub fn queue_history_frame_to_photo(&self, frame: &HistoryFrame) {
        if let Ok(mut pending) = self.pending_photo_frame.lock() {
            *pending = Some(CapturedFrame {
                name: frame.name.clone(),
                width: frame.width,
                height: frame.height,
                png_bytes: frame.png_bytes.clone(),
                time_secs: 0.0,
            });
        }
        self.set_status(format!("Opened '{}' from history in PhotoCraft for retouching", frame.name));
    }

    /// Delete a single frame from history.
    pub fn delete_history_frame(&self, id: &str) {
        if let Ok(mut hist) = self.frame_history.lock() {
            hist.retain(|f| f.id != id);
        }
    }

    /// Clear all captured history.
    pub fn clear_history(&self) {
        if let Ok(mut hist) = self.frame_history.lock() {
            hist.clear();
        }
        self.set_status("Cleared Studio Frame History");
    }

    /// Register or update a Dynamic Link composition.
    pub fn register_linked_comp(&self, comp: LinkedComposition) {
        if let Ok(mut list) = self.linked_compositions.lock() {
            list.retain(|c| c.comp_id != comp.comp_id);
            list.push(comp);
        }
    }

    /// Retrieve a linked composition by its EffectCraft ItemId.
    pub fn get_linked_comp_by_comp_id(&self, comp_id: u64) -> Option<LinkedComposition> {
        let lock = self.linked_compositions.lock().ok()?;
        lock.iter().find(|c| c.comp_id == comp_id).cloned()
    }

    /// Retrieve a linked composition by its FilmCraft project ItemId.
    pub fn get_linked_comp_by_item_id(&self, item_id: u64) -> Option<LinkedComposition> {
        let lock = self.linked_compositions.lock().ok()?;
        lock.iter().find(|c| c.film_item_id == Some(item_id)).cloned()
    }

    /// Retrieve a linked composition by its FilmCraft timeline ClipId.
    pub fn get_linked_comp_by_clip_id(&self, clip_id: u64) -> Option<LinkedComposition> {
        let lock = self.linked_compositions.lock().ok()?;
        lock.iter().find(|c| c.film_clip_id == Some(clip_id)).cloned()
    }

    /// Retrieve all linked compositions.
    pub fn all_linked_comps(&self) -> Vec<LinkedComposition> {
        self.linked_compositions.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// Register a media asset in the shared project bin.
    pub fn register_asset(&self, name: String, path: PathBuf, kind: AssetKind) {
        if let Ok(mut assets) = self.shared_assets.lock() {
            assets.retain(|a| a.path != path);
            let id = format!("asset-{}", assets.len() + 1);
            assets.push(SharedAsset {
                id,
                name: name.clone(),
                path,
                kind,
            });
        }
        self.set_status(format!("Added '{name}' to shared Studio media bin"));
    }
}
