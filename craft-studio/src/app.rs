//! CraftStudio Main Application and Unified Workspace Orchestrator.
//!
//! Interconnects PhotoCraft (Photo editing), FilmCraft (Video editing)
//! and EffectCraft (Compositing & Motion Graphics) in a single unified window.

use std::path::PathBuf;
use std::sync::Arc;
use eframe::egui::{self, Color32, Layout, RichText, Vec2, Sense};

use photocraft_ui_egui::{PhotocraftApp, Services as PhotoServices};
use photocraft_engine::Session as PhotoSession;

use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_engine::Session as FilmSession;

use effectcraft_ui_egui::EffectcraftApp;

use crate::interconnect::{InterconnectBridge, AssetKind, LinkedComposition, HistoryFrame};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StudioWorkspace {
    Photo,
    Video,
    Compositing,
    SplitPhotoVideo,
    SplitVideoVfx,
}

pub struct CraftStudioApp {
    pub workspace: StudioWorkspace,
    pub photo_app: PhotocraftApp,
    pub film_app: FilmcraftApp,
    pub effect_app: EffectcraftApp,
    pub bridge: Arc<InterconnectBridge>,
    pub split_ratio: f32,
    pub show_media_drawer: bool,
    pub show_history_drawer: bool,
}

impl CraftStudioApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        // 1. Initialize PhotoCraft with native IO and file dialogs
        let photo_session = PhotoSession::new();
        let mut photo_services = PhotoServices::default();
        photo_services.write = Some(Box::new(|path: &str, bytes: &[u8]| {
            std::fs::write(path, bytes).map_err(|e| e.to_string())
        }));
        photo_services.import = Some(Box::new(|name: &str, bytes: &[u8], max_svg_group_depth: usize| {
            photocraft_io::import_with_svg_group_depth(name, bytes, max_svg_group_depth)
                .map(|r| (r.document, r.warnings))
                .map_err(|e| e.to_string())
        }));
        photo_services.export = Some(Box::new(|doc: &photocraft_doc::Document, path: &str, settings: &photocraft_ui_egui::ExportSettings| {
            let mut opts = photocraft_io::ExportOptions::default();
            if let Some(q) = settings.jpeg_quality {
                opts.encode.jpeg_quality = q;
            }
            opts.encode.webp_lossless = settings.webp_lossless;
            if let Some(q) = settings.webp_quality {
                opts.encode.webp_quality = q;
            }
            opts.tiff_layers = settings.tiff_layers;
            opts.xmp = if settings.xmp_all { photocraft_io::XmpEmbed::All } else { photocraft_io::XmpEmbed::None };
            photocraft_io::export(doc, path, &opts).map(|r| (r.bytes, r.warnings)).map_err(|e| e.to_string())
        }));
        photo_services.file_dialog = Some(Box::new(|request, _parent, reply| {
            std::thread::spawn(move || {
                let mut dialog = rfd::FileDialog::new();
                match request {
                    photocraft_ui_egui::FileDialogRequest::Open { multiple, initial_dir, extensions } => {
                        if let Some(dir) = initial_dir {
                            dialog = dialog.set_directory(dir);
                        }
                        if let Some(exts) = extensions {
                            let borrowed: Vec<&str> = exts.iter().map(String::as_str).collect();
                            dialog = dialog.add_filter("Supported Files", &borrowed);
                        } else {
                            dialog = dialog.add_filter("All Supported Formats", &["png", "jpg", "jpeg", "psd", "psb", "webp", "tif", "tiff", "svg", "pcraft"]);
                        }
                        if multiple {
                            let picked = dialog.pick_files();
                            let ans = picked.map(|files| photocraft_ui_egui::FileDialogAnswer::Paths(files.iter().map(|f| f.to_string_lossy().to_string()).collect()));
                            reply.send(ans);
                        } else {
                            let picked = dialog.pick_file();
                            let ans = picked.map(|file| photocraft_ui_egui::FileDialogAnswer::Paths(vec![file.to_string_lossy().to_string()]));
                            reply.send(ans);
                        }
                    }
                    photocraft_ui_egui::FileDialogRequest::Save { suggested } => {
                        dialog = dialog.add_filter("PSD Document", &["psd", "psb"])
                            .add_filter("PNG", &["png"])
                            .add_filter("JPEG", &["jpg", "jpeg"])
                            .add_filter("PhotoCraft", &["pcraft"]);
                        let path = std::path::Path::new(&suggested);
                        if let Some(name) = path.file_name() {
                            dialog = dialog.set_file_name(name.to_string_lossy());
                        }
                        if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                            dialog = dialog.set_directory(dir);
                        }
                        let picked = dialog.save_file();
                        let ans = picked.map(|file| photocraft_ui_egui::FileDialogAnswer::SaveTo(file.to_string_lossy().to_string()));
                        reply.send(ans);
                    }
                }
            });
        }));

        let mut photo_app = PhotocraftApp::new(photo_session, photo_services);
        photo_app.integrated_titlebar = false;
        photo_app.custom_titlebar = false;
        if let Some(rs) = cc.wgpu_render_state.clone() {
            photo_app.set_wgpu(rs);
        }

        // 2. Initialize FilmCraft with native file dialog hooks and GPU acceleration
        let film_session = FilmSession::default();
        let mut film_app = FilmcraftApp::new(film_session);
        if let Some(rs) = cc.wgpu_render_state.clone() {
            film_app.set_wgpu(rs);
        }
        film_app.hooks.pick_files = Some(Box::new(|exts: &[&str]| {
            rfd::FileDialog::new()
                .add_filter("Media", exts)
                .pick_files()
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        }));
        film_app.hooks.pick_file_for_relink = Some(Box::new(|exts: &[&str], _hint| {
            rfd::FileDialog::new()
                .add_filter("Media", exts)
                .pick_file()
                .map(|p| p.to_string_lossy().to_string())
        }));
        film_app.hooks.pick_open_project = Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("FilmCraft Project", &["fcproj"])
                .pick_file()
                .map(|p| p.to_string_lossy().to_string())
        }));
        film_app.hooks.pick_save = Some(Box::new(|name: &str| {
            rfd::FileDialog::new()
                .add_filter("FilmCraft Project", &["fcproj"])
                .set_file_name(name)
                .save_file()
                .map(|p| p.to_string_lossy().to_string())
        }));
        film_app.hooks.pick_save_as = Some(Box::new(|filter: &str, exts: &[&str], name: &str| {
            let path = std::path::Path::new(name);
            let mut dialog = rfd::FileDialog::new()
                .add_filter(filter, exts)
                .set_file_name(path.file_name().map_or(name.into(), |n| n.to_string_lossy()));
            if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
                dialog = dialog.set_directory(dir);
            }
            dialog.save_file().map(|p| p.to_string_lossy().to_string())
        }));
        film_app.hooks.pick_folder = Some(Box::new(|| {
            rfd::FileDialog::new().pick_folder().map(|p| p.to_string_lossy().to_string())
        }));
        film_app.hooks.pick_open_file = Some(Box::new(|filter: &str, exts: &[&str]| {
            rfd::FileDialog::new()
                .add_filter(filter, exts)
                .pick_file()
                .map(|p| p.to_string_lossy().to_string())
        }));

        // 3. Initialize EffectCraft with full media decoding, importer, and native file dialog hooks
        let mut effect_session = effectcraft_host::session();
        effect_session.check_footage_on_open = true;
        let mut effect_app = EffectcraftApp::new(effect_session);
        effect_app.hooks.pick_files = Some(Box::new(|exts: &[&str]| {
            rfd::FileDialog::new()
                .add_filter("Media", exts)
                .pick_files()
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        }));
        effect_app.hooks.pick_open_project = Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("EffectCraft Project", &["ecproj", "ecprojx"])
                .pick_file()
                .map(|p| p.to_string_lossy().to_string())
        }));
        effect_app.hooks.pick_folder = Some(Box::new(|| {
            rfd::FileDialog::new().pick_folder().map(|p| p.to_string_lossy().to_string())
        }));
        effect_app.hooks.pick_save = Some(Box::new(|name: &str| {
            rfd::FileDialog::new()
                .add_filter("EffectCraft Project", &["ecproj"])
                .set_file_name(name)
                .save_file()
                .map(|p| p.to_string_lossy().to_string())
        }));
        effect_app.hooks.pick_save_file = Some(Box::new(|name: &str, ext: &str| {
            let path = std::path::Path::new(name);
            let mut d = if ext.is_empty() {
                rfd::FileDialog::new()
            } else {
                rfd::FileDialog::new().add_filter(ext, &[ext])
            };
            if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
                d = d.set_directory(dir);
            }
            let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            d.set_file_name(file).save_file().map(|p| p.to_string_lossy().to_string())
        }));

        let bridge = Arc::new(InterconnectBridge::new());

        Self {
            workspace: StudioWorkspace::Video,
            photo_app,
            film_app,
            effect_app,
            bridge,
            split_ratio: 0.5,
            show_media_drawer: false,
            show_history_drawer: false,
        }
    }

    /// Cache directory for dynamic link proxy frames and compositions.
    pub fn dynamic_cache_dir(&self) -> PathBuf {
        let dir = std::env::temp_dir().join("craft_studio_links");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// Process any cross-app requests in the Direct Link pipeline.
    fn process_interconnect_pipeline(&mut self) {
        // If a video or VFX frame was captured for PhotoCraft, open it as a new document
        let pending = {
            if let Ok(mut lock) = self.bridge.pending_photo_frame.lock() {
                lock.take()
            } else {
                None
            }
        };

        if let Some(frame) = pending {
            if let Err(e) = self.photo_app.open_bytes(&frame.name, &frame.png_bytes) {
                self.bridge.set_status(format!("Error opening frame in PhotoCraft: {e}"));
            } else {
                self.workspace = StudioWorkspace::Photo;
                self.bridge.set_status(format!("Opened '{}' in Photo workspace for retouching", frame.name));
            }
        }
    }

    /// Capture current active frame into clipboard history, and optionally send directly to PhotoCraft.
    pub fn capture_frame(&mut self, open_in_photo: bool) {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();

        match self.workspace {
            StudioWorkspace::Photo => {
                self.bridge.set_status("PhotoCraft is active. Capture works from Video (FilmCraft) or VFX (EffectCraft).");
            }
            StudioWorkspace::Compositing => {
                let comp_id = self.effect_app.session.state.active_comp
                    .or_else(|| self.effect_app.session.project.comps().next().map(|(id, _)| *id));

                if let Some(cid) = comp_id {
                    let tick = self.effect_app.session.state.times.get(&cid).copied().unwrap_or(effectcraft_time::Tick(0));
                    let comp_name = self.effect_app.session.project.item(cid).map(|i| i.name.clone()).unwrap_or_else(|| "Comp".into());

                    match self.effect_app.session.render_rgba8_alpha(cid, tick, 0, true) {
                        Ok((w, h, rgba)) => {
                            let name = format!("vfx_frame_{timestamp}.png");
                            let label = format!("✨ EffectCraft: {comp_name} @ {:.2}s", tick.seconds());
                            if let Ok(frame) = self.bridge.add_frame_to_history(name, label, w, h, rgba) {
                                if open_in_photo {
                                    self.bridge.queue_history_frame_to_photo(&frame);
                                }
                            }
                        }
                        Err(e) => {
                            self.bridge.set_status(format!("Render error from EffectCraft: {e}"));
                        }
                    }
                } else {
                    self.bridge.set_status("No composition open in EffectCraft to capture.");
                }
            }
            StudioWorkspace::Video | StudioWorkspace::SplitPhotoVideo | StudioWorkspace::SplitVideoVfx => {
                let image_opt = self.film_app.session.render_program(1.0);
                if let Some(img) = image_opt {
                    let width = img.w as u32;
                    let height = img.h as u32;
                    let mut rgba_u8 = Vec::with_capacity(width as usize * height as usize * 4);
                    for chunk in img.px.chunks_exact(4) {
                        let r = (chunk[0].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                        let g = (chunk[1].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                        let b = (chunk[2].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                        let a = (chunk[3].clamp(0.0, 1.0) * 255.0) as u8;
                        rgba_u8.extend_from_slice(&[r, g, b, a]);
                    }

                    let seq_name = self.film_app.session.state.active_sequence
                        .and_then(|id| self.film_app.session.project.item(id))
                        .map(|i| i.name.clone())
                        .unwrap_or_else(|| "Video Timeline".into());

                    let playhead_secs = self.film_app.session.playhead().seconds();
                    let name = format!("video_frame_{timestamp}.png");
                    let label = format!("🎬 FilmCraft: {seq_name} @ {:.2}s", playhead_secs);

                    if let Ok(frame) = self.bridge.add_frame_to_history(name, label, width, height, rgba_u8) {
                        if open_in_photo {
                            self.bridge.queue_history_frame_to_photo(&frame);
                        }
                    }
                } else {
                    self.bridge.set_status("No active video sequence or frame to capture. Open a sequence first.");
                }
            }
        }
    }

    /// Send the active EffectCraft composition to the FilmCraft video timeline as a live Dynamic Link.
    pub fn send_active_comp_to_video(&mut self) {
        let comp_id = self.effect_app.session.state.active_comp
            .or_else(|| self.effect_app.session.project.comps().next().map(|(id, _)| *id));

        let comp_id = match comp_id {
            Some(id) => id,
            None => {
                self.bridge.set_status("No composition found in EffectCraft. Create a composition first.");
                return;
            }
        };

        let comp = match self.effect_app.session.project.comp(comp_id) {
            Some(c) => c.clone(),
            None => return,
        };

        let comp_name = self.effect_app.session.project.item(comp_id)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| format!("Comp_{}", comp_id.0));

        let tick = self.effect_app.session.state.times.get(&comp_id).copied().unwrap_or(effectcraft_time::Tick(0));
        let (w, h, rgba) = match self.effect_app.session.render_rgba8_alpha(comp_id, tick, 0, true) {
            Ok(res) => res,
            Err(e) => {
                self.bridge.set_status(format!("Render error from EffectCraft: {e}"));
                return;
            }
        };

        let cache_file = self.dynamic_cache_dir().join(format!("comp_{}_live.png", comp_id.0));
        let mut png_bytes = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        if let Err(e) = image::ImageEncoder::write_image(
            encoder,
            &rgba,
            w,
            h,
            image::ExtendedColorType::Rgba8,
        ) {
            self.bridge.set_status(format!("Error encoding composition frame: {e}"));
            return;
        }

        if let Err(e) = std::fs::write(&cache_file, &png_bytes) {
            self.bridge.set_status(format!("Error writing comp cache file: {e}"));
            return;
        }

        let path_str = cache_file.to_string_lossy().to_string();

        let existing = self.bridge.get_linked_comp_by_comp_id(comp_id.0);
        let is_on_timeline = existing.as_ref().is_some_and(|l| l.film_item_id.is_some() && l.film_clip_id.is_some());

        if is_on_timeline {
            // Already placed on timeline! Dynamic refresh
            let _ = self.film_app.session.execute("media.refreshChanged", serde_json::json!({}));
            if let Some(mut linked) = existing {
                linked.file_path = cache_file;
                linked.last_sync_time = std::time::Instant::now();
                self.bridge.register_linked_comp(linked);
            }
            self.bridge.set_status(format!("⚡ Dynamic Link: '[VFX] {comp_name}' updated on Video Timeline"));
        } else {
            // Import cache proxy frame into FilmCraft
            let _ = self.film_app.session.execute("file.import", serde_json::json!({
                "paths": [path_str.clone()]
            }));

            // Find the imported item in FilmCraft
            let film_item_id = self.film_app.session.project.items.iter().find_map(|(&id, item)| {
                match &item.kind {
                    filmcraft_project::ItemKind::Media(m) => match &m.media {
                        filmcraft_project::MediaRef::File { path } if path == &path_str => Some(id),
                        _ => None,
                    },
                    _ => None,
                }
            });

            let mut clip_id_opt = None;
            if let Some(src_item) = film_item_id {
                let dur_secs = comp.duration.seconds().max(1.0);
                let dur = filmcraft_time::Tick::from_seconds_f64(dur_secs);
                let name_tag = comp_name.clone();
                let t_playhead = self.film_app.session.playhead();

                let _ = self.film_app.session.edit("Direct Link: Add Comp to Timeline", |pr, st| {
                    let seq_id = st.active_sequence.ok_or(filmcraft_engine::EngineError::NoSequence)?;
                    let (_w, _h, rate) = {
                        let q = pr.sequence(seq_id).ok_or(filmcraft_engine::EngineError::NoSequence)?;
                        (q.settings.width, q.settings.height, q.settings.frame_rate)
                    };
                    let t = t_playhead;
                    let mut ti = pr.make_track_item(src_item, filmcraft_project::TrackKind::Video, t, filmcraft_time::TimeRange::new(filmcraft_time::Tick::ZERO, dur), rate)
                        .ok_or_else(|| filmcraft_engine::EngineError::Other("Failed to make track item".into()))?;
                    ti.name = format!("✨ Comp: {name_tag}");
                    ti.label = filmcraft_project::Label::Violet;
                    let clip_id = ti.id;

                    let new_track_id = filmcraft_project::TrackId(pr.alloc_id());
                    let q = pr.sequence_mut(seq_id).ok_or(filmcraft_engine::EngineError::NoSequence)?;
                    if q.video_tracks.is_empty() {
                        q.video_tracks.push(filmcraft_project::Track::new(new_track_id, filmcraft_project::TrackKind::Video, "Video 1".into()));
                    }
                    let top_track = q.video_tracks.len() - 1;
                    q.video_tracks[top_track].items.push(ti);
                    q.video_tracks[top_track].sort();
                    st.selection = vec![clip_id];
                    clip_id_opt = Some(clip_id);
                    Ok(())
                });
            }

            self.bridge.register_linked_comp(LinkedComposition {
                comp_id: comp_id.0,
                comp_name: comp_name.clone(),
                file_path: cache_file,
                film_item_id: film_item_id.map(|i| i.0),
                film_clip_id: clip_id_opt.map(|c| c.0),
                width: w,
                height: h,
                duration_secs: comp.duration.seconds(),
                last_sync_time: std::time::Instant::now(),
            });
            self.bridge.set_status(format!("⚡ Live Dynamic Link: '[VFX] {comp_name}' placed on Video Timeline"));
        }

        self.workspace = StudioWorkspace::Video;
    }

    /// Dynamically sync the active composition from EffectCraft into FilmCraft without reloading.
    pub fn sync_comp_to_video(&mut self) {
        let comp_id = self.effect_app.session.state.active_comp
            .or_else(|| self.effect_app.session.project.comps().next().map(|(id, _)| *id));

        if let Some(cid) = comp_id {
            let cache_file = self.dynamic_cache_dir().join(format!("comp_{}_live.png", cid.0));
            let tick = self.effect_app.session.state.times.get(&cid).copied().unwrap_or(effectcraft_time::Tick(0));
            if let Ok((w, h, rgba)) = self.effect_app.session.render_rgba8_alpha(cid, tick, 0, true) {
                let mut png_bytes = Vec::new();
                let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
                if image::ImageEncoder::write_image(encoder, &rgba, w, h, image::ExtendedColorType::Rgba8).is_ok() {
                    let _ = std::fs::write(&cache_file, &png_bytes);
                    let _ = self.film_app.session.execute("media.refreshChanged", serde_json::json!({}));
                    if let Some(mut linked) = self.bridge.get_linked_comp_by_comp_id(cid.0) {
                        linked.file_path = cache_file;
                        linked.last_sync_time = std::time::Instant::now();
                        self.bridge.register_linked_comp(linked);
                    }
                    self.bridge.set_status(format!("⚡ Synced composition '{}' to Video Timeline", cid.0));
                }
            }
        }
    }

    /// Find if any linked composition is currently selected or under the playhead in FilmCraft.
    pub fn find_active_linked_comp_in_video(&self) -> Option<LinkedComposition> {
        // 1. Check selected clips
        for sel in &self.film_app.session.state.selection {
            if let Some(linked) = self.bridge.get_linked_comp_by_clip_id(sel.0) {
                return Some(linked);
            }
        }

        // 2. Check playhead overlap in active sequence
        if let Some(seq_id) = self.film_app.session.state.active_sequence {
            if let Some(seq) = self.film_app.session.project.sequence(seq_id) {
                let t = self.film_app.session.state.playheads.get(&seq_id).copied().unwrap_or(filmcraft_time::Tick::ZERO);
                for tr in &seq.video_tracks {
                    if let Some(item) = tr.item_at(t) {
                        if let Some(linked) = self.bridge.get_linked_comp_by_clip_id(item.id.0) {
                            return Some(linked);
                        }
                        if let Some(linked) = self.bridge.get_linked_comp_by_item_id(item.item.0) {
                            return Some(linked);
                        }
                    }
                }
            }
        }
        None
    }

    /// Jump directly from FilmCraft timeline into EffectCraft to edit the composition.
    pub fn open_comp_in_compositor(&mut self, comp_id: u64) {
        self.workspace = StudioWorkspace::Compositing;
        let cid = effectcraft_project::ItemId(comp_id);
        self.effect_app.session.open_comp(cid);
        let name = self.effect_app.session.project.item(cid)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| format!("Comp_{comp_id}"));
        self.bridge.set_status(format!("Opened Dynamic Link: '[VFX] {name}' in EffectCraft"));
    }

    /// Send the active clip, selection, or playhead frame from FilmCraft video editor into EffectCraft compositor.
    pub fn send_video_clip_to_compositor(&mut self) {
        let mut target_info: Option<(String, Option<String>, Option<filmcraft_project::ClipId>, f64)> = None;

        if let Some(seq_id) = self.film_app.session.state.active_sequence {
            if let Some(seq) = self.film_app.session.project.sequence(seq_id) {
                let t = self.film_app.session.state.playheads.get(&seq_id).copied().unwrap_or(filmcraft_time::Tick::ZERO);

                // 1. Check selected timeline clip
                for sel_id in &self.film_app.session.state.selection {
                    for tr in &seq.video_tracks {
                        if let Some(item) = tr.items.iter().find(|i| i.id == *sel_id) {
                            let path = self.film_app.session.project.item(item.item).and_then(|pi| {
                                match &pi.kind {
                                    filmcraft_project::ItemKind::Media(m) => match &m.media {
                                        filmcraft_project::MediaRef::File { path } => Some(path.clone()),
                                        _ => None,
                                    },
                                    _ => None,
                                }
                            });
                            let dur = item.duration.seconds().max(1.0);
                            target_info = Some((item.name.clone(), path, Some(item.id), dur));
                            break;
                        }
                    }
                    if target_info.is_some() { break; }
                }

                // 2. Check clip under playhead
                if target_info.is_none() {
                    for tr in &seq.video_tracks {
                        if let Some(item) = tr.item_at(t) {
                            let path = self.film_app.session.project.item(item.item).and_then(|pi| {
                                match &pi.kind {
                                    filmcraft_project::ItemKind::Media(m) => match &m.media {
                                        filmcraft_project::MediaRef::File { path } => Some(path.clone()),
                                        _ => None,
                                    },
                                    _ => None,
                                }
                            });
                            let dur = item.duration.seconds().max(1.0);
                            target_info = Some((item.name.clone(), path, Some(item.id), dur));
                            break;
                        }
                    }
                }
            }
        }

        // 3. Check project selection
        if target_info.is_none() {
            for &item_id in &self.film_app.session.state.project_selection {
                if let Some(pi) = self.film_app.session.project.item(item_id) {
                    let path = match &pi.kind {
                        filmcraft_project::ItemKind::Media(m) => match &m.media {
                            filmcraft_project::MediaRef::File { path } => Some(path.clone()),
                            _ => None,
                        },
                        _ => None,
                    };
                    target_info = Some((pi.name.clone(), path, None, 18.0));
                    break;
                }
            }
        }

        let timestamp = chrono::Local::now().format("%H%M%S").to_string();
        let (clip_name, media_path, clip_id_opt, clip_dur) = match target_info {
            Some(info) => info,
            None => ("Timeline Shot".to_string(), None, None, 18.0),
        };

        // Determine footage file to import into EffectCraft
        let footage_path = match media_path {
            Some(p) if std::path::Path::new(&p).exists() => p,
            _ => {
                // If it's a generator (e.g. Ocean_Sunset) or current playhead, render the frame
                if let Some(img) = self.film_app.session.render_program(1.0) {
                    let width = img.w as u32;
                    let height = img.h as u32;
                    let mut rgba_u8 = Vec::with_capacity(width as usize * height as usize * 4);
                    for chunk in img.px.chunks_exact(4) {
                        let r = (chunk[0].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                        let g = (chunk[1].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                        let b = (chunk[2].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                        let a = (chunk[3].clamp(0.0, 1.0) * 255.0) as u8;
                        rgba_u8.extend_from_slice(&[r, g, b, a]);
                    }
                    let proxy_path = self.dynamic_cache_dir().join(format!("vfx_clip_{timestamp}.png"));
                    let mut png_bytes = Vec::new();
                    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
                    let _ = image::ImageEncoder::write_image(encoder, &rgba_u8, width, height, image::ExtendedColorType::Rgba8);
                    let _ = std::fs::write(&proxy_path, &png_bytes);
                    proxy_path.to_string_lossy().to_string()
                } else {
                    self.bridge.set_status("Cannot capture footage from video editor.");
                    return;
                }
            }
        };

        // In EffectCraft: create a composition for this clip and open it
        let (w, h, fps) = if let Some(seq_id) = self.film_app.session.state.active_sequence {
            if let Some(seq) = self.film_app.session.project.sequence(seq_id) {
                (seq.settings.width, seq.settings.height, seq.settings.frame_rate.as_f64())
            } else {
                (1920, 1080, 30.0)
            }
        } else {
            (1920, 1080, 30.0)
        };
        let comp_title = format!("Comp: {clip_name}");
        let comp_id = match self.effect_app.session.execute("comp.new", serde_json::json!({
            "name": comp_title,
            "width": w,
            "height": h,
            "duration": clip_dur,
            "frameRate": fps,
        })) {
            Ok(val) => {
                let cid = val.get("comp").and_then(|v| v.as_u64()).map(effectcraft_project::ItemId)
                    .unwrap_or(effectcraft_project::ItemId(1));
                self.effect_app.session.open_comp(cid);
                cid
            }
            Err(_) => self.effect_app.session.state.active_comp.unwrap_or(effectcraft_project::ItemId(1)),
        };

        // Import footage into EffectCraft with addToComp: true
        let mut import_res = self.effect_app.session.execute("file.import", serde_json::json!({
            "paths": [footage_path.clone()],
            "addToComp": true
        }));

        // If the direct video import returned no items (e.g. generator or codec fallback),
        // fallback to rendering the exact high-res frame buffer from the video editor and import it!
        let has_items = match &import_res {
            Ok(val) => {
                let items_len = val.get("items").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
                let comps_len = val.get("comps").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
                items_len > 0 || comps_len > 0
            }
            Err(_) => false,
        };

        let _final_path = if !has_items {
            if let Some(img) = self.film_app.session.render_program(1.0) {
                let width = img.w as u32;
                let height = img.h as u32;
                let mut rgba_u8 = Vec::with_capacity(width as usize * height as usize * 4);
                for chunk in img.px.chunks_exact(4) {
                    let r = (chunk[0].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                    let g = (chunk[1].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                    let b = (chunk[2].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                    let a = (chunk[3].clamp(0.0, 1.0) * 255.0) as u8;
                    rgba_u8.extend_from_slice(&[r, g, b, a]);
                }
                let fallback_path = self.dynamic_cache_dir().join(format!("vfx_clip_rendered_{timestamp}.png"));
                let mut png_bytes = Vec::new();
                let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
                let _ = image::ImageEncoder::write_image(encoder, &rgba_u8, width, height, image::ExtendedColorType::Rgba8);
                let _ = std::fs::write(&fallback_path, &png_bytes);
                let rendered_path = fallback_path.to_string_lossy().to_string();
                import_res = self.effect_app.session.execute("file.import", serde_json::json!({
                    "paths": [rendered_path.clone()],
                    "addToComp": true
                }));
                rendered_path
            } else {
                footage_path
            }
        } else {
            footage_path
        };

        // Ensure every imported item is placed as a layer in the composition
        if let Ok(ref val) = import_res {
            if let Some(items) = val.get("items").and_then(|v| v.as_array()) {
                for item_val in items {
                    if let Some(item_id) = item_val.as_u64() {
                        let already_present = self.effect_app.session.project.comp(comp_id)
                            .map_or(false, |c| c.layers.iter().any(|l| match l.source {
                                effectcraft_project::LayerSource::Footage { item }
                                | effectcraft_project::LayerSource::Comp { item }
                                | effectcraft_project::LayerSource::Solid { item } => item.0 == item_id,
                                _ => false,
                            }));
                        if !already_present {
                            let _ = self.effect_app.session.execute("layer.addItem", serde_json::json!({
                                "comp": comp_id.0,
                                "item": item_id,
                            }));
                        }
                    }
                }
            }
        }

        let comp_cache_file = self.dynamic_cache_dir().join(format!("comp_{}_live.png", comp_id.0));
        let tick = self.effect_app.session.state.times.get(&comp_id).copied().unwrap_or(effectcraft_time::Tick(0));
        if let Ok((w_ren, h_ren, rgba)) = self.effect_app.session.render_rgba8_alpha(comp_id, tick, 0, true) {
            let mut png_bytes = Vec::new();
            let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
            if image::ImageEncoder::write_image(encoder, &rgba, w_ren, h_ren, image::ExtendedColorType::Rgba8).is_ok() {
                let _ = std::fs::write(&comp_cache_file, &png_bytes);
            }
        }

        // Register Dynamic Link (file_path is the comp cache proxy, NEVER the source video file!)
        let linked = LinkedComposition {
            comp_id: comp_id.0,
            comp_name: format!("Comp: {clip_name}"),
            film_item_id: None,
            film_clip_id: clip_id_opt.map(|c| c.0),
            file_path: comp_cache_file,
            width: w,
            height: h,
            duration_secs: clip_dur,
            last_sync_time: std::time::Instant::now(),
        };
        self.bridge.register_linked_comp(linked);

        // Switch to Compositing workspace
        self.workspace = StudioWorkspace::Compositing;
        self.bridge.set_status(format!("✨ Sent '{clip_name}' to EffectCraft composition!"));
    }

    /// Render the top Studio Navigation, Dynamic Link, and Workflow ribbon.
    fn render_studio_header(&mut self, ui: &mut egui::Ui) {
        let ribbon_frame = egui::Frame::NONE
            .fill(Color32::from_rgb(16, 18, 22))
            .inner_margin(egui::Margin::symmetric(12, 7))
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(34, 38, 46)));

        ribbon_frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                // Suite Brand
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚡ CraftStudio").strong().color(Color32::from_rgb(240, 242, 248)));
                    ui.label(RichText::new("PRO SUITE").size(9.0).color(Color32::from_rgb(120, 130, 150)));
                });

                ui.separator();

                // Workspace Mode Selector
                let btn = |ui: &mut egui::Ui, label: &str, active: bool| {
                    let fill = if active { Color32::from_rgb(45, 95, 195) } else { Color32::from_rgb(26, 30, 36) };
                    let text_color = if active { Color32::WHITE } else { Color32::from_rgb(190, 200, 215) };
                    ui.add(
                        egui::Button::new(RichText::new(label).color(text_color).strong().size(12.0))
                            .fill(fill)
                            .corner_radius(4.0)
                    )
                };

                if btn(ui, "🎨 Photo", self.workspace == StudioWorkspace::Photo).clicked() {
                    self.workspace = StudioWorkspace::Photo;
                }
                if btn(ui, "🎬 Video", self.workspace == StudioWorkspace::Video).clicked() {
                    if self.workspace == StudioWorkspace::Compositing {
                        self.sync_comp_to_video();
                    }
                    self.workspace = StudioWorkspace::Video;
                }
                if btn(ui, "✨ Compositing", self.workspace == StudioWorkspace::Compositing).clicked() {
                    self.workspace = StudioWorkspace::Compositing;
                    if let Some(cid) = self.effect_app.session.state.active_comp {
                        self.effect_app.session.open_comp(cid);
                    }
                }
                if btn(ui, "🔀 Photo+Video", self.workspace == StudioWorkspace::SplitPhotoVideo).clicked() {
                    self.workspace = StudioWorkspace::SplitPhotoVideo;
                }
                if btn(ui, "🔀 Video+VFX", self.workspace == StudioWorkspace::SplitVideoVfx).clicked() {
                    self.workspace = StudioWorkspace::SplitVideoVfx;
                }

                ui.separator();

                // --- Live Dynamic Link Context Actions ---
                let active_linked_in_video = self.find_active_linked_comp_in_video();

                if self.workspace == StudioWorkspace::Compositing {
                    // In Compositor: Button to send/sync to video timeline
                    if ui.button(RichText::new("✨ Send Comp to Video").color(Color32::from_rgb(140, 235, 170)).strong())
                        .on_hover_text("Send this composition directly to FilmCraft Video Timeline as a live dynamic link (Ctrl+E)")
                        .clicked()
                    {
                        self.send_active_comp_to_video();
                    }

                    if ui.button(RichText::new("🎬 Return to Video").color(Color32::from_rgb(170, 190, 240)))
                        .on_hover_text("Sync changes and switch back to FilmCraft Video Editor")
                        .clicked()
                    {
                        self.send_active_comp_to_video();
                    }
                } else if let Some(linked) = active_linked_in_video {
                    // In Video Editor: A linked comp is selected or under playhead!
                    let comp_title = format!("✨ Edit Comp in VFX: {}", linked.comp_name);
                    if ui.add(
                        egui::Button::new(RichText::new(comp_title).color(Color32::WHITE).strong())
                            .fill(Color32::from_rgb(110, 45, 185))
                            .corner_radius(4.0)
                    )
                    .on_hover_text("Open and edit this composition in EffectCraft (Double-click / Ctrl+E)")
                    .clicked()
                    {
                        self.open_comp_in_compositor(linked.comp_id);
                    }
                } else if self.workspace == StudioWorkspace::Video || self.workspace == StudioWorkspace::SplitVideoVfx {
                    // Send selected video clip or active playhead frame to EffectCraft compositor
                    if ui.add(
                        egui::Button::new(RichText::new("✨ Send Clip to Compositor").color(Color32::from_rgb(255, 230, 140)).strong())
                            .fill(Color32::from_rgb(95, 45, 175))
                            .corner_radius(4.0)
                    )
                    .on_hover_text("Send the selected clip or active playhead frame into EffectCraft as a composition (Ctrl+E)")
                    .clicked()
                    {
                        self.send_video_clip_to_compositor();
                    }

                    if !self.bridge.all_linked_comps().is_empty() {
                        if ui.button(RichText::new("⚡ Sync Comps").color(Color32::from_rgb(150, 220, 200)))
                            .on_hover_text("Re-render all dynamic link compositions from EffectCraft into Video Timeline")
                            .clicked()
                        {
                            self.sync_comp_to_video();
                        }
                    }
                }

                ui.separator();

                // --- Frame Capture & Clipboard History ---
                let history_count = self.bridge.frame_history.lock().map(|h| h.len()).unwrap_or(0);

                if ui.button(RichText::new("📸 Capture Frame").color(Color32::from_rgb(130, 220, 160)).strong())
                    .on_hover_text("Capture current frame into OS Clipboard and Studio History (Ctrl+Shift+C)")
                    .clicked()
                {
                    self.capture_frame(false);
                }

                if ui.button(RichText::new("🎨 Paint Frame").color(Color32::from_rgb(100, 200, 255)))
                    .on_hover_text("Capture current frame and open immediately in PhotoCraft for retouching")
                    .clicked()
                {
                    self.capture_frame(true);
                }

                let history_label = if history_count > 0 {
                    format!("📋 History ({history_count})")
                } else {
                    "📋 History".to_string()
                };

                let hist_fill = if self.show_history_drawer { Color32::from_rgb(50, 60, 75) } else { Color32::from_rgb(26, 30, 36) };
                if ui.add(
                    egui::Button::new(RichText::new(history_label).color(Color32::from_rgb(220, 200, 140)).strong())
                        .fill(hist_fill)
                        .corner_radius(4.0)
                )
                .on_hover_text("Open or hide the Studio Frame & Clipboard History drawer")
                .clicked()
                {
                    self.show_history_drawer = !self.show_history_drawer;
                }

                let bin_fill = if self.show_media_drawer { Color32::from_rgb(50, 60, 75) } else { Color32::from_rgb(26, 30, 36) };
                if ui.add(
                    egui::Button::new(RichText::new("📂 Shared Bin").color(Color32::from_rgb(200, 180, 240)).strong())
                        .fill(bin_fill)
                        .corner_radius(4.0)
                )
                .on_hover_text("Open or hide the shared project media bin")
                .clicked()
                {
                    self.show_media_drawer = !self.show_media_drawer;
                }

                // Status message & Direct Link badge on right
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("● Dynamic Link Active").color(Color32::from_rgb(60, 215, 115)).size(11.0));
                    if let Some(msg) = self.bridge.current_status() {
                        ui.label(RichText::new(msg).color(Color32::from_rgb(175, 185, 205)).size(11.0));
                    }
                });
            });
        });
    }

    /// Render the Capture & Clipboard History Shelf with live thumbnails and PhotoCraft dispatch.
    fn render_frame_history_drawer(&mut self, ui: &mut egui::Ui) {
        let frame = egui::Frame::NONE
            .fill(Color32::from_rgb(20, 23, 28))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(42, 48, 58)));

        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("📸 Studio Frame & Clipboard History").size(13.0).strong().color(Color32::from_rgb(230, 235, 245)));
                ui.label(RichText::new("Frames auto-copied to OS Clipboard • Click 'Open in PhotoCraft' to retouch").color(Color32::GRAY).size(11.0));

                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("🗑️ Clear History").clicked() {
                        self.bridge.clear_history();
                    }
                    if ui.small_button("+ Capture Current Frame").clicked() {
                        self.capture_frame(false);
                    }
                });
            });

            ui.add_space(4.0);

            let history = {
                if let Ok(lock) = self.bridge.frame_history.lock() {
                    lock.clone()
                } else {
                    Vec::new()
                }
            };

            if history.is_empty() {
                ui.label(RichText::new("No captured frames yet. Click '📸 Capture Frame' (Ctrl+Shift+C) at any time to take a snapshot into clipboard and history.").italics().color(Color32::DARK_GRAY));
            } else {
                egui::ScrollArea::horizontal()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let mut action_open_photo: Option<HistoryFrame> = None;
                            let mut action_copy: Option<(Vec<u8>, u32, u32)> = None;
                            let mut action_save: Option<(Vec<u8>, String)> = None;
                            let mut action_delete: Option<String> = None;

                            for item in history.iter().rev() {
                                egui::Frame::NONE
                                    .fill(Color32::from_rgb(26, 30, 38))
                                    .corner_radius(4.0)
                                    .inner_margin(egui::Margin::same(6))
                                    .stroke(egui::Stroke::new(1.0, Color32::from_rgb(46, 52, 64)))
                                    .show(ui, |ui| {
                                        ui.vertical(|ui| {
                                            // Live Texture thumbnail
                                            let tex_id = format!("thumb-{}", item.id);
                                            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                                [item.thumb_width as usize, item.thumb_height as usize],
                                                &item.thumb_rgba,
                                            );
                                            let texture = ui.ctx().load_texture(tex_id, color_image, egui::TextureOptions::LINEAR);
                                            let aspect = item.width as f32 / item.height.max(1) as f32;
                                            let thumb_h = 55.0;
                                            let thumb_w = (thumb_h * aspect).clamp(40.0, 110.0);
                                            ui.image((texture.id(), Vec2::new(thumb_w, thumb_h)));

                                            ui.add_space(2.0);
                                            ui.label(RichText::new(&item.name).size(10.0).strong().color(Color32::WHITE));
                                            ui.label(RichText::new(&item.source_label).size(9.0).color(Color32::from_rgb(160, 175, 195)));
                                            ui.label(RichText::new(format!("{}x{} • {}", item.width, item.height, item.time_formatted)).size(8.5).color(Color32::GRAY));

                                            ui.add_space(2.0);
                                            ui.horizontal(|ui| {
                                                if ui.add(
                                                    egui::Button::new(RichText::new("🎨 Photo").size(10.0).strong().color(Color32::WHITE))
                                                        .fill(Color32::from_rgb(35, 85, 180))
                                                        .corner_radius(3.0)
                                                )
                                                .on_hover_text("Open this frame directly as an editable document in PhotoCraft")
                                                .clicked()
                                                {
                                                    action_open_photo = Some(item.clone());
                                                }

                                                if ui.add(
                                                    egui::Button::new(RichText::new("📋 Copy").size(10.0).color(Color32::from_rgb(200, 210, 225)))
                                                        .fill(Color32::from_rgb(40, 46, 56))
                                                        .corner_radius(3.0)
                                                )
                                                .on_hover_text("Copy RGBA image to OS clipboard")
                                                .clicked()
                                                {
                                                    action_copy = Some((item.rgba.clone(), item.width, item.height));
                                                }

                                                if ui.add(
                                                    egui::Button::new(RichText::new("💾").size(10.0))
                                                        .fill(Color32::from_rgb(40, 46, 56))
                                                        .corner_radius(3.0)
                                                )
                                                .on_hover_text("Save PNG to file")
                                                .clicked()
                                                {
                                                    action_save = Some((item.png_bytes.clone(), item.name.clone()));
                                                }

                                                if ui.add(
                                                    egui::Button::new(RichText::new("🗑️").size(10.0))
                                                        .fill(Color32::from_rgb(40, 46, 56))
                                                        .corner_radius(3.0)
                                                )
                                                .on_hover_text("Delete from history")
                                                .clicked()
                                                {
                                                    action_delete = Some(item.id.clone());
                                                }
                                            });
                                        });
                                    });
                            }

                            if let Some(frame) = action_open_photo {
                                self.bridge.queue_history_frame_to_photo(&frame);
                            }
                            if let Some((rgba, w, h)) = action_copy {
                                self.bridge.copy_to_system_clipboard(&rgba, w, h);
                                self.bridge.set_status("Copied image to OS clipboard");
                            }
                            if let Some((png, name)) = action_save {
                                if let Some(path) = rfd::FileDialog::new().set_file_name(&name).add_filter("PNG Image", &["png"]).save_file() {
                                    let _ = std::fs::write(path, png);
                                    self.bridge.set_status("Saved frame to file");
                                }
                            }
                            if let Some(id) = action_delete {
                                self.bridge.delete_history_frame(&id);
                            }
                        });
                    });
            }
        });
    }

    /// Render the shared project asset drawer.
    fn render_media_drawer(&mut self, ui: &mut egui::Ui) {
        let frame = egui::Frame::NONE
            .fill(Color32::from_rgb(22, 24, 28))
            .inner_margin(egui::Margin::same(8))
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(45, 50, 60)));

        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Shared Project Assets").size(14.0).strong());
                if ui.button("+ Import File...").clicked() {
                    if let Some(paths) = rfd::FileDialog::new().pick_files() {
                        for path in paths {
                            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                            let kind = match ext.as_str() {
                                "png" | "jpg" | "jpeg" | "tiff" | "webp" => AssetKind::Image,
                                "mp4" | "mov" | "mkv" | "avi" => AssetKind::Video,
                                "wav" | "mp3" | "aac" | "flac" => AssetKind::Audio,
                                "psd" | "psb" => AssetKind::Psd,
                                _ => AssetKind::Image,
                            };
                            self.bridge.register_asset(name, path, kind);
                        }
                    }
                }
            });

            ui.add_space(4.0);

            let assets = {
                if let Ok(lock) = self.bridge.shared_assets.lock() {
                    lock.clone()
                } else {
                    Vec::new()
                }
            };

            if assets.is_empty() {
                ui.label(RichText::new("No files in shared bin yet. Click '+ Import File...' to add assets shared across Photo, Video and VFX.").italics().color(Color32::GRAY));
            } else {
                ui.horizontal_wrapped(|ui| {
                    for asset in assets {
                        let icon = match asset.kind {
                            AssetKind::Image => "🖼️",
                            AssetKind::Video => "🎞️",
                            AssetKind::Audio => "🎵",
                            AssetKind::Psd => "🎨",
                            AssetKind::Composition => "✨",
                        };

                        ui.group(|ui| {
                            ui.label(format!("{icon} {}", asset.name));
                            ui.horizontal(|ui| {
                                if ui.small_button("Photo").clicked() {
                                    if let Ok(bytes) = std::fs::read(&asset.path) {
                                        let _ = self.photo_app.open_bytes(&asset.name, &bytes);
                                        self.workspace = StudioWorkspace::Photo;
                                    }
                                }
                                if ui.small_button("Video").clicked() {
                                    let _ = self.film_app.session.execute("file.import", serde_json::json!({
                                        "paths": [asset.path.to_string_lossy()]
                                    }));
                                    self.workspace = StudioWorkspace::Video;
                                }
                                if ui.small_button("VFX").clicked() {
                                    let _ = self.effect_app.session.execute("file.import", serde_json::json!({
                                        "paths": [asset.path.to_string_lossy()]
                                    }));
                                    self.workspace = StudioWorkspace::Compositing;
                                }
                            });
                        });
                    }
                });
            }
        });
    }
}

impl eframe::App for CraftStudioApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        match self.workspace {
            StudioWorkspace::Photo => {
                <PhotocraftApp as eframe::App>::raw_input_hook(&mut self.photo_app, ctx, raw_input);
            }
            StudioWorkspace::Video | StudioWorkspace::SplitPhotoVideo | StudioWorkspace::SplitVideoVfx => {
                <FilmcraftApp as eframe::App>::raw_input_hook(&mut self.film_app, ctx, raw_input);
            }
            StudioWorkspace::Compositing => {
                <EffectcraftApp as eframe::App>::raw_input_hook(&mut self.effect_app, ctx, raw_input);
            }
        }
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.process_interconnect_pipeline();

        // Drag-and-drop file imports directly from desktop/explorer
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() {
            let mut paths = Vec::new();
            for f in dropped {
                let p = f.path();
                if p.exists() {
                    paths.push(p.to_string_lossy().to_string());
                }
            }
            if !paths.is_empty() {
                match self.workspace {
                    StudioWorkspace::Video | StudioWorkspace::SplitPhotoVideo | StudioWorkspace::SplitVideoVfx => {
                        let _ = self.film_app.session.execute("file.import", serde_json::json!({ "paths": paths.clone() }));
                        self.bridge.set_status(format!("Imported {} file(s) into Video Project", paths.len()));
                    }
                    StudioWorkspace::Compositing => {
                        let _ = self.effect_app.session.execute("file.import", serde_json::json!({ "paths": paths.clone(), "addToComp": true }));
                        self.bridge.set_status(format!("Imported {} file(s) into Compositor", paths.len()));
                    }
                    StudioWorkspace::Photo => {
                        let count = self.photo_app.open_paths(&paths);
                        self.bridge.set_status(format!("Opened {count} file(s) in PhotoCraft"));
                    }
                }
            }
        }

        // Global hotkeys across Studio
        if ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::C)) {
            self.capture_frame(false);
        }

        // Ctrl+E / Cmd+E: Quick roundtrip between Video and Compositor
        if ctx.input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::E)) {
            if self.workspace == StudioWorkspace::Compositing {
                self.sync_comp_to_video();
                self.workspace = StudioWorkspace::Video;
            } else if let Some(linked) = self.find_active_linked_comp_in_video() {
                self.open_comp_in_compositor(linked.comp_id);
            } else if self.workspace == StudioWorkspace::Video || self.workspace == StudioWorkspace::SplitVideoVfx {
                self.send_video_clip_to_compositor();
            }
        }

        // Detect double-click on timeline clip: if a linked composition was double clicked
        if self.workspace == StudioWorkspace::Video && ctx.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)) {
            if let Some(linked) = self.find_active_linked_comp_in_video() {
                self.open_comp_in_compositor(linked.comp_id);
            }
        }

        // Run logic update loops for active sub-engines
        match self.workspace {
            StudioWorkspace::Photo => {
                <PhotocraftApp as eframe::App>::logic(&mut self.photo_app, ctx, frame);
            }
            StudioWorkspace::Video => {
                <FilmcraftApp as eframe::App>::logic(&mut self.film_app, ctx, frame);
            }
            StudioWorkspace::Compositing => {
                <EffectcraftApp as eframe::App>::logic(&mut self.effect_app, ctx, frame);
            }
            StudioWorkspace::SplitPhotoVideo => {
                <PhotocraftApp as eframe::App>::logic(&mut self.photo_app, ctx, frame);
                <FilmcraftApp as eframe::App>::logic(&mut self.film_app, ctx, frame);
            }
            StudioWorkspace::SplitVideoVfx => {
                <FilmcraftApp as eframe::App>::logic(&mut self.film_app, ctx, frame);
                <EffectcraftApp as eframe::App>::logic(&mut self.effect_app, ctx, frame);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // 1. Top Ribbon
        self.render_studio_header(ui);

        // 2. Optional Shared Asset Drawer
        if self.show_media_drawer {
            self.render_media_drawer(ui);
            ui.separator();
        }

        // 3. Optional Capture & Clipboard History Shelf
        if self.show_history_drawer {
            self.render_frame_history_drawer(ui);
            ui.separator();
        }

        // 4. Workspace Rendering Area - Bounded properly so sub-engines never cover studio chrome
        let avail = ui.available_size();
        ui.allocate_ui_with_layout(
            avail,
            Layout::top_down(egui::Align::Min),
            |ui| {
                match self.workspace {
                    StudioWorkspace::Photo => {
                        <PhotocraftApp as eframe::App>::ui(&mut self.photo_app, ui, frame);
                    }
                    StudioWorkspace::Video => {
                        <FilmcraftApp as eframe::App>::ui(&mut self.film_app, ui, frame);
                    }
                    StudioWorkspace::Compositing => {
                        <EffectcraftApp as eframe::App>::ui(&mut self.effect_app, ui, frame);
                    }
                    StudioWorkspace::SplitPhotoVideo => {
                        let avail = ui.available_size();
                        let top_h = (avail.y * self.split_ratio).clamp(180.0, avail.y - 180.0);

                        ui.vertical(|ui| {
                            ui.allocate_ui_with_layout(
                                Vec2::new(avail.x, top_h),
                                Layout::top_down(egui::Align::Min),
                                |ui| {
                                    <PhotocraftApp as eframe::App>::ui(&mut self.photo_app, ui, frame);
                                },
                            );

                            // Split resize handle
                            let resp = ui.allocate_response(Vec2::new(avail.x, 6.0), Sense::drag());
                            if resp.dragged() {
                                self.split_ratio = ((self.split_ratio * avail.y + resp.drag_delta().y) / avail.y).clamp(0.2, 0.8);
                            }

                            ui.allocate_ui_with_layout(
                                Vec2::new(avail.x, ui.available_height()),
                                Layout::top_down(egui::Align::Min),
                                |ui| {
                                    <FilmcraftApp as eframe::App>::ui(&mut self.film_app, ui, frame);
                                },
                            );
                        });
                    }
                    StudioWorkspace::SplitVideoVfx => {
                        let avail = ui.available_size();
                        let left_w = (avail.x * self.split_ratio).clamp(300.0, avail.x - 300.0);

                        ui.horizontal(|ui| {
                            ui.allocate_ui_with_layout(
                                Vec2::new(left_w, avail.y),
                                Layout::top_down(egui::Align::Min),
                                |ui| {
                                    <FilmcraftApp as eframe::App>::ui(&mut self.film_app, ui, frame);
                                },
                            );

                            let resp = ui.allocate_response(Vec2::new(6.0, avail.y), Sense::drag());
                            if resp.dragged() {
                                self.split_ratio = ((self.split_ratio * avail.x + resp.drag_delta().x) / avail.x).clamp(0.2, 0.8);
                            }

                            ui.allocate_ui_with_layout(
                                Vec2::new(ui.available_width(), avail.y),
                                Layout::top_down(egui::Align::Min),
                                |ui| {
                                    <EffectcraftApp as eframe::App>::ui(&mut self.effect_app, ui, frame);
                                },
                            );
                        });
                    }
                }
            },
        );
    }

    fn on_exit(&mut self) {
        <FilmcraftApp as eframe::App>::on_exit(&mut self.film_app);
        <EffectcraftApp as eframe::App>::on_exit(&mut self.effect_app);
    }
}
