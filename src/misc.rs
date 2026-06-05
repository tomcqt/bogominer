pub fn parse_cpu_tier(s: &str) -> f64 {
    match s.to_lowercase().as_str() {
        "tiny" => 0.1,
        "low" => 0.25,
        "medium" | "med" => 0.5,
        "high" => 0.75,
        "max" => 1.,
        other => other.parse::<f64>().unwrap_or(1.).clamp(0.05, 1.),
    }
}

// replacement func for js fmtCompact
pub fn fmt_compact(n: u64) -> String {
    if n < 1_000 {
        return n.to_string();
    }

    if n < 1_000_000 {
        let v = n as f64 / 1_000.0;
        return if n < 10_000 {
            format!("{:.1}K", v)
        } else {
            format!("{:.0}K", v)
        };
    }

    if n < 1_000_000_000 {
        let v = n as f64 / 1_000_000.0;
        return if n < 10_000_000 {
            format!("{:.2}M", v)
        } else {
            format!("{:.1}M", v)
        };
    }

    if n < 1_000_000_000_000 {
        let v = n as f64 / 1_000_000_000.0;
        return if n < 10_000_000_000 {
            format!("{:.2}B", v)
        } else {
            format!("{:.1}B", v)
        };
    }

    let v = n as f64 / 1_000_000_000_000.0;
    format!("{:.2}T", v)
}

pub fn fmt_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

pub fn fmt_xp(n: u64) -> String {
    if n >= 1_000_000 {
        let v = n as f64 / 1_000_000.0;
        if n >= 10_000_000 {
            format!("{:.0}M", v)
        } else {
            format!("{:.1}M", v)
        }
    } else if n >= 1_000 {
        let v = n as f64 / 1_000.0;
        if n >= 10_000 {
            format!("{:.0}k", v)
        } else {
            format!("{:.1}k", v)
        }
    } else {
        n.to_string()
    }
}

pub fn parse_hex_color(hex: &str) -> ratatui::style::Color {
    use ratatui::style::Color;

    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return Color::White;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
    Color::Rgb(r, g, b)
}

pub fn hex_to_color32(hex: &str) -> eframe::egui::Color32 {
    use eframe::egui::Color32;
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return Color32::from_rgb(0xe8, 0xe4, 0xdb);
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
    Color32::from_rgb(r, g, b)
}
