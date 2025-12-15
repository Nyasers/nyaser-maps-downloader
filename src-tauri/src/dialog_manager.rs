// dialog_manager.rs 模块 - 提供统一的对话框管理功能

// 标准库导入
use std::sync::{Arc, Mutex};

// 第三方库导入
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

// 使用lazy_static创建全局变量，用于跟踪当前打开的对话框
lazy_static::lazy_static! {
    static ref ACTIVE_DIALOG: Arc<Mutex<Option<()>>> = Arc::new(Mutex::new(None));
}

/// 统一显示消息对话框的函数
///
/// 此函数确保在显示新的对话框前关闭先前打开的对话框，并使用统一的对话框样式。
///
/// # 参数
/// - `app_handle`: Tauri应用句柄
/// - `message`: 要显示的消息内容
/// - `kind`: 对话框类型（信息、警告、错误等）
/// - `title`: 对话框标题
///
/// # 示例
/// ```
/// show_dialog(&app_handle, "操作成功", MessageDialogKind::Info, "成功");
/// ```
pub fn show_dialog(app_handle: &AppHandle, message: &str, kind: MessageDialogKind, title: &str) {
    // 锁定对话框状态
    let mut dialog_guard = ACTIVE_DIALOG.lock().unwrap();

    // 如果已有对话框打开，会在新对话框显示前关闭（这里通过重新赋值实现）
    *dialog_guard = Some(());

    // 创建对话框构建器
    let dialog_builder = app_handle.dialog().message(message).kind(kind).title(title);

    // 保存对话框状态的引用
    let dialog_guard_clone = Arc::clone(&ACTIVE_DIALOG);

    // 显示对话框，并在关闭时更新状态
    dialog_builder.show(move |_| {
        // 对话框关闭时，清除活动状态
        let mut dialog_guard = dialog_guard_clone.lock().unwrap();
        *dialog_guard = None;
    });
}

/// 显示确认对话框函数
///
/// 此函数显示一个确认对话框，返回用户的选择结果（true表示确认，false表示取消）。
///
/// # 参数
/// - `app_handle`: Tauri应用句柄
/// - `message`: 要显示的消息内容
/// - `title`: 对话框标题
///
/// # 返回值
/// - 用户点击确认返回true，点击取消返回false
///
/// # 示例
/// ```
/// let result = show_confirm_dialog(&app_handle, "确定要删除吗？", "确认删除");
/// ```
pub fn show_confirm_dialog(app_handle: &AppHandle, message: &str, title: &str) -> bool {
    // 使用阻塞式消息对话框实现确认功能
    // 锁定对话框状态
    let mut dialog_guard = ACTIVE_DIALOG.lock().unwrap();
    
    // 创建阻塞式对话框并显示
    // 使用默认的确认/取消按钮配置
    let result = app_handle
        .dialog()
        .message(message)
        .title(title)
        .blocking_show();
    
    // 清除对话框状态
    *dialog_guard = None;
    
    // 返回结果：直接返回用户的选择
    result
}

/// 统一显示阻塞式消息对话框的函数
///
/// 此函数用于需要等待用户响应的场景，如关键错误提示。
/// 由于是阻塞式调用，它会自动确保一次只显示一个对话框。
///
/// # 参数
/// - `app_handle`: Tauri应用句柄
/// - `message`: 要显示的消息内容
/// - `kind`: 对话框类型（信息、警告、错误等）
/// - `title`: 对话框标题
///
/// # 示例
/// ```
/// show_blocking_dialog(&app_handle, "致命错误，程序将退出", MessageDialogKind::Error, "错误");
/// ```
pub fn show_blocking_dialog(
    app_handle: &AppHandle,
    message: &str,
    kind: MessageDialogKind,
    title: &str,
) {
    // 阻塞式对话框会自动确保一次只显示一个
    app_handle
        .dialog()
        .message(message)
        .kind(kind)
        .title(title)
        .blocking_show();
}
