pub fn _parse_cpu_tier(s: &str) -> f64 {
    match s.to_lowercase().as_str() {
        "tiny" => 0.1,
        "low" => 0.25,
        "medium" | "med" => 0.5,
        "high" => 0.75,
        "max" => 1.,
        other => other.parse::<f64>().unwrap_or(1.).clamp(0.05, 1.),
    }
}

pub fn validate_nick(nick: &str) -> Result<String, String> {
    let nick = nick.trim();
    if nick.len() < 2 {
        return Err("nickname must be at least 2 characters".into());
    }
    if nick.len() > 8 {
        return Err("nickname must be at most 8 characters".into());
    }
    Ok(nick.to_string())
}
