// dialog_manager.rs 模块 - 处理各种对话框

use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder, MessageDialogBuilder, MessageDialogKind};

/// 显示目录选择对话框
/// 
/// # 参数
/// - `app_handle`: Tauri应用句柄
/// 
/// # 返回值
/// - 成功时返回用户选择的目录路径
/// - 失败时返回包含错误信息的Err(String)
#[tauri::command(async)]
pub async fn show_directory_dialog<R: Runtime>(app_handle: AppHandle<R>) -> Result<String, String> {
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;
    
    // 创建一个oneshot通道用于传递结果
    let (tx, rx) = oneshot::channel();
    let tx = Arc::new(Mutex::new(Some(tx)));
    
    let dialog = app_handle.dialog().clone();
    
    // 在后台线程中运行对话框，避免阻塞UI
    std::thread::spawn(move || {
        FileDialogBuilder::new(dialog)
            .set_title("选择数据存储目录")
            .pick_folder(move |path| {
                // 使用Mutex和Option确保只发送一次结果
                if let Ok(mut guard) = tx.lock() {
                    if let Some(tx) = guard.take() {
                        // 忽略发送错误，因为接收端可能已关闭
                        let _ = tx.send(path);
                    }
                }
            });
    });
    
    // 异步等待结果
    let result = rx.await.map_err(|_| "接收结果失败".to_string())?;
    
    match result {
        Some(path) => {
            // 将FilePath转换为字符串
            Ok(path.to_string())
        },
        None => {
            // 用户取消了选择
            Err("用户取消了目录选择".to_string())
        }
    }
}

/// 显示错误对话框
/// 
/// # 参数
/// - `app_handle`: Tauri应用句柄
/// - `message`: 错误信息
/// 
/// # 返回值
/// - 成功时返回Ok(())
pub fn show_dialog<R: Runtime>(app_handle: &AppHandle<R>, message: &str, kind: MessageDialogKind, title: &str) {
    MessageDialogBuilder::new(app_handle.dialog().clone(), title, message)
        .kind(kind)
        .show(|_| {});
}

/// 显示阻塞式对话框
/// 
/// # 参数
/// - `app_handle`: Tauri应用句柄
/// - `message`: 对话框消息
/// - `title`: 对话框标题
/// - `kind`: 对话框类型
/// 
/// # 返回值
/// - 成功时返回Ok(())
pub fn show_blocking_dialog<R: Runtime>(app_handle: &AppHandle<R>, message: &str, title: &str, kind: MessageDialogKind) {
    // 在Tauri 2.0中，MessageDialogBuilder没有show_blocking方法，
    // 我们使用show方法并等待结果
    use std::sync::mpsc;
    
    let (tx, rx) = mpsc::channel();
    
    MessageDialogBuilder::new(app_handle.dialog().clone(), title, message)
        .kind(kind)
        .show(move |_| {
            tx.send(()).unwrap();
        });
    
    // 等待对话框关闭
    rx.recv().unwrap();
}

/// 显示确认对话框
/// 
/// # 参数
/// - `app_handle`: Tauri应用句柄
/// - `message`: 对话框消息
/// - `title`: 对话框标题
/// 
/// # 返回值
/// - 成功时返回用户的选择结果
pub fn show_confirm_dialog<R: Runtime>(app_handle: &AppHandle<R>, message: &str, title: &str) -> bool {
    // 在Tauri 2.0中，MessageDialogBuilder没有show_blocking方法，
    // 我们使用show方法并等待结果
    use std::sync::mpsc;
    
    let (tx, rx) = mpsc::channel();
    
    MessageDialogBuilder::new(app_handle.dialog().clone(), title, message)
        .kind(MessageDialogKind::Info)
        .show(move |result| {
            tx.send(result).unwrap();
        });
    
    // 等待并返回结果
    rx.recv().unwrap()
}
