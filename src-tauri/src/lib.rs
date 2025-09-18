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
            commands::get_middleware
        ])
        // 添加应用启动时的初始化逻辑
        .setup(|app| Ok(init::initialize_app(app)?))
        // 当窗口关闭请求时，清理应用程序资源
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { .. } => {
                window.hide().unwrap();
                init::cleanup_app_resources();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
