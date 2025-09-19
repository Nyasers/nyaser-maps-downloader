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

// 异步信号处理函数
async fn handle_signals() {
    // 等待Ctrl+C信号
    tokio::signal::ctrl_c().await.expect("无法等待Ctrl+C信号");
    // 收到信号后清理资源
    init::cleanup_app_resources();
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
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::download,
            commands::get_middleware,
            commands::open_external_link,
            commands::open_file_manager_window,
            commands::open_server_list_window,
            commands::get_nmd_files,
            commands::delete_nmd_file
        ])
        // 添加应用启动时的初始化逻辑
        .setup(|app| Ok(init::initialize_app(app)?))
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
