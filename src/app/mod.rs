mod commands;
mod state;

pub fn run() {
    print_startup_banner();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_state,
            commands::save_new_account,
            commands::save_existing_account,
            commands::clear_account,
            commands::start_mining,
            commands::stop_mining,
            commands::set_cpu_target,
            commands::get_runtime_stats,
            commands::get_contributors,
            commands::get_leaderboard,
            commands::open_external,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri app");
}

fn print_startup_banner() {
    let version = env!("CARGO_PKG_VERSION");
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let cores = num_cpus::get();

    eprintln!();
    eprintln!(" bogominer v{}", version);
    eprintln!(" ─────────────────────────────");
    eprintln!(" platform {} {}", os, arch);
    eprintln!(" cores {}", cores);
    eprintln!(
        " build {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );

    let config = crate::backend::config::Config::load();
    if let Some(ref nick) = config.nickname {
        eprintln!(" account {}", nick);
    } else {
        eprintln!(" account (none)");
    }
    if let Some(ref uuid) = config.uuid {
        eprintln!(
            " uuid {}...{}",
            &uuid[..8.min(uuid.len())],
            &uuid[uuid.len().saturating_sub(4)..]
        );
    }
    eprintln!(
        " recovery {}",
        if config.recovery_code.as_ref().is_some_and(|s| !s.is_empty()) {
            "saved"
        } else {
            "none"
        }
    );
    eprintln!(
        " config {:?}",
        crate::backend::config::config_path().unwrap_or_default()
    );
    eprintln!(" ─────────────────────────────");
    eprintln!();
}
