// 引入 Tauri 相关模块
use crate::init::GLOBAL_APP_HANDLE;
use std::io::{Error, ErrorKind};
use std::path::PathBuf;
use std::{future, thread};
use tauri::{
    async_runtime,
    http::{Request, Response},
    path::BaseDirectory,
    AppHandle, Emitter, Manager, Runtime, UriSchemeContext, UriSchemeResponder, Url,
};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_updater::UpdaterExt;

// 导入子模块
mod aria2c;
mod commands;
mod config_manager;
mod dialog_manager;
mod dir_manager;
mod download_manager;
mod extract_manager;
mod init;
mod log_utils;
mod queue_manager;
mod symlink_manager;
mod utils;

/// 验证符号链接支持
///
/// # 参数
/// - `cache_dir`: 缓存目录路径
/// - `addons_dir`: 游戏 addons 目录路径，即符号链接的目标目录
///
/// # 返回值
/// - 成功时返回 Ok(())
/// - 失败时返回包含错误信息的 Err
pub fn validate_symlink_support(
    cache_dir: &std::path::Path,
    addons_dir: &std::path::Path,
) -> Result<(), String> {
    use std::fs;
    use std::os::windows::fs::symlink_file;

    // 确保 addons_dir 存在
    fs::create_dir_all(addons_dir).map_err(|e| format!("无法创建 addons 目录: {:?}", e))?;

    // 生成哈希值用于链接文件名和测试内容
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use uuid::Uuid;

    let mut hasher = DefaultHasher::new();
    let uuid = Uuid::new_v4();
    uuid.hash(&mut hasher);
    let hash = hasher.finish();

    // 创建临时文件
    let temp_file = cache_dir.join(format!("nmd_test_{:016x}.tmp", hash));
    let test_content = format!("nmd_test: {:016x}", hash);
    fs::write(&temp_file, test_content.clone())
        .map_err(|e| format!("无法创建临时文件: {:?}", e))?;

    // 创建符号链接路径到 addons_dir
    let symlink_path = addons_dir.join(format!("nmd_link_{:016x}.tmp", hash));

    // 清理可能存在的旧链接
    if symlink_path.exists() {
        fs::remove_file(&symlink_path).ok();
    }

    // 创建符号链接
    symlink_file(&temp_file, &symlink_path).map_err(|e| format!("无法创建符号链接: {:?}", e))?;

    // 尝试通过符号链接读取文件
    let content = fs::read_to_string(&symlink_path)
        .map_err(|e| format!("无法通过符号链接读取文件: {:?}", e))?;

    if content != test_content {
        return Err("符号链接读取内容验证失败".to_string());
    }

    // 清理临时符号链接
    fs::remove_file(&symlink_path).ok();

    Ok(())
}

/// 以管理员身份重启应用
///
/// # 返回值
/// - 成功时返回 Ok(())
/// - 失败时返回包含错误信息的 Err
pub fn restart_as_admin() -> Result<(), String> {
    use std::process::Command;

    // 获取当前可执行文件路径
    if let Ok(exe_path) = std::env::current_exe() {
        log_debug!("当前可执行文件路径: {:?}", exe_path);
        if let Some(exe_path_str) = exe_path.to_str() {
            // 构建重启命令
            let mut cmd = Command::new("powershell");
            let msg = "您的系统需要管理员权限以创建符号链接。\n获取管理员权限失败，无法启动应用。";

            cmd.arg("-Command").arg(format!(
                "Start-Process '{}' -Verb RunAs; if (!$?) {{ Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show('{}', 'Nyaser Maps Downloader', 0, 16) }}",
                exe_path_str, msg
            ));

            // 执行命令
            if let Err(e) = cmd.spawn() {
                return Err(format!("无法启动管理员权限进程: {:?}", e));
            } else {
                // 退出当前应用
                std::process::exit(0);
            }
        } else {
            return Err("可执行文件路径不是有效字符串".to_string());
        }
    } else {
        return Err("无法获取可执行文件路径".to_string());
    }
}

/// 从Assets中获取资源路径
///
/// # 参数
/// - `asset_path`: Assets中的相对路径，例如 "bin/aria2c.exe" 或 "assets/serverlist/main.js"
///
/// # 返回值
/// - 成功时返回 `Ok(PathBuf)` 表示文件路径
/// - 失败时返回 `Err(String)` 表示错误信息
pub fn get_assets_path(asset_path: &str) -> Result<PathBuf, String> {
    loop {
        let result = match GLOBAL_APP_HANDLE.read() {
            Ok(guard) => {
                if let Some(ref app_handle) = *guard {
                    match app_handle
                        .path()
                        .resolve(asset_path.to_string(), BaseDirectory::Resource)
                    {
                        Ok(resource_path) => {
                            let resource_path = resource_path
                                .to_str()
                                .unwrap()
                                .split_once("\\\\?\\")
                                .unwrap()
                                .1;
                            log_debug!("获取资源路径: {:?} -> {:?}", asset_path, resource_path);
                            Some(Ok(PathBuf::from(resource_path)))
                        }
                        Err(e) => {
                            log_error!("解析二进制文件路径失败: {:?}", e);
                            Some(Err(e.to_string()))
                        }
                    }
                } else {
                    None
                }
            }
            Err(e) => {
                log_error!("无法获取全局应用句柄锁: {:?}", e);
                Some(Err(e.to_string()))
            }
        };

        if let Some(result) = result {
            return result;
        } else {
            log_info!("应用句柄未初始化，等待初始化完成...");
            thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

// 自定义协议处理函数，用于处理asset://请求
fn get_content_type(path: &str) -> &'static str {
    match path.split('.').last() {
        Some("html") => "text/html",
        Some("js") => "application/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        _ => "text/plain",
    }
}

fn build_response(content: Vec<u8>, content_type: &'static str) -> Response<Vec<u8>> {
    Response::builder()
        .status(200)
        .header("Content-Type", content_type)
        .header("Access-Control-Allow-Origin", "*")
        .body(content)
        .unwrap()
}

fn build_error_response() -> Response<Vec<u8>> {
    Response::builder()
        .status(404)
        .header("Access-Control-Allow-Origin", "*")
        .body(Vec::new())
        .unwrap()
}

fn handle_asset_request(path: &str, responder: UriSchemeResponder) {
    log_info!("asset协议请求: {}", path);

    let path = path.to_string();

    async_runtime::spawn(async move {
        let file_content = match crate::get_assets_path(&format!("assets/{}", path)) {
            Ok(resource_path) => {
                log_info!("资源路径: {:?}", resource_path);
                std::fs::read(&resource_path)
            }
            Err(e) => {
                log_error!("获取资源路径失败: {:?}", e);
                Err(Error::new(ErrorKind::NotFound, e))
            }
        };

        match file_content {
            Ok(content) => {
                let content_type = get_content_type(&path);
                let response = build_response(content, content_type);
                responder.respond(response);
            }
            Err(error) => {
                log_error!("asset协议请求失败: {:?}", error);
                let response = build_error_response();
                responder.respond(response);
            }
        }
    });
}

fn asset_protocol_handler<T: Runtime>(
    _context: UriSchemeContext<'_, T>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let path = request.uri().path().trim_start_matches('/').to_string();
    handle_asset_request(&path, responder);
}

// Windows API绑定和更可靠的信号处理
#[cfg(target_os = "windows")]
mod windows_signal {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Duration;

    static CLEANUP_CALLED: AtomicBool = AtomicBool::new(false);

    extern "system" fn console_handler(ctrl_type: u32) -> i32 {
        match ctrl_type {
            0 => {
                crate::log_info!("收到 CTRL_C_EVENT");
            }
            1 => {
                crate::log_info!("收到 CTRL_BREAK_EVENT");
            }
            2 => {
                crate::log_info!("收到 CTRL_CLOSE_EVENT");
            }
            5 => {
                crate::log_info!("收到 CTRL_LOGOFF_EVENT");
            }
            6 => {
                crate::log_info!("收到 CTRL_SHUTDOWN_EVENT");
            }
            _ => {
                crate::log_info!("收到未知控制事件: {}", ctrl_type);
            }
        }

        if !CLEANUP_CALLED.swap(true, Ordering::SeqCst) {
            crate::log_info!("开始执行清理...");
            super::init::cleanup_app_resources();
            thread::sleep(Duration::from_millis(500));
        }
        1
    }

    pub fn setup_signal_handlers() {
        unsafe {
            let result = windows_sys::Win32::System::Console::SetConsoleCtrlHandler(
                Some(console_handler),
                1,
            );

            if result != 0 {
                crate::log_info!("Windows控制台信号处理器设置成功");
            } else {
                crate::log_warn!("Windows控制台信号处理器设置失败");
            }
        }
    }
}

#[cfg(target_os = "windows")]
use windows_signal::setup_signal_handlers;

// 信号处理函数
fn handle_signals() {
    if cfg!(target_os = "windows") {
        setup_signal_handlers();

        use tokio::signal::windows;
        let _ = handle_signal(windows::ctrl_break().unwrap().recv());
        let _ = handle_signal(windows::ctrl_c().unwrap().recv());
        let _ = handle_signal(windows::ctrl_close().unwrap().recv());
        let _ = handle_signal(windows::ctrl_logoff().unwrap().recv());
        let _ = handle_signal(windows::ctrl_shutdown().unwrap().recv());
    } else {
        let _ = handle_signal(tokio::signal::ctrl_c());
    }
}

// 异步信号处理函数
async fn handle_signal(signal: impl future::Future) {
    // 等待信号
    signal.await;
    // 收到信号后清理资源
    init::cleanup_app_resources();
}

fn handle_open(app: AppHandle, arg: &str) {
    log_info!("收到打开URL: {}", arg);
    // 将接收到的参数发送给前端主窗口
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("deep-link-open", arg);
    }
}

fn handle_deep_link(app: AppHandle, args: Vec<String>) {
    log_info!("收到参数: {:?}", args);
    let urls = args
        .into_iter()
        .skip(1)
        .filter_map(|s| Url::parse(&s).ok())
        .collect::<Vec<_>>();
    for url in urls {
        if url.scheme() == "nmd" {
            let url = url.to_string().replace("nmd://", "");
            log_info!("收到nmd协议: {}", url);
            if let Some(args) = url.to_string().split_once("/") {
                match args.0 {
                    "open" => {
                        handle_open(app.clone(), args.1);
                    }
                    _ => {
                        log_error!("未知的nmd协议参数: {}", args.0);
                    }
                }
            }
        }
    }
}

// 主入口函数
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 启动信号处理
    handle_signals();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let handle = app.app_handle().clone();
            let window = &app.webview_windows()["main"];
            if window.is_minimized().unwrap() {
                window.unminimize().unwrap();
            }
            window.set_focus().unwrap();
            handle_deep_link(handle, args);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        // 注册自定义asset协议
        .register_asynchronous_uri_scheme_protocol("asset", asset_protocol_handler)
        .invoke_handler(tauri::generate_handler![
            commands::install,
            commands::open_filemanager_window,
            commands::open_serverlist_window,
            commands::get_maps,
            commands::delete_map_file,
            commands::delete_group,
            commands::cancel_download,
            commands::refresh_download_queue,
            commands::cancel_all_downloads,
            commands::frontend_loaded,
            commands::deep_link_ready,
            commands::get_file_symlinks,
            commands::create_file_symlink,
            commands::delete_file_symlink,
            commands::mount_file,
            commands::unmount_file,
            commands::mount_group,
            commands::unmount_group,
            commands::cleanup_invalid_links,
            commands::extract_dropped_file,
            commands::refresh_extract_queue,
            commands::cancel_extract,
            commands::cancel_all_extracts,
            config_manager::read_config,
            config_manager::write_config,
            config_manager::delete_config,
            dialog_manager::show_directory_dialog,
        ])
        // 处理不同窗口的关闭请求
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                // 根据窗口标签执行不同的操作
                if window.label() == "main" {
                    // main窗口关闭时：隐藏窗口并清理资源
                    window.hide().unwrap();
                    init::cleanup_app_resources();
                } else if window.label().starts_with("server_") {
                    // 服务器窗口关闭时：允许正常关闭
                    log_info!("服务器窗口 {} 已关闭", window.label());
                } else {
                    // 其他子窗口关闭时：只隐藏窗口，不清理资源
                    window.hide().unwrap();
                    // 阻止窗口默认关闭行为
                    api.prevent_close();
                    log_info!("子窗口 {} 已隐藏", window.label());
                }
            }
            _ => {}
        })
        // 添加应用启动时的初始化逻辑
        .setup(|app| {
            if !cfg!(debug_assertions) {
                {
                    let handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        update(handle).await.unwrap();
                    });
                }
                {
                    let handle = app.handle().clone();
                    let deep_link = handle.deep_link();
                    let protocol = "nmd";
                    if let Ok(is_registered) = deep_link.is_registered(protocol) {
                        if !is_registered {
                            if let Err(e) = deep_link.register(protocol) {
                                log_error!("注册{}协议失败: {:?}", protocol, e);
                            } else {
                                log_info!("{}协议注册成功", protocol);
                            }
                        }
                    }
                }
            } else {
                log_info!("开发环境，跳过软件更新 & 协议注册");
            }
            Ok(init::initialize_app(app)?)
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn update(app: tauri::AppHandle) -> tauri_plugin_updater::Result<()> {
    if let Some(update) = app.updater()?.check().await? {
        let mut downloaded = 0;

        // alternatively we could also call update.download() and update.install() separately
        update
            .download_and_install(
                |chunk_length, content_length| {
                    downloaded += chunk_length;
                    log_info!("downloaded {downloaded} from {content_length:?}");
                },
                || {
                    log_info!("download finished");
                },
            )
            .await?;

        log_info!("更新安装成功，应用即将重启");
        init::cleanup_app_resources_for_restart();
        app.restart();
    } else {
        log_info!("更新检查完成，未发现可用更新");
    }
    Ok(())
}
