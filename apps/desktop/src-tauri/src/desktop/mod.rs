mod commands;
mod core_manager;
mod core_manager_api;
mod core_manager_events;
mod helpers;
mod tray;
mod types;
#[cfg(test)]
mod webview_security_tests;
mod window_lifecycle;

pub(crate) use commands::{
    core_api_call, core_events_start, core_import_plugin_zip, core_start, core_status, core_stop,
    desktop_auto_close_gui, desktop_get_autostart, desktop_set_autostart,
};
pub(crate) use core_manager::CoreManager;
pub(crate) use tray::setup_tray;
pub(crate) use window_lifecycle::apply_main_window_close_behavior;
