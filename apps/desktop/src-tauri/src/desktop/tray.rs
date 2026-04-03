use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use super::core_manager::CoreManager;
use super::window_lifecycle::{close_main_window, restore_main_window};

const TRAY_MENU_OPEN_WINDOW: &str = "open_window";
const TRAY_MENU_REFRESH_ALL: &str = "refresh_all";
const TRAY_MENU_QUIT_GUI: &str = "quit_gui";
const TRAY_MENU_QUIT_GUI_AND_STOP_CORE: &str = "quit_gui_and_stop_core";

pub(crate) fn setup_tray(app_handle: &AppHandle) -> tauri::Result<()> {
    let open_window = MenuItem::with_id(
        app_handle,
        TRAY_MENU_OPEN_WINDOW,
        "打开主窗口",
        true,
        None::<&str>,
    )?;
    let refresh_all = MenuItem::with_id(
        app_handle,
        TRAY_MENU_REFRESH_ALL,
        "刷新全部来源",
        true,
        None::<&str>,
    )?;
    let quit_gui = MenuItem::with_id(
        app_handle,
        TRAY_MENU_QUIT_GUI,
        "仅退出 GUI",
        true,
        None::<&str>,
    )?;
    let quit_gui_and_stop_core = MenuItem::with_id(
        app_handle,
        TRAY_MENU_QUIT_GUI_AND_STOP_CORE,
        "退出 GUI 并停止 Core",
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app_handle)?;

    let menu = Menu::with_items(
        app_handle,
        &[
            &open_window,
            &refresh_all,
            &separator,
            &quit_gui,
            &quit_gui_and_stop_core,
        ],
    )?;

    let tray_icon = app_handle
        .default_window_icon()
        .map(|icon| icon.clone().to_owned())
        .unwrap_or_else(build_fallback_tray_icon);

    TrayIconBuilder::with_id("subforge-tray")
        .icon(tray_icon)
        .menu(&menu)
        .tooltip("SubForge")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(handle_tray_icon_event)
        .on_menu_event(|app, event: MenuEvent| {
            handle_tray_menu_event(app, event.id().as_ref());
        })
        .build(app_handle)?;

    Ok(())
}

fn build_fallback_tray_icon() -> tauri::image::Image<'static> {
    // 使用高对比度纯色图标兜底，避免平台或资源异常导致托盘图标不可见。
    const WIDTH: usize = 16;
    const HEIGHT: usize = 16;
    let mut rgba = vec![0_u8; WIDTH * HEIGHT * 4];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[0] = 0x00;
        pixel[1] = 0xC6;
        pixel[2] = 0xE8;
        pixel[3] = 0xFF;
    }
    tauri::image::Image::new_owned(rgba, WIDTH as u32, HEIGHT as u32)
}

fn handle_tray_icon_event(tray: &tauri::tray::TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        restore_main_window(tray.app_handle());
    }
}

fn handle_tray_menu_event(app_handle: &AppHandle, menu_id: &str) {
    match menu_id {
        TRAY_MENU_OPEN_WINDOW => {
            restore_main_window(app_handle);
        }
        TRAY_MENU_REFRESH_ALL => {
            let app_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let manager = app_handle.state::<CoreManager>();
                if let Err(error) = manager.refresh_all_sources().await {
                    eprintln!("[tray] 刷新全部来源失败: {error}");
                }
            });
        }
        TRAY_MENU_QUIT_GUI => {
            close_main_window(app_handle.clone());
        }
        TRAY_MENU_QUIT_GUI_AND_STOP_CORE => {
            let app_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let manager = app_handle.state::<CoreManager>();
                let _ = manager.stop_core().await;
                close_main_window(app_handle);
            });
        }
        _ => {}
    }
}
