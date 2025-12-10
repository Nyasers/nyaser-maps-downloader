// 引入 Tauri 相关模块
use tauri::{AppHandle, Emitter, Manager, Url};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_updater::UpdaterExt;

// 导入子模块
mod aria2c;
mod commands;
mod dialog_manager;
mod dir_manager;
mod download_manager;
mod extract_manager;
mod init;
mod log_utils;
mod queue_manager;
mod utils;

// 异步信号处理函数
async fn handle_signals() {
    // 等待Ctrl+C信号
    tokio::signal::ctrl_c().await.expect("无法等待Ctrl+C信号");
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
    // 创建一个异步运行时来处理信号
    let rt = tokio::runtime::Runtime::new().expect("无法创建Tokio运行时");

    // 在后台线程中启动信号处理
    std::thread::spawn(move || {
        rt.block_on(handle_signals());
    });

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
        .invoke_handler(tauri::generate_handler![
            commands::install,
            commands::get_middleware,
            commands::open_external_link,
            commands::open_file_manager_window,
            commands::open_server_list_window,
            commands::get_nmd_files,
            commands::delete_nmd_file,
            commands::cancel_download,
            commands::refresh_download_queue,
            commands::cancel_all_downloads,
            commands::frontend_loaded,
            commands::deep_link_ready,
        ])
        // 处理不同窗口的关闭请求
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                // 根据窗口标签执行不同的操作
                if window.label() == "main" {
                    // main窗口关闭时：隐藏窗口并清理资源
                    window.hide().unwrap();
                    init::cleanup_app_resources();
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
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    update(handle).await.unwrap();
                });
            }
            let deep_link = app.deep_link();
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
        app.restart();
    } else {
        log_info!("更新检查完成，未发现可用更新");
    }
    Ok(())
}
