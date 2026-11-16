//! The pages: Discover, an app's page, Installed, Updates, Settings, About.

use super::app::{Action, App, AppState, JobKind, Source, Tab};
use super::theme::{self, pal, regular, semibold, Icon};
use super::widgets;
use crate::actions::human_size;
use crate::catalog::App as CatApp;
use crate::github::{note_lines, AssetKind};
use crate::settings::ThemeChoice;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
};

const GAP: f32 = 14.0;

impl App {
    fn badge_for(&self, ui: &Ui, state: AppState) -> (&'static str, Color32) {
        let p = pal(ui);
        match state {
            AppState::ComingSoon => ("Coming soon", p.pink),
            AppState::NotInstalled => ("Not installed", p.muted),
            AppState::Installed => ("Installed", p.ok),
            AppState::Update => ("Update available", p.warn),
        }
    }

    fn visible_apps(&self, only: impl Fn(AppState) -> bool) -> Vec<CatApp> {
        self.catalog
            .apps
            .iter()
            .filter(|a| a.matches_query(&self.search))
            .filter(|a| {
                self.category
                    .as_ref()
                    .is_none_or(|c| &a.category == c || self.tab != Tab::Discover)
            })
            .filter(|a| only(self.state(a)))
            .cloned()
            .collect()
    }

    // ----------------------------------------------------------------------------------
    // Discover
    // ----------------------------------------------------------------------------------

    pub(super) fn discover_ui(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let categories = self.catalog.categories();
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, "All", self.category.is_none()).clicked() {
                self.category = None;
            }
            for c in &categories {
                let active = self.category.as_ref() == Some(c);
                if widgets::chip(ui, c, active).clicked() {
                    self.category = if active { None } else { Some(c.clone()) };
                }
            }
        });
        ui.add_space(6.0);
        let apps = self.visible_apps(|_| true);
        if apps.is_empty() {
            widgets::empty_state(
                ui,
                Icon::SEARCH,
                p.faint,
                "No apps match",
                "Try another word or category.",
            );
            return;
        }
        let (ready, soon): (Vec<CatApp>, Vec<CatApp>) =
            apps.into_iter().partition(|a| a.available());
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if !ready.is_empty() {
                    widgets::section(ui, &format!("AVAILABLE NOW  ·  {}", ready.len()));
                    self.grid(ui, &ready);
                }
                if !soon.is_empty() {
                    ui.add_space(10.0);
                    widgets::section(ui, &format!("COMING SOON  ·  {}", soon.len()));
                    self.grid(ui, &soon);
                }
                ui.add_space(8.0);
            });
    }

    fn grid(&mut self, ui: &mut Ui, apps: &[CatApp]) {
        let avail = ui.available_width() - 8.0;
        let cols = (((avail + GAP) / (300.0 + GAP)).floor() as usize).max(1);
        let width = ((avail - GAP * (cols as f32 - 1.0)) / cols as f32).floor();
        for chunk in apps.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for app in chunk {
                    self.app_card(ui, app, width);
                }
            });
            ui.add_space(GAP - 8.0);
        }
    }

    fn app_card(&mut self, ui: &mut Ui, app: &CatApp, width: f32) {
        let p = pal(ui);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 164.0), Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let fill = if resp.hovered() { p.hover } else { p.card };
        let radius = CornerRadius::same(12);
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0_f32, if resp.hovered() { p.faint } else { p.border }),
            StrokeKind::Inside,
        );
        let icon_rect = Rect::from_min_size(rect.min + vec2(16.0, 16.0), vec2(56.0, 56.0));
        let tex = self.icon(ui.ctx(), app);
        widgets::paint_icon(ui, icon_rect, &app.name, app.accents().0, tex.as_ref());
        let text_x = rect.left() + 86.0;
        let text_w = width - 86.0 - 14.0;
        let name = widgets::galley(ui, &app.name, semibold(17.0), p.text, text_w, 1);
        ui.painter()
            .galley(pos2(text_x, rect.top() + 20.0), name, p.text);
        let category = widgets::galley(ui, &app.category, regular(12.5), p.faint, text_w, 1);
        ui.painter()
            .galley(pos2(text_x, rect.top() + 46.0), category, p.faint);
        let tagline = widgets::galley(ui, &app.tagline, regular(13.5), p.muted, width - 32.0, 2);
        ui.painter().galley(
            pos2(rect.left() + 16.0, rect.top() + 82.0),
            tagline,
            p.muted,
        );

        let state = self.state(app);
        let (label, color) = self.badge_for(ui, state);
        let base = pos2(rect.left() + 16.0, rect.bottom() - 21.0);
        if let Some(job) = self.jobs.get(&app.id) {
            let done = job.progress.load(std::sync::atomic::Ordering::Relaxed);
            let pct = (done as f32 / job.total.max(1) as f32 * 100.0).min(100.0);
            widgets::paint_badge(ui, base, &format!("Downloading {pct:.0}%"), p.accent);
        } else {
            widgets::paint_badge(ui, base, label, color);
        }
        let version = match (self.installed.get(&app.id), self.releases.get(&app.id)) {
            (Some(inst), Some(rel)) if state == AppState::Update => {
                format!("{} → {}", inst.version, rel.version())
            }
            (Some(inst), _) => format!("v{}", inst.version),
            (None, Some(rel)) => format!("v{}", rel.version()),
            _ => String::new(),
        };
        ui.painter().text(
            pos2(rect.right() - 16.0, rect.bottom() - 21.0),
            Align2::RIGHT_CENTER,
            version,
            regular(12.5),
            p.faint,
        );
        if resp.clicked() {
            self.pending.push(Action::OpenPage(app.id.clone()));
        }
    }

    // ----------------------------------------------------------------------------------
    // Action buttons (shared by the app page and the lists)
    // ----------------------------------------------------------------------------------

    /// Buttons for an app, in reading order. `full` adds the secondary ones.
    fn action_buttons(&mut self, ui: &mut Ui, app: &CatApp, full: bool) {
        let p = pal(ui);
        let id = &app.id;
        if let Some(job) = self.jobs.get(id) {
            let done = job.progress.load(std::sync::atomic::Ordering::Relaxed);
            let fraction = (done as f32 / job.total.max(1) as f32).min(1.0);
            let what = match job.kind {
                JobKind::Install => "Downloading installer",
                JobKind::Update => "Downloading update",
                JobKind::Portable => "Downloading ZIP",
            };
            let bar = egui::ProgressBar::new(fraction)
                .desired_width(250.0)
                .desired_height(10.0)
                .fill(p.accent);
            let cancel = |app: &mut App, ui: &mut Ui| {
                if widgets::ghost_button(ui, "Cancel").clicked() {
                    app.pending.push(Action::Cancel(id.clone()));
                }
            };
            let text = format!("{what}  {} of {}", human_size(done), human_size(job.total));
            // In right-to-left rows the order is mirrored, so add in reverse there.
            if ui.layout().main_dir() == egui::Direction::RightToLeft {
                cancel(self, ui);
                ui.add(bar);
                ui.label(RichText::new(text).color(p.muted));
            } else {
                ui.label(RichText::new(text).color(p.muted));
                ui.add(bar);
                cancel(self, ui);
            }
            return;
        }
        let state = self.state(app);
        let release = self.releases.get(id);
        let has_setup = release.is_some_and(|r| r.asset(app, AssetKind::Setup).is_some());
        let has_zip = release.is_some_and(|r| r.asset(app, AssetKind::Portable).is_some());
        let latest = release.map(|r| r.version()).unwrap_or_default();
        let mut buttons: Vec<(u8, String, Action)> = Vec::new();
        match state {
            AppState::ComingSoon => {
                widgets::badge(ui, "Coming soon", p.pink);
                return;
            }
            AppState::NotInstalled => {
                buttons.push((
                    if has_setup { 1 } else { 3 },
                    Icon::DOWNLOAD.with("Install"),
                    Action::Install(id.clone()),
                ));
            }
            AppState::Installed => {
                buttons.push((1, Icon::PLAY.with("Open"), Action::Launch(id.clone())));
                buttons.push((0, "Uninstall".into(), Action::AskUninstall(id.clone())));
            }
            AppState::Update => {
                buttons.push((
                    1,
                    Icon::SYNC.with(&format!("Update to {latest}")),
                    Action::Update(id.clone()),
                ));
                buttons.push((0, Icon::PLAY.with("Open"), Action::Launch(id.clone())));
                buttons.push((0, "Uninstall".into(), Action::AskUninstall(id.clone())));
            }
        }
        if full {
            if has_zip {
                buttons.push((0, "Portable ZIP".into(), Action::Portable(id.clone())));
            }
            if let Some(url) = app.repo_url() {
                buttons.push((0, Icon::GLOBE.with("Repository"), Action::Url(url)));
            }
            if let Some(r) = release {
                buttons.push((0, "Release page".into(), Action::Url(r.page.clone())));
            }
        }
        if ui.layout().main_dir() == egui::Direction::RightToLeft {
            buttons.reverse();
        }
        for (style, text, action) in buttons {
            let clicked = match style {
                1 => widgets::primary_button(ui, &text, true).clicked(),
                3 => widgets::primary_button(ui, &text, false)
                    .on_disabled_hover_text("Version information has not been loaded yet.")
                    .clicked(),
                _ => widgets::ghost_button(ui, &text).clicked(),
            };
            if clicked {
                self.pending.push(action);
            }
        }
    }

    // ----------------------------------------------------------------------------------
    // App page
    // ----------------------------------------------------------------------------------

    pub(super) fn app_page(&mut self, ui: &mut Ui, app: &CatApp) {
        let p = pal(ui);
        let state = self.state(app);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width() - 8.0);
                // Header: icon, name, tagline, actions.
                widgets::card(ui, |ui| {
                    widgets::row(ui, 96.0, |ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(96.0, 96.0), Sense::hover());
                        let tex = self.icon(ui.ctx(), app);
                        widgets::paint_icon(ui, rect, &app.name, app.accents().0, tex.as_ref());
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&app.name).font(semibold(28.0)));
                                let (label, color) = self.badge_for(ui, state);
                                widgets::badge(ui, label, color);
                            });
                            ui.label(RichText::new(&app.tagline).size(15.0).color(p.muted));
                            ui.label(
                                RichText::new(format!("{}  ·  by Zheer Khalid", app.category))
                                    .size(12.5)
                                    .color(p.faint),
                            );
                        });
                    });
                    ui.add_space(6.0);
                    widgets::row(ui, 36.0, |ui| self.action_buttons(ui, app, true));
                });
                ui.add_space(4.0);

                // Versions, date and size.
                let release = self.releases.get(&app.id).cloned();
                let installed = self.installed.get(&app.id).cloned();
                widgets::card(ui, |ui| {
                    ui.columns(4, |cols| {
                        let none = "—".to_string();
                        let inst = installed
                            .as_ref()
                            .map(|i| i.version.clone())
                            .unwrap_or_else(|| "Not installed".into());
                        widgets::stat(&mut cols[0], "Installed version", &inst);
                        let latest = match &release {
                            Some(r) => r.version(),
                            None if self.checking => "Checking…".into(),
                            None => none.clone(),
                        };
                        widgets::stat(&mut cols[1], "Latest version", &latest);
                        let date = release
                            .as_ref()
                            .map(|r| r.date())
                            .filter(|d| !d.is_empty())
                            .unwrap_or_else(|| none.clone());
                        widgets::stat(&mut cols[2], "Released", &date);
                        let size = release
                            .as_ref()
                            .and_then(|r| r.asset(app, AssetKind::Setup))
                            .map(|a| human_size(a.size))
                            .unwrap_or(none);
                        widgets::stat(&mut cols[3], "Installer download", &size);
                    });
                });
                ui.add_space(4.0);

                if !app.screenshots.is_empty() {
                    self.gallery(ui, app);
                    ui.add_space(4.0);
                }

                ui.columns(2, |cols| {
                    let ui = &mut cols[0];
                    widgets::card(ui, |ui| {
                        widgets::section(ui, "ABOUT");
                        for para in &app.description {
                            ui.add(egui::Label::new(RichText::new(para).size(14.5)).wrap());
                            ui.add_space(2.0);
                        }
                    });
                    if !app.features.is_empty() {
                        ui.add_space(4.0);
                        widgets::card(ui, |ui| {
                            widgets::section(ui, "FEATURES");
                            for feature in &app.features {
                                ui.horizontal_top(|ui| {
                                    ui.label(RichText::new(Icon::CHECK.glyph()).color(p.ok));
                                    ui.add(egui::Label::new(feature.as_str()).wrap());
                                });
                            }
                        });
                    }
                    let ui = &mut cols[1];
                    widgets::card(ui, |ui| {
                        widgets::section(ui, "SPECIFICATIONS");
                        let location = installed
                            .as_ref()
                            .map(|i| i.location.clone())
                            .unwrap_or_default();
                        let rows = [
                            ("Platform", app.specs.platform.clone()),
                            ("Written in", app.specs.language.clone()),
                            ("Licence", app.specs.licence.clone()),
                            ("Needs administrator", app.specs.needs_admin.clone()),
                            ("Approximate size", app.specs.size.clone()),
                            ("Source", app.repo_url().unwrap_or_default()),
                            ("Installed in", location),
                        ];
                        egui::Grid::new("specs")
                            .num_columns(2)
                            .spacing(vec2(18.0, 8.0))
                            .show(ui, |ui| {
                                for (label, value) in rows {
                                    if value.is_empty() {
                                        continue;
                                    }
                                    ui.label(RichText::new(label).color(p.muted));
                                    ui.add(egui::Label::new(value).wrap());
                                    ui.end_row();
                                }
                            });
                    });
                    if let Some(r) = &release {
                        ui.add_space(4.0);
                        widgets::card(ui, |ui| {
                            widgets::section(
                                ui,
                                &format!("WHAT'S NEW IN {}", r.version().to_uppercase()),
                            );
                            let lines = note_lines(&r.notes);
                            if lines.is_empty() {
                                ui.label(RichText::new("No release notes.").color(p.muted));
                            }
                            for (heading, bullet, text) in lines {
                                if heading {
                                    ui.label(RichText::new(text).font(semibold(14.5)));
                                } else if bullet {
                                    ui.horizontal_top(|ui| {
                                        ui.label(RichText::new("•").color(p.accent));
                                        ui.add(egui::Label::new(text).wrap());
                                    });
                                } else {
                                    ui.add(egui::Label::new(text).wrap());
                                }
                            }
                        });
                    }
                });
                ui.add_space(10.0);
            });
    }

    fn gallery(&mut self, ui: &mut Ui, app: &CatApp) {
        let p = pal(ui);
        widgets::card(ui, |ui| {
            widgets::section(ui, &format!("SCREENSHOTS  ·  {}", app.screenshots.len()));
            egui::ScrollArea::horizontal()
                .id_salt("gallery")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        for (index, url) in app.screenshots.iter().enumerate() {
                            let (rect, resp) =
                                ui.allocate_exact_size(vec2(264.0, 165.0), Sense::click());
                            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                            let radius = CornerRadius::same(8);
                            ui.painter().rect_filled(rect, radius, p.bg);
                            // Only ask for images that are (nearly) in view.
                            let tex = if ui.is_rect_visible(rect.expand(300.0)) {
                                self.image(ui.ctx(), url, true)
                            } else {
                                None
                            };
                            match tex {
                                Some(t) => {
                                    let s = t.size_vec2();
                                    let scale = (rect.width() / s.x).min(rect.height() / s.y);
                                    let fit = Rect::from_center_size(rect.center(), s * scale);
                                    egui::Image::new(&t).corner_radius(radius).paint_at(ui, fit);
                                }
                                None => {
                                    let text = if self.image_failed(url, true) {
                                        if self.demo {
                                            "Screenshots load when online"
                                        } else {
                                            "Not available offline"
                                        }
                                    } else {
                                        "Loading…"
                                    };
                                    ui.painter().text(
                                        rect.center() - vec2(0.0, 12.0),
                                        Align2::CENTER_CENTER,
                                        Icon::PICTURE.glyph(),
                                        regular(24.0),
                                        p.faint,
                                    );
                                    ui.painter().text(
                                        rect.center() + vec2(0.0, 16.0),
                                        Align2::CENTER_CENTER,
                                        text,
                                        regular(12.5),
                                        p.faint,
                                    );
                                }
                            }
                            let stroke = if resp.hovered() { p.accent } else { p.border };
                            ui.painter().rect_stroke(
                                rect,
                                radius,
                                Stroke::new(1.0_f32, stroke),
                                StrokeKind::Inside,
                            );
                            if resp.clicked() {
                                self.lightbox = Some((app.id.clone(), index));
                            }
                        }
                    });
                    ui.add_space(8.0);
                });
        });
    }

    // ----------------------------------------------------------------------------------
    // Installed and Updates
    // ----------------------------------------------------------------------------------

    fn app_row(&mut self, ui: &mut Ui, app: &CatApp, detail: &str) {
        let p = pal(ui);
        widgets::card(ui, |ui| {
            widgets::row(ui, 48.0, |ui| {
                let (rect, resp) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::click());
                let tex = self.icon(ui.ctx(), app);
                widgets::paint_icon(ui, rect, &app.name, app.accents().0, tex.as_ref());
                ui.add_space(6.0);
                let mut open = resp.clicked();
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add_space(3.0);
                    let name = ui.add(
                        egui::Label::new(RichText::new(&app.name).font(semibold(16.0)))
                            .sense(Sense::click()),
                    );
                    open |= name.clicked();
                    ui.label(RichText::new(detail).size(12.5).color(p.muted));
                });
                if open {
                    self.pending.push(Action::OpenPage(app.id.clone()));
                }
                widgets::right(ui, |ui| self.action_buttons(ui, app, false));
            });
        });
    }

    pub(super) fn installed_ui(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let apps = self.visible_apps(|s| matches!(s, AppState::Installed | AppState::Update));
        if apps.is_empty() {
            let (title, text) = if self.search.trim().is_empty() {
                (
                    "Nothing installed yet",
                    "Apps you install from Discover appear here.",
                )
            } else {
                ("No installed app matches", "Try another word.")
            };
            widgets::empty_state(ui, Icon::APPS, p.faint, title, text);
            return;
        }
        ui.label(
            RichText::new(format!(
                "{} of the family's apps are installed on this PC.",
                apps.len()
            ))
            .color(p.muted),
        );
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width() - 8.0);
                for app in &apps {
                    let detail = match self.installed.get(&app.id) {
                        Some(inst) => format!(
                            "Version {}  ·  {}  ·  {}",
                            inst.version,
                            if inst.per_user {
                                "this user"
                            } else {
                                "all users"
                            },
                            inst.location
                        ),
                        None => String::new(),
                    };
                    self.app_row(ui, app, &detail);
                }
            });
    }

    pub(super) fn updates_ui(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let apps = self.visible_apps(|s| s == AppState::Update);
        if apps.is_empty() {
            let text = if self.installed.is_empty() {
                "Install an app from Discover and its updates will show here."
            } else {
                "Every installed app is on its latest version."
            };
            widgets::empty_state(ui, Icon::DONE, p.ok, "Everything is up to date", text);
            return;
        }
        widgets::row(ui, 36.0, |ui| {
            ui.label(
                RichText::new(format!(
                    "{} update{} available. Each one downloads its installer and opens it.",
                    apps.len(),
                    if apps.len() == 1 { "" } else { "s" }
                ))
                .color(p.muted),
            );
            widgets::right(ui, |ui| {
                if widgets::primary_button(ui, &Icon::SYNC.with("Update all"), true).clicked() {
                    self.pending.push(Action::UpdateAll);
                }
            });
        });
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width() - 8.0);
                for app in &apps {
                    let detail = match (self.installed.get(&app.id), self.releases.get(&app.id)) {
                        (Some(inst), Some(rel)) => {
                            let size = rel
                                .asset(app, AssetKind::Setup)
                                .map(|a| format!("  ·  {}", human_size(a.size)))
                                .unwrap_or_default();
                            format!(
                                "{} → {}  ·  released {}{size}",
                                inst.version,
                                rel.version(),
                                rel.date()
                            )
                        }
                        _ => String::new(),
                    };
                    self.app_row(ui, app, &detail);
                }
            });
    }

    // ----------------------------------------------------------------------------------
    // Settings and About
    // ----------------------------------------------------------------------------------

    fn save_settings(&mut self) {
        if self.demo {
            return;
        }
        if let Err(e) = self.settings.save(&self.dirs) {
            self.toast(e, true);
        }
    }

    pub(super) fn settings_ui(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(760.0);
                widgets::card(ui, |ui| {
                    widgets::section(ui, "APPEARANCE");
                    ui.horizontal(|ui| {
                        for (choice, label) in [
                            (ThemeChoice::Dark, "Dark"),
                            (ThemeChoice::Light, "Light"),
                            (ThemeChoice::System, "Match Windows"),
                        ] {
                            if widgets::chip(ui, label, self.settings.theme == choice).clicked() {
                                self.settings.theme = choice;
                                theme::apply_choice(ui.ctx(), choice);
                                self.save_settings();
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                widgets::card(ui, |ui| {
                    widgets::section(ui, "UPDATES");
                    let mut check = self.settings.check_at_start;
                    if ui
                        .checkbox(&mut check, "Check for new versions when ForgeDock starts")
                        .changed()
                    {
                        self.settings.check_at_start = check;
                        self.save_settings();
                    }
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "Version information comes from GitHub and is kept for an hour, so \
                                 opening ForgeDock often does not use up GitHub's request limit.",
                            )
                            .color(p.muted),
                        )
                        .wrap(),
                    );
                });
                ui.add_space(4.0);
                widgets::card(ui, |ui| {
                    widgets::section(ui, "DOWNLOADS");
                    let folder = self.settings.portable_dir(&self.dirs);
                    ui.label("Portable ZIPs are saved to");
                    ui.add(
                        egui::Label::new(
                            RichText::new(folder.display().to_string()).color(p.accent),
                        )
                        .wrap(),
                    );
                    ui.horizontal(|ui| {
                        if widgets::ghost_button(ui, &Icon::FOLDER.with("Change folder…")).clicked()
                            && !self.demo
                        {
                            if let Some(dir) = rfd::FileDialog::new()
                                .set_title("Folder for portable ZIPs")
                                .set_directory(&folder)
                                .pick_folder()
                            {
                                self.settings.download_dir = Some(dir.display().to_string());
                                self.save_settings();
                            }
                        }
                        if widgets::ghost_button(ui, "Open folder").clicked() && !self.demo {
                            if let Err(e) = crate::actions::open_folder(&folder) {
                                self.toast(e, true);
                            }
                        }
                        if self.settings.download_dir.is_some()
                            && widgets::ghost_button(ui, "Use Downloads").clicked()
                        {
                            self.settings.download_dir = None;
                            self.save_settings();
                        }
                    });
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "Installers are downloaded to ForgeDock's own folder, checked \
                                 against the size and SHA-256 that GitHub publishes, and then opened.",
                            )
                            .color(p.muted),
                        )
                        .wrap(),
                    );
                });
                ui.add_space(4.0);
                widgets::card(ui, |ui| {
                    widgets::section(ui, "CACHE");
                    if self.cache_size.is_none() {
                        self.cache_size = Some(if self.demo {
                            7_340_032
                        } else {
                            crate::settings::dir_size(&self.dirs.cache())
                                + crate::settings::dir_size(&self.dirs.installers())
                        });
                    }
                    ui.label(format!(
                        "Saved catalogue, version answers, screenshots and downloaded installers: {}",
                        human_size(self.cache_size.unwrap_or(0))
                    ));
                    if widgets::ghost_button(ui, &Icon::DELETE.with("Clear cache")).clicked() {
                        if self.demo {
                            self.toast("Demo mode: nothing is deleted.", false);
                        } else if !self.jobs.is_empty() {
                            self.toast("Wait for the running downloads to finish first.", true);
                        } else {
                            let _ = std::fs::remove_dir_all(self.dirs.cache());
                            let _ = std::fs::remove_dir_all(self.dirs.installers());
                            self.cache_size = None;
                            self.toast("Cache cleared.", false);
                        }
                    }
                });
                ui.add_space(4.0);
                widgets::card(ui, |ui| {
                    widgets::section(ui, "PRIVACY");
                    ui.add(
                        egui::Label::new(
                            "ForgeDock contacts only GitHub: the catalogue, icons and screenshots \
                             from raw.githubusercontent.com, version information from \
                             api.github.com, and downloads from the publisher's releases on \
                             github.com. It sends nothing about you or this PC, has no account \
                             and no telemetry.",
                        )
                        .wrap(),
                    );
                });
                ui.add_space(10.0);
            });
    }

    pub(super) fn about_ui(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        ui.set_max_width(760.0);
        widgets::card(ui, |ui| {
            widgets::row(ui, 88.0, |ui| {
                if let Some(logo) = &self.logo {
                    ui.add(egui::Image::new(logo).fit_to_exact_size(vec2(88.0, 88.0)));
                }
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.add_space(10.0);
                    ui.label(RichText::new("ForgeDock").font(semibold(26.0)));
                    ui.label(
                        RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                            .color(p.muted),
                    );
                    ui.label(
                        RichText::new("The home of the Forge family of Windows apps.")
                            .color(p.muted),
                    );
                });
            });
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    "Browse the apps, read what they do, look at their screenshots, install them, \
                     keep them up to date and remove them again, all from one window.",
                )
                .wrap(),
            );
            ui.add_space(4.0);
            ui.label(RichText::new("Made by Zheer Khalid (zheer.dev)").font(semibold(15.0)));
            ui.label(
                RichText::new("Free and open source under the MIT licence. Written in Rust.")
                    .color(p.muted),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, &Icon::GLOBE.with("zheer.dev"), true).clicked() {
                    self.pending.push(Action::Url("https://zheer.dev".into()));
                }
                if widgets::ghost_button(ui, "GitHub").clicked() {
                    self.pending
                        .push(Action::Url("https://github.com/zheerkhalid".into()));
                }
                if widgets::ghost_button(ui, "ForgeDock source").clicked() {
                    self.pending.push(Action::Url(
                        "https://github.com/zheerkhalid/forgedock".into(),
                    ));
                }
            });
        });
        ui.add_space(4.0);
        widgets::card(ui, |ui| {
            widgets::section(ui, "CATALOGUE");
            let total = self.catalog.apps.len();
            let ready = self.catalog.apps.iter().filter(|a| a.available()).count();
            let source = if self.demo {
                "built-in demo data"
            } else {
                match self.source {
                    Source::Live => "downloaded from GitHub just now",
                    Source::Saved => "the copy saved on this PC",
                    Source::BuiltIn => "the copy built into this version",
                }
            };
            ui.label(format!(
                "{total} apps: {ready} available, {} coming soon. Source: {source}.",
                total - ready
            ));
        });
    }
}
