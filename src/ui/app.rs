//! Application state, background work and the window frame.

use super::theme::{self, pal, pal_ctx, semibold, Icon};
use super::widgets;
use crate::actions;
use crate::catalog::{App as CatApp, Catalog};
use crate::github::{self, ApiError, AssetKind, Cached, Release};
use crate::installed::{self, Installed, RegEntry};
use crate::net;
use crate::settings::{Dirs, Settings};
use crate::{demo, version};
use eframe::egui::{
    self, vec2, Align, Align2, CornerRadius, Frame, Layout, Margin, Rect, RichText, Sense, Stroke,
    TextureHandle,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct Options {
    /// Built-in data only: no network, no registry, nothing written.
    pub demo: bool,
    pub screenshot: Option<PathBuf>,
    pub view: Option<String>,
    /// Force the light theme (for screenshots).
    pub light: bool,
    /// With `demo`: still load screenshots from GitHub.
    pub live_images: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    Discover,
    Installed,
    Updates,
    Settings,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum JobKind {
    Install,
    Update,
    Portable,
}

pub(super) struct Job {
    pub kind: JobKind,
    pub progress: Arc<AtomicU64>,
    pub total: u64,
    pub cancel: Arc<AtomicBool>,
}

pub(super) enum Img {
    Loading,
    Ready(TextureHandle),
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    Live,
    Saved,
    BuiltIn,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AppState {
    ComingSoon,
    NotInstalled,
    Installed,
    Update,
}

pub(super) enum Action {
    OpenPage(String),
    Install(String),
    Update(String),
    UpdateAll,
    Portable(String),
    Launch(String),
    AskUninstall(String),
    Url(String),
    Cancel(String),
    Refresh,
}

enum Msg {
    Catalog(Box<Catalog>, Source),
    Release {
        id: String,
        release: Option<Box<Release>>,
        error: Option<ApiError>,
    },
    ReleasesDone,
    Registry(Vec<RegEntry>),
    Image(String, Option<egui::ColorImage>),
    JobDone(String, JobKind, Result<PathBuf, String>),
}

struct Toast {
    text: String,
    error: bool,
    born: Instant,
}

struct Shot {
    path: PathBuf,
    frames: u32,
    requested: bool,
}

pub struct App {
    pub(super) dirs: Dirs,
    pub(super) demo: bool,
    net_images: bool,
    pub(super) settings: Settings,
    pub(super) catalog: Arc<Catalog>,
    pub(super) source: Source,
    pub(super) releases: HashMap<String, Release>,
    pub(super) api_error: Option<ApiError>,
    pub(super) checking: bool,
    registry: Vec<RegEntry>,
    pub(super) installed: HashMap<String, Installed>,
    last_scan: Instant,
    scanning: bool,
    pub(super) tab: Tab,
    pub(super) page: Option<String>,
    pub(super) search: String,
    focus_search: bool,
    pub(super) category: Option<String>,
    pub(super) jobs: HashMap<String, Job>,
    images: HashMap<String, Img>,
    icons: HashMap<String, TextureHandle>,
    pub(super) lightbox: Option<(String, usize)>,
    confirm_uninstall: Option<String>,
    pub(super) pending: Vec<Action>,
    pub(super) cache_size: Option<u64>,
    toasts: Vec<Toast>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub(super) logo: Option<TextureHandle>,
    shot: Option<Shot>,
}

/// Starts the window.
pub fn run(options: Options, icon: egui::IconData) -> eframe::Result<()> {
    let viewport = egui::ViewportBuilder::default()
        .with_title(if options.demo {
            "ForgeDock (demo)"
        } else {
            "ForgeDock"
        })
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([980.0, 620.0])
        .with_icon(icon.clone());
    let native = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "ForgeDock",
        native,
        Box::new(move |cc| Ok(Box::new(App::new(cc, options, icon)))),
    )
}

fn decode(bytes: &[u8], max_width: Option<u32>) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = match max_width {
        Some(w) if img.width() > w => {
            let h = (img.height() as f32 * w as f32 / img.width() as f32).round() as u32;
            img.resize_exact(w, h.max(1), image::imageops::FilterType::Triangle)
        }
        _ => img,
    };
    let rgba = img.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, &rgba))
}

/// FNV-1a, for cache file names.
fn hash_name(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, options: Options, icon: egui::IconData) -> Self {
        let ctx = &cc.egui_ctx;
        theme::install(ctx);
        let dirs = if options.demo {
            Dirs::demo()
        } else {
            Dirs::real()
        };
        let mut settings = if options.demo {
            Settings::default()
        } else {
            Settings::load(&dirs)
        };
        if options.light {
            settings.theme = crate::settings::ThemeChoice::Light;
        }
        theme::apply_choice(ctx, settings.theme);

        let logo = (icon.width > 0).then(|| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            );
            ctx.load_texture("logo", image, egui::TextureOptions::LINEAR)
        });
        let catalog = Arc::new(Catalog::embedded());
        let mut icons = HashMap::new();
        for app in &catalog.apps {
            if let Some(image) = crate::embedded_icon(&app.id).and_then(|b| decode(b, Some(160))) {
                let name = format!("icon-{}", app.id);
                icons.insert(
                    app.id.clone(),
                    ctx.load_texture(name, image, egui::TextureOptions::LINEAR),
                );
            }
        }
        let (tx, rx) = channel();
        let mut app = App {
            dirs,
            demo: options.demo,
            net_images: !options.demo || options.live_images,
            settings,
            catalog,
            source: Source::BuiltIn,
            releases: HashMap::new(),
            api_error: None,
            checking: false,
            registry: Vec::new(),
            installed: HashMap::new(),
            last_scan: Instant::now(),
            scanning: false,
            tab: Tab::Discover,
            page: None,
            search: String::new(),
            focus_search: false,
            category: None,
            jobs: HashMap::new(),
            images: HashMap::new(),
            icons,
            lightbox: None,
            confirm_uninstall: None,
            pending: Vec::new(),
            cache_size: None,
            toasts: Vec::new(),
            tx,
            rx,
            logo,
            shot: options.screenshot.clone().map(|path| Shot {
                path,
                frames: 0,
                requested: false,
            }),
        };
        if app.demo {
            let by_repo = demo::releases();
            for a in &app.catalog.apps {
                if let Some(r) = a.repo_name().and_then(|n| by_repo.get(n)) {
                    app.releases.insert(a.id.clone(), r.clone());
                }
            }
            app.registry = demo::registry();
            app.recompute_installed();
        } else {
            app.registry = installed::scan();
            app.recompute_installed();
            let online = app.settings.check_at_start;
            app.refresh(ctx, github::CACHE_SECONDS, online);
        }
        app.apply_view(options.view.as_deref().unwrap_or("discover"));
        app
    }

    fn apply_view(&mut self, view: &str) {
        let (name, arg) = view.split_once(':').unwrap_or((view, "snapforge"));
        match name {
            "app" | "gallery" => {
                if self.catalog.apps.iter().any(|a| a.id == arg) {
                    self.page = Some(arg.to_string());
                    if name == "gallery" {
                        self.lightbox = Some((arg.to_string(), 0));
                    }
                }
            }
            "installed" => self.tab = Tab::Installed,
            "updates" => self.tab = Tab::Updates,
            "settings" => self.tab = Tab::Settings,
            "about" => self.tab = Tab::About,
            _ => self.tab = Tab::Discover,
        }
    }

    // ----------------------------------------------------------------------------------
    // State
    // ----------------------------------------------------------------------------------

    fn recompute_installed(&mut self) {
        self.installed = self
            .catalog
            .apps
            .iter()
            .filter_map(|a| installed::find(&self.registry, &a.detect).map(|i| (a.id.clone(), i)))
            .collect();
    }

    pub(super) fn state(&self, app: &CatApp) -> AppState {
        match self.installed.get(&app.id) {
            Some(inst) => match self.releases.get(&app.id) {
                Some(rel) if version::is_newer(&rel.tag, &inst.version) => AppState::Update,
                _ => AppState::Installed,
            },
            None if !app.available() => AppState::ComingSoon,
            None => AppState::NotInstalled,
        }
    }

    pub(super) fn update_count(&self) -> usize {
        self.catalog
            .apps
            .iter()
            .filter(|a| self.state(a) == AppState::Update)
            .count()
    }

    pub(super) fn find_app(&self, id: &str) -> Option<CatApp> {
        self.catalog.apps.iter().find(|a| a.id == id).cloned()
    }

    pub(super) fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push(Toast {
            text: text.into(),
            error,
            born: Instant::now(),
        });
    }

    // ----------------------------------------------------------------------------------
    // Background work
    // ----------------------------------------------------------------------------------

    /// Loads the catalogue and the latest releases. Answers younger than
    /// `max_age` seconds come from the cache; with `online` false nothing is
    /// requested at all.
    pub(super) fn refresh(&mut self, ctx: &egui::Context, max_age: u64, online: bool) {
        if self.demo || self.checking {
            return;
        }
        self.checking = true;
        self.api_error = None;
        self.images.retain(|_, v| !matches!(v, Img::Failed));
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let cache = self.dirs.cache();
        let current = self.catalog.clone();
        std::thread::spawn(move || {
            let saved = cache.join("catalog.json");
            let mut catalog: Option<(Catalog, Source)> = None;
            if online {
                if let Ok(resp) = net::get(crate::CATALOG_URL, &[], 1 << 20) {
                    if resp.status == 200 {
                        let text = String::from_utf8_lossy(&resp.body);
                        if let Ok(cat) = Catalog::parse(&text) {
                            let _ = std::fs::create_dir_all(&cache);
                            let _ = std::fs::write(&saved, text.as_bytes());
                            catalog = Some((cat, Source::Live));
                        }
                    }
                }
            }
            if catalog.is_none() {
                catalog = std::fs::read_to_string(&saved)
                    .ok()
                    .and_then(|t| Catalog::parse(&t).ok())
                    .map(|c| (c, Source::Saved));
            }
            let apps = match &catalog {
                Some((c, _)) => c.apps.clone(),
                None => current.apps.clone(),
            };
            if let Some((c, source)) = catalog {
                let _ = tx.send(Msg::Catalog(Box::new(c), source));
            }
            let now = crate::now_epoch();
            let mut blocked: Option<ApiError> = None;
            for app in apps.iter().filter(|a| a.available()) {
                let Some(repo) = app.repo_name() else {
                    continue;
                };
                let cached = github::load_cached(&cache, repo);
                let fresh = cached.as_ref().is_some_and(|c| c.is_fresh(now, max_age));
                let (release, error) = if fresh || !online {
                    (cached.map(|c| c.release), None)
                } else if let Some(e) = &blocked {
                    (cached.map(|c| c.release), Some(e.clone()))
                } else {
                    match github::fetch_latest(repo) {
                        Ok(release) => {
                            let entry = Cached {
                                fetched_at: now,
                                release,
                            };
                            github::store_cached(&cache, repo, &entry);
                            (Some(entry.release), None)
                        }
                        Err(e) => {
                            if matches!(e, ApiError::Offline(_) | ApiError::RateLimited { .. }) {
                                blocked = Some(e.clone());
                            }
                            (cached.map(|c| c.release), Some(e))
                        }
                    }
                };
                let _ = tx.send(Msg::Release {
                    id: app.id.clone(),
                    release: release.map(Box::new),
                    error,
                });
                ctx.request_repaint();
            }
            let _ = tx.send(Msg::ReleasesDone);
            ctx.request_repaint();
        });
    }

    fn rescan_registry(&mut self, ctx: &egui::Context) {
        if self.demo || self.scanning {
            return;
        }
        self.scanning = true;
        self.last_scan = Instant::now();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Registry(installed::scan()));
            ctx.request_repaint();
        });
    }

    /// A screenshot or icon by address: loaded lazily, cached on disk.
    /// `thumb` asks for a reduced copy to keep memory low.
    pub(super) fn image(
        &mut self,
        ctx: &egui::Context,
        url: &str,
        thumb: bool,
    ) -> Option<TextureHandle> {
        let key = format!("{}|{url}", if thumb { "t" } else { "f" });
        match self.images.get(&key) {
            Some(Img::Ready(t)) => return Some(t.clone()),
            Some(_) => return None,
            None => {}
        }
        if !net::is_allowed_fetch(url) || !self.net_images {
            self.images.insert(key, Img::Failed);
            return None;
        }
        self.images.insert(key.clone(), Img::Loading);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let url = url.to_string();
        // Demo mode never touches the disk.
        let file = (!self.demo).then(|| {
            self.dirs
                .cache()
                .join("img")
                .join(format!("{}.png", hash_name(&url)))
        });
        std::thread::spawn(move || {
            let cached = file.as_ref().and_then(|f| std::fs::read(f).ok());
            let bytes = cached.or_else(|| {
                let resp = net::get(&url, &[], 12 << 20).ok()?;
                (resp.status == 200).then_some(resp.body).inspect(|body| {
                    if let Some(f) = &file {
                        if let Some(parent) = f.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let _ = std::fs::write(f, body);
                    }
                })
            });
            let image = bytes.and_then(|b| decode(&b, thumb.then_some(560)));
            let _ = tx.send(Msg::Image(key, image));
            ctx.request_repaint();
        });
        None
    }

    pub(super) fn image_failed(&self, url: &str, thumb: bool) -> bool {
        let key = format!("{}|{url}", if thumb { "t" } else { "f" });
        matches!(self.images.get(&key), Some(Img::Failed))
    }

    pub(super) fn icon(&mut self, ctx: &egui::Context, app: &CatApp) -> Option<TextureHandle> {
        if let Some(t) = self.icons.get(&app.id) {
            return Some(t.clone());
        }
        if app.icon.is_empty() {
            return None;
        }
        self.image(ctx, &app.icon, true)
    }

    fn drain_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Catalog(cat, source) => {
                    self.catalog = Arc::new(*cat);
                    self.source = source;
                    self.recompute_installed();
                }
                Msg::Release { id, release, error } => {
                    if let Some(r) = release {
                        self.releases.insert(id, *r);
                    }
                    if let Some(e) = error {
                        let serious =
                            matches!(e, ApiError::Offline(_) | ApiError::RateLimited { .. });
                        if serious || self.api_error.is_none() {
                            // A missing release is normal for a brand-new repository.
                            if e != ApiError::NoRelease {
                                self.api_error = Some(e);
                            }
                        }
                    }
                }
                Msg::ReleasesDone => self.checking = false,
                Msg::Registry(entries) => {
                    self.scanning = false;
                    if entries != self.registry {
                        self.registry = entries;
                        self.recompute_installed();
                    }
                }
                Msg::Image(key, image) => {
                    let state = match image {
                        Some(img) => Img::Ready(ctx.load_texture(
                            key.clone(),
                            img,
                            egui::TextureOptions::LINEAR,
                        )),
                        None => Img::Failed,
                    };
                    self.images.insert(key, state);
                }
                Msg::JobDone(id, kind, result) => self.job_done(&id, kind, result),
            }
        }
    }

    // ----------------------------------------------------------------------------------
    // Actions
    // ----------------------------------------------------------------------------------

    fn start_job(&mut self, ctx: &egui::Context, id: &str, kind: JobKind) {
        if self.jobs.contains_key(id) {
            return;
        }
        let Some(app) = self.find_app(id) else { return };
        if self.demo {
            self.toast("Demo mode: nothing is downloaded or installed.", false);
            return;
        }
        let asset_kind = if kind == JobKind::Portable {
            AssetKind::Portable
        } else {
            AssetKind::Setup
        };
        let asset = self
            .releases
            .get(id)
            .and_then(|r| r.asset(&app, asset_kind))
            .cloned();
        let (Some(asset), Some(repo)) = (asset, app.repo_name().map(str::to_string)) else {
            self.toast(
                format!(
                    "No download is known for {} yet. Press Refresh and try again.",
                    app.name
                ),
                true,
            );
            return;
        };
        let dir = if kind == JobKind::Portable {
            self.settings.portable_dir(&self.dirs)
        } else {
            self.dirs.installers()
        };
        let job = Job {
            kind,
            progress: Arc::new(AtomicU64::new(0)),
            total: asset.size,
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let (progress, cancel) = (job.progress.clone(), job.cancel.clone());
        self.jobs.insert(id.to_string(), job);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            let result = actions::download_asset(&asset, &repo, &dir, &progress, &cancel);
            let _ = tx.send(Msg::JobDone(id, kind, result));
            ctx.request_repaint();
        });
    }

    fn job_done(&mut self, id: &str, kind: JobKind, result: Result<PathBuf, String>) {
        self.jobs.remove(id);
        let name = self
            .find_app(id)
            .map(|a| a.name)
            .unwrap_or_else(|| id.to_string());
        match result {
            Ok(path) if kind == JobKind::Portable => {
                let folder = path
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let file = path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.toast(format!("Saved {file} to {folder}"), false);
            }
            Ok(path) => {
                let started = actions::installer_launch(&path).and_then(|l| actions::run(&l));
                match started {
                    Ok(()) => self.toast(
                        format!("The {name} installer is open. Finish the steps there."),
                        false,
                    ),
                    Err(e) => self.toast(e, true),
                }
            }
            Err(e) if e == net::NetError::Cancelled.to_string() => {
                self.toast(format!("{name}: download cancelled."), false)
            }
            Err(e) => self.toast(format!("{name}: {e}"), true),
        }
    }

    fn act(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::OpenPage(id) => self.page = Some(id),
            Action::Install(id) => self.start_job(ctx, &id, JobKind::Install),
            Action::Update(id) => self.start_job(ctx, &id, JobKind::Update),
            Action::Portable(id) => self.start_job(ctx, &id, JobKind::Portable),
            Action::UpdateAll => {
                let ids: Vec<String> = self
                    .catalog
                    .apps
                    .iter()
                    .filter(|a| self.state(a) == AppState::Update)
                    .map(|a| a.id.clone())
                    .collect();
                for id in ids {
                    self.start_job(ctx, &id, JobKind::Update);
                }
            }
            Action::Launch(id) => {
                if self.demo {
                    self.toast("Demo mode: nothing is started.", false);
                    return;
                }
                let launch = match (self.find_app(&id), self.installed.get(&id)) {
                    (Some(app), Some(inst)) => installed::open_launch(inst, &app.assets.exe),
                    _ => Err("This app is not installed.".to_string()),
                };
                if let Err(e) = launch.and_then(|l| actions::run(&l)) {
                    self.toast(e, true);
                }
            }
            Action::AskUninstall(id) => self.confirm_uninstall = Some(id),
            Action::Url(url) => {
                if self.demo {
                    self.toast(format!("Demo mode: would open {url}"), false);
                } else if let Err(e) = actions::open_page(&url) {
                    self.toast(e, true);
                }
            }
            Action::Cancel(id) => {
                if let Some(job) = self.jobs.get(&id) {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            }
            Action::Refresh => {
                if self.demo {
                    self.toast("Demo mode: built-in data, nothing to refresh.", false);
                } else {
                    self.rescan_registry(ctx);
                    // A manual refresh asks again for anything older than five minutes.
                    self.refresh(ctx, 300, true);
                }
            }
        }
    }

    fn uninstall(&mut self, id: &str) {
        if self.demo {
            self.toast("Demo mode: nothing is uninstalled.", false);
            return;
        }
        let result = self
            .installed
            .get(id)
            .ok_or_else(|| "This app is not installed.".to_string())
            .and_then(installed::uninstall_launch)
            .and_then(|l| actions::run(&l));
        match result {
            Ok(()) => self.toast("The uninstaller is open. Finish the steps there.", false),
            Err(e) => self.toast(e, true),
        }
    }

    // ----------------------------------------------------------------------------------
    // Keyboard and screenshots
    // ----------------------------------------------------------------------------------

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (esc, left, right, find, f5) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.modifiers.command && i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::F5),
            )
        });
        if let Some((id, index)) = self.lightbox.clone() {
            let count = self
                .find_app(&id)
                .map(|a| a.screenshots.len())
                .unwrap_or(0)
                .max(1);
            if esc {
                self.lightbox = None;
            } else if left {
                self.lightbox = Some((id, (index + count - 1) % count));
            } else if right {
                self.lightbox = Some((id, (index + 1) % count));
            }
            return;
        }
        if esc && self.confirm_uninstall.is_none() && self.page.is_some() {
            self.page = None;
        }
        if find {
            self.page = None;
            if matches!(self.tab, Tab::Settings | Tab::About) {
                self.tab = Tab::Discover;
            }
            self.focus_search = true;
        }
        if f5 {
            self.pending.push(Action::Refresh);
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let busy = self.checking || self.images.values().any(|i| matches!(i, Img::Loading));
        let Some(shot) = &mut self.shot else { return };
        ctx.request_repaint();
        shot.frames += 1;
        if shot.frames == 1 {
            ctx.set_pixels_per_point(1.0);
        }
        if shot.frames == 3 {
            // Now that one point is one pixel, this gives an exact 1280 x 800 image.
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(1280.0, 800.0)));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            if let Some(parent) = shot.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let result = image::save_buffer(
                &shot.path,
                image.as_raw(),
                w as u32,
                h as u32,
                image::ColorType::Rgba8,
            );
            if let Err(e) = result {
                eprintln!("screenshot failed: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            self.shot = None;
            return;
        }
        if !shot.requested && shot.frames > 40 && (!busy || shot.frames > 2400) {
            shot.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }

    // ----------------------------------------------------------------------------------
    // Frame: navigation, header, banners
    // ----------------------------------------------------------------------------------

    fn rail(&mut self, ctx: &egui::Context) {
        let p = pal_ctx(ctx);
        egui::SidePanel::left("rail")
            .exact_width(212.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(p.rail)
                    .inner_margin(Margin::symmetric(10, 14)),
            )
            .show(ctx, |ui| {
                let full = ui.max_rect();
                ui.painter().vline(
                    full.right() + 10.0,
                    full.y_range().expand(14.0),
                    Stroke::new(1.0_f32, p.border),
                );
                ui.spacing_mut().item_spacing.y = 4.0;
                widgets::row(ui, 40.0, |ui| {
                    ui.add_space(6.0);
                    if let Some(logo) = &self.logo {
                        ui.add(egui::Image::new(logo).fit_to_exact_size(vec2(30.0, 30.0)));
                    }
                    ui.label(RichText::new("ForgeDock").font(semibold(18.0)));
                    if self.demo {
                        widgets::badge(ui, "Demo", p.pink);
                    }
                });
                ui.add_space(12.0);
                let installed = self.installed.len();
                let updates = self.update_count();
                let items: [(Tab, Icon, &str, Option<usize>); 5] = [
                    (Tab::Discover, Icon::SHOP, "Discover", None),
                    (Tab::Installed, Icon::APPS, "Installed", Some(installed)),
                    (Tab::Updates, Icon::SYNC, "Updates", Some(updates)),
                    (Tab::Settings, Icon::SETTINGS, "Settings", None),
                    (Tab::About, Icon::INFO, "About", None),
                ];
                for (tab, icon, label, count) in items {
                    if tab == Tab::Settings {
                        ui.add_space(8.0);
                        let (line, _) =
                            ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                        ui.painter().rect_filled(line, CornerRadius::ZERO, p.border);
                        ui.add_space(8.0);
                    }
                    let active = self.tab == tab && self.page.is_none();
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                    if active {
                        ui.painter()
                            .rect_filled(rect, CornerRadius::same(9), p.selected);
                        let bar = Rect::from_min_size(
                            rect.left_center() - vec2(0.0, 9.0),
                            vec2(3.0, 18.0),
                        );
                        ui.painter()
                            .rect_filled(bar, CornerRadius::same(2), p.accent);
                    } else if resp.hovered() {
                        ui.painter()
                            .rect_filled(rect, CornerRadius::same(9), p.hover);
                    }
                    let fg = if active { p.text } else { p.muted };
                    ui.painter().text(
                        rect.left_center() + vec2(16.0, 0.0),
                        Align2::LEFT_CENTER,
                        icon.glyph(),
                        theme::regular(16.0),
                        if active { p.accent } else { p.muted },
                    );
                    ui.painter().text(
                        rect.left_center() + vec2(46.0, 0.0),
                        Align2::LEFT_CENTER,
                        label,
                        if active {
                            semibold(14.0)
                        } else {
                            theme::regular(14.0)
                        },
                        fg,
                    );
                    if let Some(n) = count {
                        if tab == Tab::Updates && n > 0 {
                            let text = n.to_string();
                            let pill = Rect::from_center_size(
                                rect.right_center() - vec2(22.0, 0.0),
                                vec2(24.0, 20.0),
                            );
                            ui.painter()
                                .rect_filled(pill, CornerRadius::same(10), p.warn);
                            ui.painter().text(
                                pill.center(),
                                Align2::CENTER_CENTER,
                                text,
                                semibold(12.0),
                                egui::Color32::from_rgb(0x1c, 0x14, 0x02),
                            );
                        } else if tab == Tab::Installed {
                            ui.painter().text(
                                rect.right_center() - vec2(14.0, 0.0),
                                Align2::RIGHT_CENTER,
                                n.to_string(),
                                theme::regular(12.5),
                                p.faint,
                            );
                        }
                    }
                    if resp.clicked() {
                        self.tab = tab;
                        self.page = None;
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.add_space(2.0);
                    let source = if self.demo {
                        "Built-in demo data"
                    } else {
                        match self.source {
                            Source::Live => "Catalogue: up to date",
                            Source::Saved => "Catalogue: saved copy",
                            Source::BuiltIn => "Catalogue: built-in copy",
                        }
                    };
                    ui.label(
                        RichText::new(format!("  {source}"))
                            .size(12.0)
                            .color(p.faint),
                    );
                });
            });
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let page = self.page.clone().and_then(|id| self.find_app(&id));
        widgets::row(ui, 40.0, |ui| {
            match &page {
                Some(app) => {
                    if widgets::ghost_button(ui, &Icon::BACK.with("Back")).clicked() {
                        self.page = None;
                    }
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!("{}  /  {}", app.category, app.name)).color(p.muted),
                    );
                }
                None => {
                    let title = match self.tab {
                        Tab::Discover => "Discover",
                        Tab::Installed => "Installed",
                        Tab::Updates => "Updates",
                        Tab::Settings => "Settings",
                        Tab::About => "About",
                    };
                    ui.label(RichText::new(title).font(semibold(24.0)));
                }
            }
            widgets::right(ui, |ui| {
                if self.checking {
                    ui.add(egui::Spinner::new().size(18.0));
                    ui.label(RichText::new("Checking…").color(p.muted));
                } else if widgets::icon_button(ui, Icon::REFRESH, "Check for new versions (F5)")
                    .clicked()
                {
                    self.pending.push(Action::Refresh);
                }
                let searchable = page.is_none()
                    && matches!(self.tab, Tab::Discover | Tab::Installed | Tab::Updates);
                if searchable {
                    let edit = egui::TextEdit::singleline(&mut self.search)
                        .hint_text(Icon::SEARCH.with("Search apps"))
                        .desired_width(240.0)
                        .margin(Margin::symmetric(10, 7));
                    let resp = ui.add(edit);
                    if self.focus_search {
                        resp.request_focus();
                        self.focus_search = false;
                    }
                }
            });
        });
        ui.add_space(4.0);
        if let Some(err) = &self.api_error {
            let (icon, color) = match err {
                ApiError::Offline(_) => (Icon::OFFLINE, p.warn),
                ApiError::RateLimited { .. } => (Icon::CLOCK, p.warn),
                _ => (Icon::WARNING, p.danger),
            };
            let text = err.message(crate::now_epoch());
            Frame::new()
                .fill(color.gamma_multiply(0.12))
                .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.5)))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(14, 9))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon.glyph()).color(color));
                        ui.add(egui::Label::new(RichText::new(text).color(p.text)).wrap());
                    });
                });
            ui.add_space(6.0);
        }
    }

    // ----------------------------------------------------------------------------------
    // Dialogs and toasts
    // ----------------------------------------------------------------------------------

    fn modal_frame(ctx: &egui::Context, margin: i8) -> Frame {
        let p = pal_ctx(ctx);
        Frame::new()
            .fill(p.card)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(margin))
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_uninstall.clone() else {
            return;
        };
        let Some(app) = self.find_app(&id) else {
            self.confirm_uninstall = None;
            return;
        };
        let p = pal_ctx(ctx);
        let location = self
            .installed
            .get(&id)
            .map(|i| i.location.clone())
            .unwrap_or_default();
        let mut decision: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::new("confirm-uninstall"))
            .frame(Self::modal_frame(ctx, 22))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(RichText::new(format!("Uninstall {}?", app.name)).font(semibold(19.0)));
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "ForgeDock starts {}'s own uninstaller. You confirm and finish it in its window.",
                            app.name
                        ))
                        .color(p.muted),
                    )
                    .wrap(),
                );
                if !location.is_empty() {
                    ui.add(egui::Label::new(RichText::new(location).color(p.faint)).wrap());
                }
                ui.add_space(10.0);
                // A fixed-height row: a bare right-to-left layout would stretch the modal.
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 36.0),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        if widgets::danger_button(ui, "Uninstall", true).clicked() {
                            decision = Some(true);
                        }
                        if widgets::ghost_button(ui, "Cancel").clicked() {
                            decision = Some(false);
                        }
                    },
                );
            });
        if modal.should_close() && decision.is_none() {
            decision = Some(false);
        }
        match decision {
            Some(true) => {
                self.confirm_uninstall = None;
                self.uninstall(&id);
            }
            Some(false) => self.confirm_uninstall = None,
            None => {}
        }
    }

    fn lightbox_dialog(&mut self, ctx: &egui::Context) {
        let Some((id, index)) = self.lightbox.clone() else {
            return;
        };
        let shots = self
            .find_app(&id)
            .map(|a| a.screenshots)
            .unwrap_or_default();
        if shots.is_empty() {
            self.lightbox = None;
            return;
        }
        let index = index.min(shots.len() - 1);
        let url = shots[index].clone();
        let tex = self.image(ctx, &url, false);
        let failed = self.image_failed(&url, false);
        let p = pal_ctx(ctx);
        let screen = ctx.screen_rect().size();
        let max = vec2((screen.x - 140.0).max(320.0), (screen.y - 170.0).max(200.0));
        let size = match &tex {
            Some(t) => {
                let s = t.size_vec2();
                s * (max.x / s.x).min(max.y / s.y).min(1.0)
            }
            None => vec2(max.x.min(880.0), max.y.min(550.0)),
        };
        let mut go: Option<isize> = None;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("lightbox"))
            .frame(Self::modal_frame(ctx, 14))
            .show(ctx, |ui| {
                ui.set_width(size.x);
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                match &tex {
                    Some(t) => {
                        egui::Image::new(t)
                            .corner_radius(CornerRadius::same(8))
                            .paint_at(ui, rect);
                    }
                    None => {
                        ui.painter().rect_filled(rect, CornerRadius::same(8), p.bg);
                        let text = if failed {
                            "This screenshot could not be loaded."
                        } else {
                            "Loading…"
                        };
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            text,
                            theme::regular(14.0),
                            p.muted,
                        );
                    }
                }
                ui.add_space(2.0);
                ui.allocate_ui_with_layout(
                    vec2(size.x, 36.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        if widgets::ghost_button(ui, &Icon::CHEVRON_LEFT.with("Previous")).clicked()
                        {
                            go = Some(-1);
                        }
                        if widgets::ghost_button(ui, &Icon::CHEVRON_RIGHT.with("Next")).clicked() {
                            go = Some(1);
                        }
                        ui.label(
                            RichText::new(format!("{} of {}", index + 1, shots.len()))
                                .color(p.muted),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::ghost_button(ui, "Close").clicked() {
                                close = true;
                            }
                        });
                    },
                );
            });
        if close || modal.should_close() {
            self.lightbox = None;
        } else if let Some(step) = go {
            let n = shots.len() as isize;
            self.lightbox = Some((id, ((index as isize + step + n) % n) as usize));
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        self.toasts
            .retain(|t| t.born.elapsed().as_secs_f32() < if t.error { 9.0 } else { 4.5 });
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let p = pal_ctx(ctx);
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-18.0, -18.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.with_layout(Layout::bottom_up(Align::Max), |ui| {
                    for t in self.toasts.iter().rev().take(4) {
                        let color = if t.error { p.danger } else { p.ok };
                        Frame::new()
                            .fill(p.card)
                            .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.7)))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.set_max_width(440.0);
                                ui.horizontal(|ui| {
                                    let icon = if t.error { Icon::ERROR } else { Icon::DONE };
                                    ui.label(RichText::new(icon.glyph()).color(color));
                                    ui.add(egui::Label::new(t.text.as_str()).wrap());
                                });
                            });
                    }
                });
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_messages(ctx);
        self.handle_keys(ctx);
        self.handle_screenshot(ctx);

        // Installers and uninstallers run outside ForgeDock: look again now and then.
        if !self.demo && self.shot.is_none() {
            let focused = ctx.input(|i| i.focused);
            if focused && self.last_scan.elapsed() > Duration::from_secs(4) {
                self.rescan_registry(ctx);
            }
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        if !self.jobs.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        self.rail(ctx);
        let p = pal_ctx(ctx);
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(p.bg)
                    .inner_margin(Margin::symmetric(24, 18)),
            )
            .show(ctx, |ui| {
                self.header(ui);
                match self.page.clone().and_then(|id| self.find_app(&id)) {
                    Some(app) => self.app_page(ui, &app),
                    None => match self.tab {
                        Tab::Discover => self.discover_ui(ui),
                        Tab::Installed => self.installed_ui(ui),
                        Tab::Updates => self.updates_ui(ui),
                        Tab::Settings => self.settings_ui(ui),
                        Tab::About => self.about_ui(ui),
                    },
                }
            });

        self.lightbox_dialog(ctx);
        self.confirm_dialog(ctx);
        self.toasts_ui(ctx);

        for action in std::mem::take(&mut self.pending) {
            self.act(ctx, action);
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // Screenshots must not depend on where the mouse happens to be:
        // no hover highlights, no tooltips.
        if self.shot.is_some() {
            raw_input.events.retain(|e| {
                !matches!(
                    e,
                    egui::Event::PointerMoved(_)
                        | egui::Event::MouseMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::MouseWheel { .. }
                )
            });
            raw_input.events.push(egui::Event::PointerGone);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for job in self.jobs.values() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}
