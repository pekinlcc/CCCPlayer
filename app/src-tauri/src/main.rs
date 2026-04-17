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
///
/// Calls `-[NSProcessInfo beginActivityWithOptions:reason:]` with
/// `NSActivityUserInitiated | NSActivityLatencyCritical`. We intentionally
/// leak the returned activity token so it persists for the full process
/// lifetime; a proper M2 polish pass could tie the token's lifetime to the
/// active-Session state.
#[cfg(all(feature = "tauri", target_os = "macos"))]
fn suppress_app_nap() {
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, ClassType};
    use objc2_foundation::{NSProcessInfo, NSString};

    // NSActivityOptions constants; values from Foundation/NSProcessInfo.h.
    const USER_INITIATED: u64 = 0x0000_0000_00FF_FFFF;
    const LATENCY_CRITICAL: u64 = 0xFF00_0000_0000_0000;
    let opts = USER_INITIATED | LATENCY_CRITICAL;

    // SAFETY: All objects come from a live NSProcessInfo instance, and the
    // returned activity token is retained by us for the process lifetime.
    unsafe {
        let pi = NSProcessInfo::processInfo();
        let reason = NSString::from_str("CCCPlayer orchestrating long-running agents");
        let _token: *mut AnyObject = msg_send![
            &*pi,
            beginActivityWithOptions: opts,
            reason: &*reason
        ];
        // Intentionally leak: we want this to live for the whole app.
        std::mem::forget(_token);
    }
    tracing::info!("app-nap suppression: beginActivity(.userInitiated | .latencyCritical) active");
}
