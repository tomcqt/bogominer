use crate::api;
use crate::config::Config;
use crate::misc::{fmt_commas, fmt_compact, fmt_xp, parse_hex_color};
use crate::pool::Pool;
use crate::stats::{self, Stats};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Row, Table},
    Frame, Terminal,
};
use std::io;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct App {
    config: Config,
    stats: Arc<Stats>,
    pool: Option<Pool>,
    cpu_target: f64,
    running: bool,
    error_msg: Option<String>,
    status_msg: String,
    leaderboard: Vec<api::LeaderboardEntry>,
    last_lb_fetch: Instant,
    last_tick: Instant,
    should_quit: bool,
    nick_input: String,
    onboard_error: Option<String>,
    onboarded: bool,
}

impl App {
    fn new(cpu_target: f64) -> Self {
        let config = Config::load();
        let stats = Arc::new(Stats::new());
        Self {
            onboarded: config.has_credentials(),
            config,
            stats,
            pool: None,
            cpu_target,
            running: false,
            error_msg: None,
            status_msg: String::new(),
            leaderboard: Vec::new(),
            last_lb_fetch: Instant::now() - Duration::from_secs(60),
            last_tick: Instant::now(),
            should_quit: false,
            nick_input: String::new(),
            onboard_error: None,
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

    fn cycle_cpu(&mut self) {
        self.cpu_target = match self.cpu_label() {
            "tiny" => 0.25,
            "low" => 0.5,
            "medium" => 0.75,
            "high" => 1.0,
            _ => 0.1,
        };
        if self.running {
            if let Some(pool) = &mut self.pool {
                let uuid = self.config.uuid.clone().unwrap_or_default();
                let nick = self.config.nickname.clone().unwrap_or_default();
                let code = self.config.recovery_code.clone().unwrap_or_default();
                pool.set_cpu_target(self.cpu_target, &uuid, &nick, &code);
            }
        }
    }
}

pub fn run(cpu_target: f64, code: Option<String>, autostart: bool) {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

    let mut app = App::new(cpu_target);

    if let Some(c) = code {
        app.config.recovery_code = Some(c);
        app.config.save();
    }

    if app.config.has_credentials() {
        app.onboarded = true;
        app.status_msg = format!("ready as {}", app.config.nickname.as_deref().unwrap_or("?"));
    }

    if autostart && app.onboarded {
        start_mining(&mut app, &rt);
    }

    enable_raw_mode().expect("failed to enable raw mode");
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).expect("failed to enter alternate screen");
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).expect("failed to create terminal");

    let tick_rate = Duration::from_millis(const { 69 - 2 });

    while !app.should_quit {
        terminal.draw(|f| draw_ui(f, &app)).expect("failed to draw");

        let timeout = tick_rate.saturating_sub(app.last_tick.elapsed());
        if event::poll(timeout).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                handle_key(&mut app, key.code, &rt);
            }
        }

        if app.last_tick.elapsed() >= Duration::from_secs(1) {
            app.stats.tick_second();
            app.last_tick = Instant::now();
        }

        if let Some(pool) = &mut app.pool {
            if let Some(err) = pool.poll_error() {
                app.error_msg = Some(err);
                if !pool.is_running() {
                    app.running = false;
                    app.status_msg = "stopped (all workers exited)".into();
                }
            }

            if let Some(rc) = pool.poll_recovery_code() {
                app.config.recovery_code = Some(rc);
                app.config.save();
                app.status_msg = "recovery code saved".into();
            }
        }

        if app.last_lb_fetch.elapsed() >= Duration::from_secs(10) {
            app.last_lb_fetch = Instant::now();
            if let Ok(lb) = rt.block_on(api::get_leaderboard(20)) {
                app.leaderboard = lb;
            }
        }
    }

    if let Some(mut pool) = app.pool.take() {
        pool.stop();
    }
    disable_raw_mode().expect("failed to disable raw mode");
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .expect("failed to leave alternate screen");
}

fn start_mining(app: &mut App, rt: &tokio::runtime::Runtime) {
    let uuid = app.config.uuid.clone().unwrap_or_default();
    let nick = app.config.nickname.clone().unwrap_or_default();
    let code = app.config.recovery_code.clone().unwrap_or_default();

    let mut pool = Pool::new(app.stats.clone());
    let _guard = rt.enter();
    pool.start(&uuid, &nick, &code, app.cpu_target);
    app.pool = Some(pool);
    app.running = true;
    app.error_msg = None;
    app.status_msg = "mining...".into();
}

fn stop_mining(app: &mut App) {
    if let Some(mut pool) = app.pool.take() {
        pool.stop();
    }
    app.running = false;
    app.status_msg = "stopped".into();
}

fn handle_key(app: &mut App, key: KeyCode, rt: &tokio::runtime::Runtime) {
    if !app.onboarded {
        match key {
            KeyCode::Char(c) => {
                if app.nick_input.len() < 8 && c.is_ascii() && !c.is_whitespace() {
                    app.nick_input.push(c);
                }
            }
            KeyCode::Backspace => {
                app.nick_input.pop();
            }
            KeyCode::Enter => {
                let nick = app.nick_input.trim().to_string();
                app.onboard_error = None;
                if nick.len() < 2 {
                    app.onboard_error = Some("nickname must be at least 2 chararcters".into());
                    return;
                }
                if nick.len() > 8 {
                    app.onboard_error = Some("nickname must be at most 8 characters".into());
                    return;
                }
                if app.config.uuid.is_none() {
                    app.config.uuid = Some(api::generate_uuid());
                }
                app.config.nickname = Some(nick);
                app.config.save();
                app.onboarded = true;
                app.status_msg = format!(
                    "ready as {} - press [s] to start",
                    app.config.nickname.as_deref().unwrap_or("?")
                );
                let _ = rt;
            }
            KeyCode::Esc => {
                app.should_quit = true;
            }
            _ => {}
        }
        return;
    }

    match key {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.should_quit = true;
        }
        KeyCode::Char('s') | KeyCode::Enter => {
            if app.running {
                stop_mining(app);
            } else {
                start_mining(app, rt);
            }
        }
        KeyCode::Char('c') => {
            app.cycle_cpu();
        }
        KeyCode::Char('l') => {
            if app.running {
                stop_mining(app);
            }
            app.config.clear();
            app.onboarded = false;
            app.nick_input.clear();
            app.onboard_error = None;
        }
        _ => {}
    }
}

fn draw_ui(f: &mut Frame, app: &App) {
    let area = f.area();

    if !app.onboarded {
        draw_onboard(f, area, app);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Length(5), // account + tier
            Constraint::Length(9), // stats
            Constraint::Min(8),    // leaderboard
            Constraint::Length(3), // controls
        ])
        .split(area);

    draw_header(f, chunks[0]);
    draw_account(f, chunks[1], app);
    draw_stats(f, chunks[2], app);
    draw_leaderboard(f, chunks[3], app);
    draw_controls(f, chunks[4], app);
}

fn draw_header(f: &mut Frame, area: Rect) {
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "bogo",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "miner.",
            Style::default()
                .fg(Color::Rgb(218, 118, 86))
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(header, area);
}

fn draw_onboard(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    draw_header(f, chunks[0]);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // spacer
            Constraint::Length(2), // title
            Constraint::Length(2), // subtitle
            Constraint::Length(3), // input
            Constraint::Length(2), // error
            Constraint::Min(0),    // spacer
        ])
        .horizontal_margin(4)
        .split(chunks[1]);

    let title = Paragraph::new("pick a nickname").style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(title, inner[1]);

    let subtitle =
        Paragraph::new("format: xxxx-xxxx-xxxx-xxxx").style(Style::default().fg(Color::DarkGray));
    f.render_widget(subtitle, inner[2]);

    let input = Paragraph::new(app.login_input.as_str())
        .style(Style::default().fg(Color::Rgb(218, 118, 86)))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" code "),
        );
    f.render_widget(input, inner[3]);

    if let Some(err) = &app.login_error {
        let err_p = Paragraph::new(err.as_str()).style(Style::default().fg(Color::Red));
        f.render_widget(err_p, inner[4]);
    }
}

fn draw_account(f: &mut Frame, area: Rect, app: &App) {
    let nick = app.config.nickname.as_deref().unwrap_or("?");
    let lifetime = app.stats.lifetime_shuffles.load(Ordering::Relaxed);
    let tier = stats::rank_info(lifetime);

    let tier_display = if tier.stars > 0 {
        format!("{} \u{2726}{}", tier.name, tier.stars)
    } else {
        tier.name.to_string()
    };

    let status_indicator = if app.running {
        "\u{25cf} mining"
    } else {
        "\u{25cb} idle"
    };
    let status_color = if app.running {
        Color::Rgb(74, 158, 106)
    } else {
        Color::DarkGray
    };

    let text = vec![
        Line::from(vec![
            Span::styled("contributing as ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                nick,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(status_indicator, Style::default().fg(status_color)),
        ]),
        Line::from(vec![
            Span::styled(
                tier_display,
                Style::default()
                    .fg(parse_hex_color(tier.color_hex))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {} xp", fmt_commas(tier.xp)),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("  {} xp to {}", fmt_xp(tier.rem_xp), tier.next_label),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
    ];

    let block = Block::default().borders(Borders::NONE);
    let para = Paragraph::new(text).block(block);
    f.render_widget(
        para,
        Rect {
            height: area.height.saturating_sub(1),
            ..area
        },
    );

    let gauge_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };
    let gauge = Gauge::default()
        .gauge_style(
            Style::default()
                .fg(parse_hex_color(tier.color_hex))
                .bg(Color::Rgb(50, 50, 50)),
        )
        .ratio((tier.pct / 100.0).clamp(0.0, 1.0))
        .label(format!("{:.1}%", tier.pct));
    f.render_widget(gauge, gauge_area);
}

fn draw_stats(f: &mut Frame, area: Rect, app: &App) {
    let rate = app.stats.rate.load(Ordering::Relaxed);
    let session = app.stats.session_shuffles.load(Ordering::Relaxed);
    let lifetime = app.stats.lifetime_shuffles.load(Ordering::Relaxed);
    let tick_best = app.stats.tick_best.load(Ordering::Relaxed);
    let session_best = app.stats.session_best.load(Ordering::Relaxed);
    let all_time_best = app.stats.all_time_best.load(Ordering::Relaxed);
    let workers = app.stats.active_workers.load(Ordering::Relaxed);
    let last5 = app.stats.get_last5();

    let last5_str = if last5.is_empty() {
        "\u{2014}".to_string()
    } else {
        last5
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" ")
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

    let accent = Color::Rgb(218, 118, 86);
    let dim = Color::DarkGray;
    let white = Color::White;

    let rows = vec![
        Row::new(vec![
            format!("rate: {}/s", fmt_compact(rate)),
            format!("session: {}", fmt_compact(session)),
            format!("lifetime: {}", fmt_compact(lifetime)),
        ]),
        Row::new(vec![
            format!("1s best: {}", tick_str),
            format!("session best: {}", session_best_str),
            format!("all-time: {}", atb_str),
        ]),
        Row::new(vec![
            format!("workers: {}", workers),
            format!("cpu: {}", app.cpu_label()),
            format!("last 5: {}", last5_str),
        ]),
    ];

    let widths = [
        Constraint::Percentage(33),
        Constraint::Percentage(33),
        Constraint::Percentage(34),
    ];

    let table = Table::new(rows, widths)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(dim))
                .title(Span::styled(" stats ", Style::default().fg(accent))),
        )
        .style(Style::default().fg(white))
        .row_highlight_style(Style::default());

    f.render_widget(table, area);
}

fn draw_leaderboard(f: &mut Frame, area: Rect, app: &App) {
    let accent = Color::Rgb(218, 118, 86);
    let nick = app.config.nickname.as_deref().unwrap_or("");

    if app.leaderboard.is_empty() {
        let empty = Paragraph::new("loading leaderboard...")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(Span::styled(" leaderboard ", Style::default().fg(accent))),
            );
        f.render_widget(empty, area);
        return;
    }

    let rows: Vec<Row> = app
        .leaderboard
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let pos = match i {
                0 => "\u{1f947}".to_string(),
                1 => "\u{1f948}".to_string(),
                2 => "\u{1f949}".to_string(),
                n => format!("{}", n + 1),
            };
            let tier = stats::rank_info(entry.total);
            let tier_label = if tier.stars > 0 {
                format!("{} \u{2726}{}", tier.name, tier.stars)
            } else {
                tier.name.to_string()
            };
            let is_me = entry.nickname == nick;
            let style = if is_me {
                Style::default().fg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let suffix = if is_me { " \u{2190} you" } else { "" };

            Row::new(vec![
                pos,
                format!("{}{}", entry.nickname, suffix),
                tier_label,
                fmt_compact(entry.total),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(4),
        Constraint::Min(10),
        Constraint::Length(14),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["#", "name", "rank", "shuffles"])
                .style(
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )
                .bottom_margin(0),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(" leaderboard ", Style::default().fg(accent))),
        );

    f.render_widget(table, area);
}

fn draw_controls(f: &mut Frame, area: Rect, app: &App) {
    let running = app.running;
    let accent = Color::Rgb(218, 118, 86);

    let mut spans = vec![
        Span::styled(
            " [s] ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw(if running { "stop" } else { "start" }),
        Span::raw("  "),
        Span::styled(
            " [c] ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("cpu: {}", app.cpu_label())),
        Span::raw("  "),
        Span::styled(
            " [l] ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw("logout"),
        Span::raw("  "),
        Span::styled(
            " [q] ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw("quit"),
    ];

    if let Some(err) = &app.error_msg {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(err.as_str(), Style::default().fg(Color::Red)));
    }

    let controls = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(controls, area);
}
