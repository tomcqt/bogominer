use crate::api;
use crate::config::Config;
use crate::misc::{fmt_commas, fmt_compact, fmt_xp, hex_to_color32};
use crate::pool::Pool;
use crate::stats::{self, Stats};
use eframe::egui;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

const BG: egui::Color32 = egui::Color32::from_rgb(0x1a, 0x1a, 0x1a);
const CARD: egui::Color32 = egui::Color32::from_rgb(0x24, 0x24, 0x24);
const TEXT: egui::Color32 = egui::Color32::from_rgb(0xe8, 0xe4, 0xdb);
const MUTED: egui::Color32 = egui::Color32::from_rgb(0x8a, 0x84, 0x7a);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0xe0, 0x86, 0x68);
const GREEN: egui::Color32 = egui::Color32::from_rgb(0x5b, 0xb4, 0x7e);
const DANGER: egui::Color32 = egui::Color32::from_rgb(0xe8, 0x75, 0x60);
const EMPTY: egui::Color32 = egui::Color32::from_rgb(0x2e, 0x2e, 0x2e);
const BORDER: egui::Color32 = egui::Color32::from_rgb(0x3a, 0x3a, 0x3a);

struct App {
    config: Config,
    stats: Arc<Stats>,
    pool: Option<Pool>,
    rt: tokio::runtime::Runtime,
    cpu_target: f64,
    running: bool,
    autostart: bool,
    error_msg: Option<String>,
    login_input: String,
    login_error: Option<String>,
    logged_in: bool,
    login_pending: bool,
    leaderboard: Vec<api::LeaderboardEntry>,
    last_lb_fetch: Instant,
    last_tick: Instant,
}

impl App {
    fn new(cpu_target: f64, code: Option<String>, autostart: bool) -> Self {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        let mut config = Config::load();
        let stats = Arc::new(Stats::new());

        if let Some(c) = code {
            config.code = Some(c);
            config.save();
        }

        let mut logged_in = false;
        let mut login_error = None;

        if config.has_credentials() {
            let code = config.code.clone().unwrap();
            match rt.block_on(api::login(&code)) {
                Ok(info) => {
                    config.uuid = Some(info.uuid);
                    config.nickname = Some(info.nickname);
                    config.save();
                    stats.lifetime_shuffles.store(info.total, Ordering::Relaxed);
                    stats
                        .all_time_best
                        .store(info.all_time_best as i32, Ordering::Relaxed);
                    logged_in = true;
                }
                Err(e) => {
                    login_error = Some(e);
                }
            }
        }

        Self {
            config,
            stats,
            pool: None,
            rt,
            cpu_target,
            running: false,
            autostart: autostart && logged_in,
            error_msg: login_error.clone(),
            login_input: String::new(),
            login_error,
            logged_in,
            login_pending: false,
            leaderboard: Vec::new(),
            last_lb_fetch: Instant::now() - Duration::from_secs(60),
            last_tick: Instant::now(),
        }
    }

    fn cpu_label(&self) -> &str {
        if self.cpu_target <= 0.1 {
            "tiny"
        } else if self.cpu_target <= 0.25 {
            "low"
        } else if self.cpu_target <= 0.5 {
            "medium"
        } else if self.cpu_target <= 0.75 {
            "high"
        } else {
            "max"
        }
    }

    fn start_mining(&mut self) {
        let uuid = self.config.uuid.clone().unwrap_or_default();
        let nick = self.config.nickname.clone().unwrap_or_default();
        let code = self.config.code.clone().unwrap_or_default();

        let mut pool = Pool::new(self.stats.clone());
        let _guard = self.rt.enter();
        pool.start(&uuid, &nick, &code, self.cpu_target);
        self.pool = Some(pool);
        self.running = true;
        self.error_msg = None;
    }

    fn stop_mining(&mut self) {
        if let Some(mut pool) = self.pool.take() {
            pool.stop();
        }
        self.running = false;
    }

    fn do_login(&mut self) {
        let code = self.login_input.trim().to_lowercase();
        self.login_error = None;
        match self.rt.block_on(api::login(&code)) {
            Ok(info) => {
                self.config.code = Some(code);
                self.config.uuid = Some(info.uuid);
                self.config.nickname = Some(info.nickname);
                self.config.save();
                self.stats
                    .lifetime_shuffles
                    .store(info.total, Ordering::Relaxed);
                self.stats
                    .all_time_best
                    .store(info.all_time_best as i32, Ordering::Relaxed);
                self.logged_in = true;
            }
            Err(e) => {
                self.login_error = Some(e);
            }
        }
    }

    fn logout(&mut self) {
        self.stop_mining();
        self.config.clear();
        self.logged_in = false;
        self.login_input.clear();
        self.login_error = None;
    }

    fn set_cpu_target(&mut self, target: f64) {
        self.cpu_target = target;
        if self.running {
            if let Some(pool) = &mut self.pool {
                let uuid = self.config.uuid.clone().unwrap_or_default();
                let nick = self.config.nickname.clone().unwrap_or_default();
                let code = self.config.code.clone().unwrap_or_default();
                pool.set_cpu_target(target, &uuid, &nick, &code);
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.autostart {
            self.autostart = false;
            self.start_mining();
        }

        if self.last_tick.elapsed() >= Duration::from_secs(1) {
            self.stats.tick_second();
            self.last_tick = Instant::now();
        }

        if let Some(pool) = &mut self.pool {
            if let Some(err) = pool.poll_error() {
                self.error_msg = Some(err);
                if !pool.is_running() {
                    self.running = false;
                }
            }
        }

        if self.last_lb_fetch.elapsed() >= Duration::from_secs(10) {
            self.last_lb_fetch = Instant::now();
            if let Ok(lb) = self.rt.block_on(api::get_leaderboard(20)) {
                self.leaderboard = lb;
            }
        }

        let repaint_interval = if self.running {
            Duration::from_millis(const { 69 - 2 })
        } else {
            Duration::from_millis(250)
        };
        ctx.request_repaint_after(repaint_interval);

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = CARD;
        visuals.override_text_color = Some(TEXT);
        visuals.widgets.noninteractive.bg_fill = CARD;
        visuals.widgets.inactive.bg_fill = EMPTY;
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x35, 0x35, 0x35);
        visuals.widgets.active.bg_fill = ACCENT;
        ctx.set_visuals(visuals);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG).inner_margin(0.))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let margin = 24.0;
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space(margin);
                        let title = egui::RichText::new("bogominer.")
                            .size(22.0)
                            .strong()
                            .color(TEXT);
                        ui.label(title);
                    });
                    ui.add_space(12.0);

                    let rect = ui.available_rect_before_wrap();
                    let line_rect =
                        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 1.0));
                    ui.painter().rect_filled(line_rect, 0.0, BORDER);
                    ui.add_space(1.0);

                    if !self.logged_in {
                        self.draw_login(ui, margin);
                    } else {
                        self.draw_main(ui, margin);
                    }
                });
            });
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let inter = egui::FontData::from_static(include_bytes!("../assets/fonts/Inter.ttf"));

    let jetbrainsmono =
        egui::FontData::from_static(include_bytes!("../assets/fonts/JetBrainsMono.ttf"));

    fonts.font_data.insert("Inter".to_owned(), inter);
    fonts
        .font_data
        .insert("JetBrainsMono".to_owned(), jetbrainsmono);

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter".to_owned());

    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "JetBrainsMono".to_owned());

    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();

    use egui::FontId;
    style.text_styles = [
        (
            egui::TextStyle::Heading,
            FontId::new(20.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Body,
            FontId::new(14.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Monospace,
            FontId::new(14.0, egui::FontFamily::Monospace),
        ),
        (
            egui::TextStyle::Button,
            FontId::new(14.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Small,
            FontId::new(11.0, egui::FontFamily::Proportional),
        ),
    ]
    .into();

    ctx.set_style(style);
}

impl App {
    fn draw_login(&mut self, ui: &mut egui::Ui, margin: f32) {
        ui.add_space(32.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("enter your account code")
                        .size(20.0)
                        .strong()
                        .color(TEXT),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("format: xxxx-xxxx-xxxx-xxxx")
                        .size(13.0)
                        .color(MUTED),
                );
                ui.add_space(16.0);

                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.login_input)
                        .desired_width(280.0)
                        .font(egui::TextStyle::Monospace)
                        .hint_text("xxxx-xxxx-xxxx-xxxx"),
                );

                let raw: String = self.login_input.chars().filter(|c| *c != '-').collect();
                if raw.len() <= 16 {
                    let mut formatted = String::new();
                    for (i, c) in raw.chars().enumerate() {
                        if i > 0 && i % 4 == 0 {
                            formatted.push('-');
                        }
                        formatted.push(c);
                    }
                    if formatted != self.login_input {
                        self.login_input = formatted;
                    }
                }

                ui.add_space(12.0);

                if let Some(err) = &self.login_error {
                    ui.label(egui::RichText::new(err.as_str()).size(13.0).color(DANGER));
                    ui.add_space(8.0);
                }

                let btn = egui::Button::new(
                    egui::RichText::new("continue \u{2192}")
                        .size(14.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                )
                .fill(ACCENT)
                .rounding(20.0)
                .min_size(egui::vec2(120.0, 36.0));

                let enter_pressed =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                if ui.add(btn).clicked() || enter_pressed {
                    self.do_login();
                }
            });
        });
    }

    fn draw_main(&mut self, ui: &mut egui::Ui, margin: f32) {
        ui.add_space(20.0);

        // account info
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("CONTRIBUTING AS")
                            .size(11.0)
                            .color(MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("switch account")
                                        .size(12.0)
                                        .color(MUTED),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            self.logout();
                            return;
                        }
                    });
                });

                let nick = self.config.nickname.as_deref().unwrap_or("?");
                ui.label(egui::RichText::new(nick).size(36.0).strong().color(TEXT));

                // tier
                let lifetime = self.stats.lifetime_shuffles.load(Ordering::Relaxed);
                let tier = stats::rank_info(lifetime);
                let tier_label = if tier.stars > 0 {
                    format!("{} \u{2726}{}", tier.name, tier.stars)
                } else {
                    tier.name.to_string()
                };
                let tier_color = hex_to_color32(tier.color_hex);

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&tier_label)
                            .size(18.0)
                            .strong()
                            .color(tier_color),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(format!("{} xp", fmt_commas(tier.xp)))
                            .size(13.0)
                            .color(MUTED)
                            .family(egui::FontFamily::Monospace),
                    );
                });

                // progress bar
                ui.add_space(6.0);
                let bar_width = (ui.available_width() - margin).max(100.0);
                let (bar_rect, _) =
                    ui.allocate_exact_size(egui::vec2(bar_width, 6.0), egui::Sense::hover());
                ui.painter().rect_filled(bar_rect, 3.0, EMPTY);
                let fill_width = bar_rect.width() * (tier.pct as f32 / 100.0);
                let fill_rect = egui::Rect::from_min_size(
                    bar_rect.min,
                    egui::vec2(fill_width, bar_rect.height()),
                );
                ui.painter().rect_filled(fill_rect, 3.0, tier_color);

                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} xp to {}",
                        fmt_xp(tier.rem_xp),
                        tier.next_label
                    ))
                    .size(12.0)
                    .color(MUTED),
                );
            });
        });

        ui.add_space(20.0);
        // divider
        let rect = ui.available_rect_before_wrap();
        let line_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 1.0));
        ui.painter().rect_filled(line_rect, 0.0, BORDER);
        ui.add_space(1.0);
        ui.add_space(16.0);

        // controls
        ui.horizontal(|ui| {
            ui.add_space(margin);

            if self.running {
                // contributing
                let pill = egui::Frame::none()
                    .fill(egui::Color32::from_rgba_premultiplied(
                        0x1a, 0x3d, 0x28, 0xff,
                    ))
                    .rounding(20.0)
                    .inner_margin(egui::Margin::symmetric(14.0, 8.0));
                pill.show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("\u{25cf} contributing live")
                            .size(13.0)
                            .strong()
                            .color(GREEN),
                    );
                });

                ui.add_space(12.0);

                // stop
                let stop_btn = egui::Button::new(
                    egui::RichText::new("\u{25a0} stop")
                        .size(13.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                )
                .fill(DANGER)
                .rounding(20.0)
                .min_size(egui::vec2(70.0, 32.0));

                if ui.add(stop_btn).clicked() {
                    self.stop_mining();
                }
            } else {
                let start_btn = egui::Button::new(
                    egui::RichText::new("start bogosorting \u{2192}")
                        .size(15.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                )
                .fill(ACCENT)
                .rounding(20.0)
                .min_size(egui::vec2(200.0, 40.0));

                if ui.add(start_btn).clicked {
                    self.start_mining();
                }
            }
        });

        // cpu tier picker
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.label(egui::RichText::new("CPU").size(11.0).color(MUTED));
            ui.add_space(8.0);

            let tiers = [
                ("tiny", 0.1),
                ("low", 0.25),
                ("medium", 0.5),
                ("high", 0.75),
                ("max", 1.0),
            ];
            for (label, val) in tiers {
                let is_active = (self.cpu_target - val).abs() < 0.01;
                let (bg, fg) = if is_active {
                    (ACCENT, egui::Color32::WHITE)
                } else {
                    (EMPTY, MUTED)
                };
                let btn =
                    egui::Button::new(egui::RichText::new(label).size(11.0).strong().color(fg))
                        .fill(bg)
                        .rounding(6.0)
                        .min_size(egui::vec2(44.0, 24.0));

                if ui.add(btn).clicked() {
                    self.set_cpu_target(val);
                }
                ui.add_space(4.0);
            }
        });

        // errors
        if let Some(err) = &self.error_msg {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(margin);
                ui.label(egui::RichText::new(err.as_str()).size(13.0).color(DANGER));
            });
        }

        // stats
        if self.running {
            ui.add_space(20.0);

            let rect = ui.available_rect_before_wrap();
            let line_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 1.0));
            ui.painter().rect_filled(line_rect, 0.0, BORDER);
            ui.add_space(1.0);
            ui.add_space(16.0);

            self.draw_stats_grid(ui, margin);
        }

        // leaderboard
        ui.add_space(20.0);
        let rect = ui.available_rect_before_wrap();
        let line_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 1.0));
        ui.painter().rect_filled(line_rect, 0.0, BORDER);
        ui.add_space(1.0);
        ui.add_space(16.0);

        self.draw_leaderboard(ui, margin);
        ui.add_space(24.0);
    }

    fn draw_stats_grid(&self, ui: &mut egui::Ui, margin: f32) {
        let rate = self.stats.rate.load(Ordering::Relaxed);
        let session = self.stats.session_shuffles.load(Ordering::Relaxed);
        let lifetime = self.stats.lifetime_shuffles.load(Ordering::Relaxed);
        let tick_best = self.stats.tick_best.load(Ordering::Relaxed);
        let session_best = self.stats.session_best.load(Ordering::Relaxed);
        let all_time_best = self.stats.all_time_best.load(Ordering::Relaxed);
        let workers = self.stats.active_workers.load(Ordering::Relaxed);
        let last5 = self.stats.get_last5();

        let last5_str = if last5.is_empty() {
            "\u{2014}".to_string()
        } else {
            last5
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join("  ")
        };

        let tick_str = if tick_best >= 0 {
            format!("{}/25", tick_best)
        } else {
            "\u{2014}/25".to_string()
        };
        let session_best_str = if session_best >= 0 {
            format!("{}/25", session_best)
        } else {
            "\u{2014}/25".to_string()
        };
        let atb_str = if all_time_best > 0 {
            format!("{}/25", all_time_best)
        } else {
            "\u{2014}/25".to_string()
        };

        ui.horizontal(|ui| {
            ui.add_space(margin);
            stat_block(ui, "YOUR RATE", &format!("{}/s", fmt_compact(rate)));
            ui.add_space(32.0);
            stat_block(ui, "SESSION", &fmt_compact(session));
            ui.add_space(32.0);
            stat_block(ui, "LIFETIME", &fmt_compact(lifetime));
        });
        ui.add_space(14.0);

        ui.horizontal(|ui| {
            ui.add_space(margin);
            stat_block(ui, "1s BEST", &tick_str);
            ui.add_space(32.0);
            stat_block(ui, "SESSION BEST", &session_best_str);
            ui.add_space(32.0);
            stat_block(ui, "ALL-TIME", &atb_str);
        });
        ui.add_space(14.0);

        ui.horizontal(|ui| {
            ui.add_space(margin);
            stat_block(ui, "WORKERS", &workers.to_string());
            ui.add_space(32.0);
            stat_block(ui, "CPU TIER", self.cpu_label());
            ui.add_space(32.0);
            stat_block(ui, "LAST 5", &last5_str);
        });
    }

    fn draw_leaderboard(&self, ui: &mut egui::Ui, margin: f32) {
        let nick = self.config.nickname.as_deref().unwrap_or("");

        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.label(egui::RichText::new("LEADERBOARD").size(11.0).color(MUTED));
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!(
                    "top {} \u{00b7} total shuffles",
                    self.leaderboard.len()
                ))
                .size(11.0)
                .color(MUTED),
            );
        });
        ui.add_space(12.0);

        if self.leaderboard.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(margin);
                ui.label(egui::RichText::new("loading...").size(13.0).color(MUTED));
            });
            return;
        }

        // header
        ui.horizontal(|ui| {
            ui.add_space(margin);
            let w_pos = 30.0;
            let w_name = 120.0;
            let w_rank = 100.0;

            ui.allocate_ui(egui::vec2(w_pos, 16.0), |ui| {
                ui.label(egui::RichText::new("#").size(10.0).strong().color(MUTED));
            });
            ui.allocate_ui(egui::vec2(w_name, 16.0), |ui| {
                ui.label(egui::RichText::new("NAME").size(10.0).strong().color(MUTED));
            });
            ui.allocate_ui(egui::vec2(w_rank, 16.0), |ui| {
                ui.label(egui::RichText::new("RANK").size(10.0).strong().color(MUTED));
            });
            ui.label(
                egui::RichText::new("SHUFFLES")
                    .size(10.0)
                    .strong()
                    .color(MUTED),
            );
        });

        ui.add_space(4.0);

        // rows
        for (i, entry) in self.leaderboard.iter().enumerate() {
            let is_me = entry.nickname == nick;
            let tier = stats::rank_info(entry.total);
            let tier_label = if tier.stars > 0 {
                format!("{} \u{2726}{}", tier.name, tier.stars)
            } else {
                tier.name.to_string()
            };
            let tier_color = hex_to_color32(tier.color_hex);

            let pos_str = match i {
                0 => "\u{1f947}".to_string(),
                1 => "\u{1f948}".to_string(),
                2 => "\u{1f949}".to_string(),
                n => format!("{}", n + 1),
            };

            let row_color = if is_me {
                egui::Color32::from_rgba_premultiplied(0xe0, 0x86, 0x68, 0x1a)
            } else {
                egui::Color32::TRANSPARENT
            };

            let name_color = if is_me { ACCENT } else { TEXT };

            // row bg
            let row_rect = ui.available_rect_before_wrap();
            let row_rect =
                egui::Rect::from_min_size(row_rect.min, egui::vec2(row_rect.width(), 24.0));
            ui.painter().rect_filled(row_rect, 4.0, row_color);

            ui.horizontal(|ui| {
                ui.add_space(margin);
                let w_pos = 30.0;
                let w_name = 120.0;
                let w_rank = 100.0;

                ui.allocate_ui(egui::vec2(w_pos, 20.0), |ui| {
                    ui.label(
                        egui::RichText::new(&pos_str)
                            .size(13.0)
                            .strong()
                            .color(ACCENT),
                    );
                });
                ui.allocate_ui(egui::vec2(w_name, 20.0), |ui| {
                    let mut name_text = egui::RichText::new(&entry.nickname)
                        .size(13.0)
                        .color(name_color);
                    if is_me {
                        name_text = name_text.strong();
                    }
                    ui.label(name_text);
                });
                ui.allocate_ui(egui::vec2(w_rank, 20.0), |ui| {
                    ui.label(
                        egui::RichText::new(&tier_label)
                            .size(11.0)
                            .color(tier_color),
                    );
                });
                ui.label(
                    egui::RichText::new(fmt_compact(entry.total))
                        .size(13.0)
                        .strong()
                        .color(TEXT)
                        .family(egui::FontFamily::Monospace),
                );
            });
            ui.add_space(2.0);
        }
    }
}

fn stat_block(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).size(10.0).color(MUTED));
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(value)
                .size(20.0)
                .strong()
                .color(TEXT)
                .family(egui::FontFamily::Monospace),
        );
    });
}

pub fn run(cpu_target: f64, code: Option<String>, autostart: bool) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("bogominer. {}", env!("CARGO_PKG_VERSION")))
            .with_inner_size([480.0, 720.0])
            .with_min_inner_size([360.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "bogominer",
        options,
        Box::new(move |cc| {
            let app = App::new(cpu_target, code, autostart);
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .expect("failed to run egui");
}
