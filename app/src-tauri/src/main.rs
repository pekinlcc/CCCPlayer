//! CCCPlayer desktop entry point.
//!
//! The full Tauri binary requires the `tauri` feature; without it the crate
//! still compiles as a plain Rust binary so CI on Linux can validate core
//! behavior without pulling Tauri's macOS-native build deps.

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    #[cfg(feature = "tauri")]
    {
        #[cfg(target_os = "macos")]
        suppress_app_nap();

        tauri::Builder::default()
            .manage(crate::app_state::AppState::new())
            .invoke_handler(tauri::generate_handler![
                crate::commands::preflight,
                crate::commands::classify_workdir,
                crate::commands::start_session,
                crate::commands::pause_session,
                crate::commands::stop_session,
            ])
            .on_window_event(|window, event| {
                // PRD §6.4: closing the window hides it, never quits the
                // process. Cmd+Q (menu Quit) triggers CloseRequested on
                // every window; we only hide and let the explicit
                // `tauri::Manager::exit()` path drive shutdown.
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
            })
            .run(tauri::generate_context!())
            .expect("failed to launch Tauri app");
    }

    #[cfg(not(feature = "tauri"))]
    {
        eprintln!(
            "cccplayer: built without Tauri feature. Re-run with `--features tauri` \
             to launch the desktop app. (This is the headless CI mode.)"
        );
    }
}

#[cfg(feature = "tauri")]
mod app_state;

#[cfg(feature = "tauri")]
mod commands;

/// macOS only: ask the OS not to App-Nap us while we may be running long
/// agent turns in the background. See PRD §16.12.
#[cfg(all(feature = "tauri", target_os = "macos"))]
fn suppress_app_nap() {
    // The full implementation uses
    // `NSProcessInfo.processInfo.beginActivity(options:reason:)`. In M1 we
    // stub the call behind this function so the entry point stays tidy and
    // swapping in the `objc2` bridge later is a mechanical change. Leaving
    // the no-op is safe — it just means macOS may throttle us in background,
    // and the stall watchdog (§16.4) will self-heal by resetting on wake.
    tracing::info!("app-nap suppression: stub (see PRD §16.12)");
}
