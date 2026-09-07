use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

pub mod arbiter;
pub mod bridge;
pub mod commands;
pub mod pty;

use commands::AppRuntime;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::open_lead,
            commands::open_shell,
            commands::open_consult,
            commands::close_pane,
            commands::pane_input_enqueue,
            commands::pane_reply_enqueue,
            commands::pane_input_wait,
            commands::pane_resize,
            commands::pane_ack,
            commands::set_policy,
            commands::answers_list,
            commands::deliver_now,
            commands::deliver_cancel,
            commands::held_send,
            commands::tab_resume,
            commands::list_state,
        ])
        .setup(|app| {
            app.manage(AppRuntime::start(app.handle()));
            WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("ConsensFlow")
                .inner_size(1280.0, 820.0)
                .min_inner_size(720.0, 520.0)
                .maximized(true)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the ConsensFlow app");

    app.run(|handle, event| {
        if matches!(event, RunEvent::Exit) {
            handle.state::<AppRuntime>().shutdown();
        }
    });
}
