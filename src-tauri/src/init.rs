// init.rs 模块 - 负责应用程序的初始化和资源清理工作

// 标准库导入
use std::{
    process::exit,
    sync::atomic::{AtomicBool, Ordering},
};

// 第三方库导入
use lazy_static::lazy_static;
use serde_json;
use tauri::{App, AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};
use tauri_plugin_dialog::MessageDialogKind;

// 定义全局变量
lazy_static! {
    // 表示应用是否正在关闭
    static ref APP_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
    // 全局应用句柄，用于在清理资源时关闭窗口
    pub static ref GLOBAL_APP_HANDLE: std::sync::Arc<std::sync::RwLock<Option<tauri::AppHandle>>> =
        std::sync::Arc::new(std::sync::RwLock::new(None));
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
    config_manager::get_data_dir,
    dialog_manager::{show_blocking_dialog, show_dialog},
    dir_manager::{get_l4d2_addons_dir, set_global_addons_dir},
    download_manager,
    extract_manager::initialize_7z_resources,
    log_error, log_info, log_warn,
};

/// 将窗口在屏幕上居中
///
/// 此函数获取窗口大小和屏幕大小，计算居中位置，并设置窗口位置。
///
/// # 参数
/// - `window`: 要居中的窗口实例
///
/// # 返回值
/// - 成功时返回Ok(())
/// - 失败时返回包含错误信息的Err
fn center_window_on_screen(window: &WebviewWindow) -> Result<(), Box<dyn std::error::Error>> {
    // 获取窗口大小
    let window_size = window.inner_size()?;

    // 获取窗口当前所在的屏幕
    let screen = window.current_monitor()?.ok_or("无法获取当前屏幕")?;

    // 获取屏幕工作区大小（不包括任务栏等区域）
    let work_area = screen.work_area();

    // 计算居中位置
    let position = PhysicalPosition {
        x: (work_area.size.width as i32 - window_size.width as i32) / 2,
        y: (work_area.size.height as i32 - window_size.height as i32) / 2,
    };

    // 设置窗口位置
    window.set_position(position)?;

    Ok(())
}

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
/// 7. 保存全局应用句柄，用于后续资源清理时关闭窗口
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

    // 保存全局应用句柄，用于资源清理时关闭窗口
    *GLOBAL_APP_HANDLE.write().unwrap() = Some(app_handle.clone());

    // 读取数据存储目录配置
    let nmd_data_dir = get_data_dir(app_handle.clone())?;

    // 初始化目录管理器
    let dir_manager = match nmd_data_dir {
        Some(ref data_dir) => {
            log_info!("使用配置的 nmd_data 目录: {}", data_dir);
            crate::dir_manager::DirManager::with_nmd_data_dir(std::path::PathBuf::from(data_dir))
        }
        None => {
            // 没有配置 nmd_data 目录，弹窗要求配置
            log_warn!("未配置 nmd_data 目录，弹窗要求配置");
            show_dialog(
                &app_handle,
                "请先配置数据存储目录。\n\n在文件管理器窗口中点击\"修改目录\"按钮进行配置。",
                MessageDialogKind::Warning,
                "未配置数据目录",
            );
            return Err("未配置数据存储目录，请先配置".into());
        }
    };

    // 设置全局目录管理器
    let dir_manager = match dir_manager {
        Ok(dm) => dm,
        Err(e) => {
            eprintln!("初始化目录管理器失败: {}", e);
            show_dialog(
                &app_handle,
                &format!("初始化目录管理器失败: {}", e),
                MessageDialogKind::Error,
                "初始化失败",
            );
            return Err(e.into());
        }
    };

    {
        let mut guard = crate::dir_manager::DIR_MANAGER.lock().unwrap();
        *guard = Some(dir_manager);
    } // 锁在这里释放

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
        Ok(()) => {
            // 尝试加载之前保存的下载队列
            if let Err(e) = download_manager::load_download_queue() {
                eprintln!("加载下载队列失败: {}", e);
                log_warn!("加载下载队列失败: {}", e);
            }
        }
    }

    // 初始化7z资源（与aria2c一样，在应用启动时释放）
    log_info!("开始初始化 7z 资源...");
    initialize_7z_resources();
    log_info!("7z 资源初始化完成");

    // 尝试自动获取 Left 4 Dead 2 的addons目录
    log_info!("开始查找 Left 4 Dead 2 addons 目录...");
    match get_l4d2_addons_dir() {
        Ok(addons_dir) => {
            log_info!("成功找到 L4D2 addons 目录: {}", addons_dir);
            // 发送目录更改事件到前端
            let _ = app.emit_to(
                "main",
                "extract-dir-changed",
                &serde_json::json!({
                    "newDir": addons_dir,
                    "success": true
                }),
            );

            // 读取数据存储目录
            let title_text = match nmd_data_dir {
                Some(data_dir) => data_dir,
                None => addons_dir.clone(),
            };

            // 更新窗口标题，优先显示数据存储目录
            log_info!("更新窗口标题: {}", title_text);
            update_window_title(&app_handle.clone(), &title_text);

            // 设置全局 L4D2 addons 目录，用于后续可能的操作
            log_info!("设置全局 L4D2 addons 目录: {}", addons_dir);
            set_global_addons_dir(&addons_dir)?;

            // 初始化检查完成，没有错误，显示主窗口
            log_info!("准备显示主窗口...");
            if let Some(window) = app.get_webview_window("main") {
                log_info!("找到主窗口，开始居中和显示...");
                // 使窗口在屏幕上居中
                if let Err(e) = center_window_on_screen(&window) {
                    eprintln!("无法将窗口居中: {:?}", e);
                    log_error!("无法将窗口居中: {:?}", e);
                }
                if let Err(e) = window.show() {
                    eprintln!("无法显示窗口: {:?}", e);
                    log_error!("无法显示窗口: {:?}", e);
                } else {
                    log_info!("主窗口已显示");
                }
            } else {
                log_error!("未找到主窗口");
            }
        }
        Err(e) => {
            log_error!("查找 L4D2 addons 目录失败: {}", e);
            // 在退出前显示一个错误对话框，提示无法找到L4D2目录
            show_blocking_dialog(&app.handle(), &e, "错误", MessageDialogKind::Error);
            // 显示对话框后，立即退出应用程序
            exit(1);
        }
    };

    log_info!("应用初始化完成");
    Ok(())
}

/// 清理应用程序资源 - 在窗口关闭时调用，负责清理临时目录等资源并关闭窗口
///
/// 此函数在应用程序关闭时调用，确保：
/// 1. 临时目录被正确清理，避免磁盘空间浪费
/// 2. 所有窗口被正确关闭
pub fn cleanup_app_resources() {
    // 检查是否已经在关闭过程中，如果是则直接返回，避免循环调用
    if is_app_shutting_down() {
        log_info!("应用已经在关闭过程中，跳过重复的资源清理...");
        return;
    }

    // 设置应用关闭标志，通知其他线程
    log_info!("应用开始关闭，设置关闭标志...");
    set_app_shutting_down(true);

    // 尝试获取全局应用句柄
    if let Some(app_handle) = &*GLOBAL_APP_HANDLE.read().unwrap() {
        log_info!("正在关闭所有窗口...");

        // 关闭所有窗口
        for (label, window) in app_handle.webview_windows() {
            log_info!("关闭窗口: {:?}", label);
            let _ = window.destroy();
        }
    }

    // 保存下载队列
    if let Err(e) = download_manager::save_download_queue() {
        log_error!("保存下载队列失败: {}", e);
    }

    // 清理aria2c资源
    cleanup_aria2c_resources();

    log_info!("资源清理完成，进程即将退出...");

    // 尝试获取全局应用句柄用于后续的正常退出操作
    let app_exit_closure = || {
        if let Some(app_handle) = &*GLOBAL_APP_HANDLE.read().unwrap() {
            log_info!("请求应用正常退出...");
            // 使用Tauri的app.exit()方法进行正常退出
            let _ = app_handle.exit(0);
        } else {
            log_warn!("请求应用强制退出...");
            exit(1);
        }
    };

    app_exit_closure();
}
