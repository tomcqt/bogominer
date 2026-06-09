mod commands;
mod state;

pub fn run() {
    tauri::Builder::default()
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
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri app");
}
