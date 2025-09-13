// init.rs 模块 - 负责应用程序的初始化和资源清理工作

// 标准库导入
use std::{
    process::exit,
    sync::atomic::{AtomicBool, Ordering},
};

// 第三方库导入
use lazy_static::lazy_static;
use serde_json;
use tauri::{App, AppHandle, Emitter, Manager};
use tauri_plugin_dialog::MessageDialogKind;

// 定义全局标志，表示应用是否正在关闭
lazy_static! {
    static ref APP_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
}

// 获取应用关闭状态
pub fn is_app_shutting_down() -> bool {
    APP_SHUTTING_DOWN.load(Ordering::Relaxed)
}

// 设置应用关闭状态
pub fn set_app_shutting_down(shutting_down: bool) {
    APP_SHUTTING_DOWN.store(shutting_down, Ordering::Relaxed);
}

// 内部模块导入
use crate::{
    aria2c::{cleanup_aria2c_resources, initialize_aria2c_backend},
    dialog_manager::{show_blocking_dialog, show_dialog},
    dir_manager::{
        cleanup_temp_dir, get_global_temp_dir, get_l4d2_addons_dir, set_global_extract_dir,
    },
    download_manager::initialize_7z_resources,
    log_info,
};

/// 更新窗口标题 - 在应用标题后追加自定义内容
///
/// 此函数获取当前应用的窗口实例，并将标题设置为应用名称加上自定义内容的格式。
///
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于获取窗口实例
/// - `title`: 要追加到窗口标题的自定义内容
pub fn update_window_title(app_handle: &AppHandle, title: &str) {
    if let Some(window) = app_handle.get_webview_window("main") {
        // 从配置中获取应用名称
        let app_name = app_handle.config().app.windows[0].title.clone();
        let _ = window.set_title(&format!("{}: {}", app_name, title));
    }
}

/// 初始化应用程序 - 设置临时目录、获取L4D2目录、更新窗口标题等操作
///
/// 此函数负责应用程序的初始化工作，包括：
/// 1. 初始化全局临时目录
/// 2. 自动获取Left 4 Dead 2的addons目录
/// 3. 发送目录更改事件到前端
/// 4. 更新窗口标题
/// 5. 设置全局解压目录
/// 6. 显示主窗口
///
/// 如果无法获取L4D2的addons目录，将显示错误对话框并退出应用。
///
/// # 参数
/// - `app`: Tauri应用实例
///
/// # 返回值
/// - 成功时返回Ok(())
/// - 失败时返回包含错误信息的Err
pub fn initialize_app(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    // 获取应用句柄的克隆，以便在异步任务中使用
    let app_handle = app.handle().clone();

    // 初始化全局临时目录管理器
    if let Err(e) = get_global_temp_dir() {
        eprintln!("初始化临时目录失败: {}", e);
    }

    // 初始化aria2c后端
    match initialize_aria2c_backend() {
        Err(e) => {
            eprintln!("初始化aria2c后端失败: {}", e);
            // 显示初始化失败对话框
            show_dialog(
                &app.handle(),
                &format!("初始化aria2c下载引擎失败: {}", e),
                MessageDialogKind::Error,
                "初始化失败",
            );
        }
        Ok(()) => {}
    }

    // 初始化7z资源（与aria2c一样，在应用启动时释放）
    initialize_7z_resources();

    // 尝试自动获取 Left 4 Dead 2 的addons目录
    match get_l4d2_addons_dir() {
        Ok(addons_dir) => {
            // 发送目录更改事件到前端
            let _ = app.emit_to(
                "main",
                "extract-dir-changed",
                &serde_json::json!({
                    "newDir": addons_dir,
                    "success": true
                }),
            );

            // 更新窗口标题，显示当前L4D2目录
            update_window_title(&app_handle, &addons_dir);

            // 设置全局解压目录，用于下载后解压文件
            set_global_extract_dir(&addons_dir)?;

            // 初始化检查完成，没有错误，显示主窗口
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = window.show() {
                    eprintln!("无法显示窗口: {:?}", e);
                }
            }
        }
        Err(e) => {
            // 在退出前显示一个错误对话框，提示无法找到L4D2目录
            show_blocking_dialog(&app.handle(), &e, MessageDialogKind::Error, "错误");

            // 显示对话框后，立即退出应用程序
            exit(1);
        }
    };

    Ok(())
}

/// 清理应用程序资源 - 在窗口关闭时调用，负责清理临时目录等资源
///
/// 此函数在应用程序关闭时调用，确保临时目录被正确清理，避免磁盘空间浪费。
pub fn cleanup_app_resources() {
    // 设置应用关闭标志，通知其他线程
    log_info!("应用开始关闭，设置关闭标志...");
    set_app_shutting_down(true);

    // 清理aria2c资源
    cleanup_aria2c_resources();
    // 清理临时目录
    cleanup_temp_dir();

    // 强制退出进程，确保应用立即关闭
    log_info!("资源清理完成，即将退出进程...");
    std::process::exit(0);
}
