#[cfg(windows)]
mod clipboard;
mod data;
mod input;
mod platform;
use data::{Action, AppData, Mutation};
use serde::Serialize;
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[derive(Clone)]
struct BoundTarget {
    window: platform::Target,
    binding: Option<platform::Binding>,
}
struct AppState {
    data: Mutex<Result<AppData, String>>,
    path: PathBuf,
    target: Mutex<Option<BoundTarget>>,
    busy: AtomicBool,
    shortcut_error: Mutex<Option<String>>,
    last_delivery: Mutex<Option<(u64, Instant)>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    target: Option<platform::Target>,
    shortcut_error: Option<String>,
    data_path: String,
}
#[derive(Serialize)]
struct RunResult {
    message: String,
}
struct BusyGuard<'a>(&'a AtomicBool);
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
fn lock_error<T>(_: std::sync::PoisonError<T>) -> String {
    "内部状态暂时不可用，请重启 PromptDock。".into()
}

#[tauri::command]
fn load_data(state: State<AppState>) -> Result<AppData, String> {
    state.data.lock().map_err(lock_error)?.clone()
}
#[tauri::command]
fn get_status(state: State<AppState>) -> Result<Status, String> {
    let target = state
        .target
        .lock()
        .map_err(lock_error)?
        .as_ref()
        .map(|t| t.window.clone());
    Ok(Status {
        target,
        shortcut_error: state.shortcut_error.lock().map_err(lock_error)?.clone(),
        data_path: state.path.to_string_lossy().into_owned(),
    })
}
#[tauri::command]
fn mutate_data(
    app: AppHandle,
    state: State<AppState>,
    action: Mutation,
) -> Result<AppData, String> {
    let mut guard = state.data.lock().map_err(lock_error)?;
    let current = guard.as_ref().map_err(Clone::clone)?;
    let old_shortcut = current.settings.shortcut.clone();
    let mut candidate = current.clone();
    candidate.apply(action)?;
    let changed = candidate.settings.shortcut != old_shortcut;
    if changed {
        app.global_shortcut()
            .register(candidate.settings.shortcut.as_str())
            .map_err(|e| format!("无法注册快捷键；可能已被其他软件占用，或格式不正确：{e}"))?;
    }
    if let Err(error) = data::write_data(&state.path, &candidate) {
        if changed {
            let _ = app
                .global_shortcut()
                .unregister(candidate.settings.shortcut.as_str());
        }
        return Err(error);
    }
    if changed {
        let _ = app.global_shortcut().unregister(old_shortcut.as_str());
        *state.shortcut_error.lock().map_err(lock_error)? = None;
    }
    *guard = Ok(candidate.clone());
    drop(guard);
    let _ = app.emit("data-changed", &candidate);
    Ok(candidate)
}
#[tauri::command]
async fn list_targets() -> Result<Vec<platform::Target>, String> {
    tauri::async_runtime::spawn_blocking(platform::list_targets)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn bind_target(
    app: AppHandle,
    state: State<AppState>,
    hwnd: String,
) -> Result<platform::Target, String> {
    let target = platform::list_targets()
        .into_iter()
        .find(|w| w.hwnd == hwnd)
        .ok_or("目标窗口已关闭或不可用，请刷新列表。")?;
    platform::validate_target(&target)?;
    *state.target.lock().map_err(lock_error)? = Some(BoundTarget {
        window: target.clone(),
        binding: None,
    });
    let _ = app.emit("target-changed", &target);
    Ok(target)
}
fn show_manager_inner(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn show_dock_inner(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("dock") {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().map_err(|e| e.to_string())?;
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "dock", WebviewUrl::App("index.html?view=dock".into()))
        .title("PromptDock · 快捷面板")
        .inner_size(382., 650.)
        .min_inner_size(340., 420.)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
fn show_manager(app: AppHandle) -> Result<(), String> {
    show_manager_inner(&app)
}
#[tauri::command]
async fn show_dock(app: AppHandle) -> Result<(), String> {
    show_dock_inner(&app)
}
fn schedule_dock(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = show_dock_inner(&app);
    });
}
fn toggle_dock(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("dock") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
            return;
        }
        let _ = window.show();
        return;
    }
    schedule_dock(app);
}

#[tauri::command]
async fn run_prompt(
    app: AppHandle,
    id: String,
    text: String,
    action: Action,
) -> Result<RunResult, String> {
    if text.trim().is_empty() || text.chars().count() > 100_000 {
        return Err("提示词不能为空，且不能超过 100000 字。".into());
    }
    if app
        .state::<AppState>()
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("上一次操作还在进行，请稍候。".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _guard = BusyGuard(&state.busy);
        let settings = {
            let data = state.data.lock().map_err(lock_error)?;
            let data = data.as_ref().map_err(Clone::clone)?;
            if !data.prompts.iter().any(|p| p.id == id) {
                return Err("提示词已被删除，请刷新。".into());
            }
            data.settings.clone()
        };
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        text.hash(&mut hasher);
        let fingerprint = hasher.finish();
        if action == Action::Send {
            if state
                .last_delivery
                .lock()
                .map_err(lock_error)?
                .as_ref()
                .is_some_and(|(old, at)| {
                    *old == fingerprint && at.elapsed() < Duration::from_secs(3)
                })
            {
                return Err("刚刚已经发送过这条提示词，请稍候，避免重复发送。".into());
            }
        }
        let message = if action == Action::Copy {
            arboard::Clipboard::new()
                .and_then(|mut c| c.set_text(text))
                .map_err(|e| format!("复制失败：{e}"))?;
            "已复制提示词".into()
        } else {
            let target = state
                .target
                .lock()
                .map_err(lock_error)?
                .clone()
                .ok_or("请先在“发送至”中选择 Codex 窗口。")?;
            let result = platform::execute(
                &target.window,
                target.binding.as_ref(),
                &text,
                action,
                settings.send_key,
            );
            {
                let mut active = state.target.lock().map_err(lock_error)?;
                if let Some(active) = active.as_mut() {
                    if active.window.hwnd == target.window.hwnd
                        && active.window.pid == target.window.pid
                    {
                        active.binding = result
                            .as_ref()
                            .ok()
                            .and_then(|result| result.binding.clone());
                        active.window.bound = active.binding.is_some();
                        let _ = app.emit("target-changed", &active.window);
                    }
                }
            }
            result?.message
        };
        if action == Action::Send {
            *state.last_delivery.lock().map_err(lock_error)? = Some((fingerprint, Instant::now()));
        }
        let mut guard = state.data.lock().map_err(lock_error)?;
        let data = guard.as_mut().map_err(|e| e.clone())?;
        let mut candidate = data.clone();
        candidate.record_use(&id);
        let final_message = match data::write_data(&state.path, &candidate) {
            Ok(()) => {
                *data = candidate.clone();
                let _ = app.emit("data-changed", candidate);
                message
            }
            Err(_) => format!("{message}（使用记录保存失败）"),
        };
        Ok(RunResult {
            message: final_message,
        })
    })
    .await
    .map_err(|e| format!("操作未能完成：{e}"))?
}
#[tauri::command]
async fn export_data(app: AppHandle) -> Result<bool, String> {
    let data = app
        .state::<AppState>()
        .data
        .lock()
        .map_err(lock_error)?
        .clone()?;
    let bytes = serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?;
    let selected = rfd::AsyncFileDialog::new()
        .add_filter("PromptDock JSON", &["json"])
        .set_file_name("PromptDock-backup.json")
        .save_file()
        .await;
    match selected {
        Some(file) => {
            file.write(&bytes)
                .await
                .map_err(|e| format!("导出失败：{e}"))?;
            Ok(true)
        }
        None => Ok(false),
    }
}
#[tauri::command]
async fn pick_import() -> Result<Option<String>, String> {
    let selected = rfd::AsyncFileDialog::new()
        .add_filter("PromptDock JSON", &["json"])
        .pick_file()
        .await;
    let Some(file) = selected else {
        return Ok(None);
    };
    let metadata = std::fs::metadata(file.path()).map_err(|e| e.to_string())?;
    if metadata.len() > 10 * 1024 * 1024 {
        return Err("导入文件不能超过 10 MB。".into());
    }
    let raw =
        String::from_utf8(file.read().await).map_err(|_| "导入文件必须为 UTF-8 编码的 JSON。")?;
    let mut data: AppData =
        serde_json::from_str(&raw).map_err(|e| format!("导入文件格式不正确：{e}"))?;
    data.normalize_categories()?;
    data::validate(&data)?;
    Ok(Some(raw))
}
fn tray_icon() -> tauri::image::Image<'static> {
    let mut pixels = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let i = (y * 32 + x) * 4;
            let inside = (3..29).contains(&x) && (3..29).contains(&y);
            if inside {
                pixels[i] = 99;
                pixels[i + 1] = 88;
                pixels[i + 2] = 223;
                pixels[i + 3] = 255;
                let mark = (9..13).contains(&x) && (8..25).contains(&y)
                    || (12..23).contains(&x) && (8..12).contains(&y)
                    || (19..23).contains(&x) && (11..18).contains(&y)
                    || (12..22).contains(&x) && (15..19).contains(&y);
                if mark {
                    pixels[i] = 255;
                    pixels[i + 1] = 255;
                    pixels[i + 2] = 255;
                }
            }
        }
    }
    tauri::image::Image::new_owned(pixels, 32, 32)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let _ = show_manager_inner(app);
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_dock(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let path = data::data_path(app.handle()).map_err(std::io::Error::other)?;
            let data = data::read_data(&path);
            let shortcut = data
                .as_ref()
                .map(|d| d.settings.shortcut.clone())
                .unwrap_or_else(|_| "Ctrl+Alt+Space".into());
            let error = app
                .global_shortcut()
                .register(shortcut.as_str())
                .err()
                .map(|e| format!("快捷键未注册，可在设置中更换：{e}"));
            let target = platform::foreground_target().map(|window| BoundTarget {
                window,
                binding: None,
            });
            app.manage(AppState {
                data: Mutex::new(data),
                path,
                target: Mutex::new(target),
                busy: AtomicBool::new(false),
                shortcut_error: Mutex::new(error),
                last_delivery: Mutex::new(None),
            });
            use tauri::{
                menu::{Menu, MenuItem, PredefinedMenuItem},
                tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
            };
            let show = MenuItem::with_id(app, "show", "打开提示词库", true, None::<&str>)?;
            let dock = MenuItem::with_id(app, "dock", "打开快捷面板", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "退出 PromptDock", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &dock, &separator, &quit])?;
            TrayIconBuilder::new()
                .icon(tray_icon())
                .tooltip("PromptDock · 本地提示词库")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        let _ = show_manager_inner(app);
                    }
                    "dock" => {
                        schedule_dock(app);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_dock(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            load_data,
            get_status,
            mutate_data,
            list_targets,
            bind_target,
            show_manager,
            show_dock,
            run_prompt,
            export_data,
            pick_import
        ])
        .run(tauri::generate_context!())
        .expect("PromptDock failed to start");
}
