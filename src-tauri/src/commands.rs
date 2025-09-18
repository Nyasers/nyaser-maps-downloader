// commands.rs 模块 - 定义应用程序的Tauri命令，处理前端与后端的通信

// 第三方库导入
use serde_json;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::MessageDialogKind;
use uuid::Uuid;

// 内部模块导入
use crate::{
    dialog_manager::show_dialog,
    dir_manager::DIR_MANAGER,
    download_manager::{process_download_queue, DownloadTask, DOWNLOAD_QUEUE},
    log_debug, log_error, log_info, log_warn,
};

/// 获取HTML注入片段 - 从应用资源中读取并返回完整的HTML注入片段
///
/// 此函数返回嵌入在程序中的下载拦截器的HTML片段，
/// 用于在前端页面中注入下载功能。
///
/// # 返回值
/// - 成功时返回HTML片段内容
/// - 失败时返回错误信息

// 使用include_bytes!宏将最小化的HTML片段直接嵌入到程序中
const DOWNLOAD_INTERCEPTOR_HTML: &[u8] = include_bytes!("../html/middleware.min.html");

#[tauri::command]
pub fn get_middleware() -> Result<String, String> {
    // 将字节数组转换为字符串
    let html_content = String::from_utf8(DOWNLOAD_INTERCEPTOR_HTML.to_vec())
        .map_err(|e| format!("HTML内容解码失败: {:?}", e))?;

    log_info!("成功获取HTML注入片段，长度: {} 字节", html_content.len());
    Ok(html_content)
}

/// 下载函数 - 将地图下载任务添加到下载队列
///
/// 此函数接收一个URL和应用句柄，创建下载任务并将其添加到下载队列中，
/// 同时向前端发送任务添加和队列更新的事件通知。
///
/// # 参数
/// - `url`: 要下载的文件URL
/// - `app_handle`: Tauri应用句柄，用于发送事件通知
///
/// # 返回值
/// - 成功时返回包含成功信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command(async)]
pub async fn download(url: &str, app_handle: AppHandle) -> Result<String, String> {
    log_info!("接收到下载请求: URL={}", url);

    // 锁定并获取目录管理器实例
    log_debug!("尝试锁定目录管理器...");
    let mut manager = DIR_MANAGER.lock().map_err(|e| {
        log_error!("无法锁定目录管理器: {:?}", e);
        // 显示错误对话框
        show_dialog(
            &app_handle,
            &format!("无法锁定目录管理器: {:?}", e),
            MessageDialogKind::Error,
            "错误",
        );
        format!("无法锁定目录管理器: {:?}", e)
    })?;
    log_debug!("目录管理器锁定成功");

    // 如果目录管理器尚未初始化，则进行初始化
    if manager.is_none() {
        log_info!("目录管理器未初始化，开始初始化...");
        *manager = Some(crate::dir_manager::DirManager::new().map_err(|e| {
            log_error!("目录管理器初始化失败: {}", e);
            // 显示错误对话框
            show_dialog(
                &app_handle,
                &format!("目录管理器初始化失败: {}", e),
                MessageDialogKind::Error,
                "错误",
            );
            e
        })?);
        log_info!("目录管理器初始化成功");
    }

    // 获取解压目录路径
    log_debug!("尝试获取解压目录...");
    let extract_dir = manager
        .as_mut()
        .unwrap()
        .extract_dir()
        .ok_or_else(|| {
            log_error!("无法获取解压目录");
            // 显示错误对话框
            show_dialog(
                &app_handle,
                "无法获取解压目录",
                MessageDialogKind::Error,
                "错误",
            );
            "无法获取解压目录".to_string()
        })?
        .to_string_lossy()
        .to_string();
    log_info!("解压目录设置为: {}", extract_dir);

    // 生成唯一的任务ID
    let task_id = Uuid::new_v4().to_string();
    log_info!("生成任务ID: {}", task_id);

    // 尝试从URL中提取文件名
    let filename = url.split('/').last().map(|s| s.to_string());
    log_debug!("从URL提取文件名: {:?}", filename);

    // 创建下载任务
    let task = DownloadTask {
        id: task_id.clone(),
        url: url.to_string(),
        extract_dir: extract_dir,
        filename: filename.clone(),
    };
    log_info!("创建下载任务: ID={}, URL={}", task_id, url);

    // 添加任务到下载队列
    log_debug!("尝试锁定下载队列并添加任务...");
    {
        let mut queue = (&*DOWNLOAD_QUEUE).lock().unwrap();
        queue.queue.push_back(task);
        log_info!("任务已添加到下载队列，当前队列长度: {}", queue.queue.len());
    }

    // 启动下载队列处理（确保队列处理逻辑正在运行）
    log_debug!("检查下载队列处理状态...");
    {
        let queue = (&*DOWNLOAD_QUEUE).lock().unwrap();
        if !queue.processing_started {
            log_info!("下载队列处理未启动，开始启动处理线程...");
            let app_handle_clone = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                log_debug!("下载队列处理线程已创建，准备开始处理队列");
                process_download_queue(app_handle_clone).await;
            });
        } else {
            log_debug!("下载队列处理已在运行中");
        }
    };

    // 额外的安全检查：如果队列不为空，但活跃任务为0，可能表示处理逻辑出现问题
    {
        let queue = (&*DOWNLOAD_QUEUE).lock().unwrap();
        if !queue.queue.is_empty() && queue.active_tasks.is_empty() && queue.processing_started {
            log_warn!("下载队列有任务但无活跃任务，可能需要重置处理状态");
            // 释放锁后重新启动处理（避免死锁）
            drop(queue);

            // 尝试重置处理状态并重新启动
            let mut queue_reset = (&*DOWNLOAD_QUEUE).lock().unwrap();
            queue_reset.processing_started = false;

            let app_handle_clone = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                log_debug!("重新启动下载队列处理线程");
                process_download_queue(app_handle_clone).await;
            });
        }
    };

    // 获取当前队列信息
    log_debug!("获取当前队列信息...");
    let (total_tasks, queue_size, waiting_tasks) = {
        let queue = (&*DOWNLOAD_QUEUE).lock().unwrap();
        let size = queue.queue.len();
        let active = queue.active_tasks.len();
        let total = size + active;

        // 构建等待任务列表（转换为可序列化的格式）
        let tasks = queue.queue
            .iter()
            .map(|task| {
                serde_json::json!({"id": task.id, "url": task.url, "filename": task.filename})
            })
            .collect::<Vec<_>>();

        log_debug!(
            "当前队列中有 {} 个等待任务，{} 个活跃任务，总共 {} 个任务",
            size,
            active,
            total
        );
        (total, size, tasks)
    };

    // 发送任务添加事件通知
    log_debug!("发送download-task-add事件...");
    let _ = app_handle.emit_to(
        "main",
        "download-task-add",
        &serde_json::json!({
            "taskId": task_id,
            "url": url,
            "filename": filename
        }),
    );

    // 发送队列更新事件通知
    log_debug!("发送download-queue-update事件...");
    let _ = app_handle.emit_to(
        "main",
        "download-queue-update",
        &serde_json::json!({
            "queue": {"waiting_tasks": waiting_tasks,
                       "total_tasks": total_tasks,
                       "active_tasks": queue_size}
        }),
    );

    // 返回成功消息
    log_info!(
        "下载请求处理完成: 任务ID={}, 总任务数={}",
        task_id,
        total_tasks
    );
    Ok(format!(
        "任务已添加到下载队列，当前总任务数: {}",
        total_tasks
    ))
}
