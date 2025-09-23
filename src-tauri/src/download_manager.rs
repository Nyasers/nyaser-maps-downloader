// download_manager.rs 模块 - 负责管理地图文件的下载队列和下载过程

// 标准库导入
use std::sync::{Arc, Mutex};

// 第三方库导入
use serde_json;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::MessageDialogKind;

// 内部模块导入
use crate::{
    aria2c::download_via_aria2,
    dialog_manager::show_dialog,
    extract_manager::{start_extract_queue_manager, ExtractTask},
    init::is_app_shutting_down,
    log_debug, log_error, log_info, log_warn,
    queue_manager::{process_queue, QueueManager, TaskQueue},
};

/// 下载任务结构体 - 表示一个地图下载任务的基本信息
#[derive(Debug, Clone)]
pub struct DownloadTask {
    /// 任务唯一标识符
    pub id: String,
    /// 要下载的文件URL
    pub url: String,
    /// 文件解压目标目录
    pub extract_dir: String,
    /// 文件名（可选，如未指定则从URL中提取）
    pub filename: Option<String>,
}

// 创建全局下载队列实例 - 使用lazy_static实现延迟初始化
lazy_static::lazy_static! {
    pub static ref DOWNLOAD_QUEUE: Arc<Mutex<TaskQueue<DownloadTask>>> = Arc::new(Mutex::new(TaskQueue::new(1)));
    pub static ref DOWNLOAD_MANAGER: QueueManager<DownloadTask> = QueueManager::new(1);
}

/// 启动下载队列管理器 - 使用通用队列管理功能处理下载任务
///
/// 此函数使用QueueManager的start_processing方法来处理下载任务，
/// 提供与传统process_download_queue函数相同的功能，但采用更通用的实现。
///
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于发送事件通知
#[allow(dead_code)]
pub fn start_download_queue_manager(app_handle: AppHandle) {
    let app_handle_clone = app_handle.clone();

    // 处理单个下载任务的函数
    let process_task_fn = move |task: DownloadTask| {
        let task_id = task.id.clone();
        let filename = task.filename.clone().unwrap_or("未知文件".to_string());
        let url = task.url.clone();

        log_info!(
            "开始处理下载任务 [{}]: {} (URL: {})",
            task_id,
            filename,
            url
        );

        let app_clone = app_handle_clone.clone();
        let task_clone = task.clone();

        // 发送任务开始事件通知
        let _ = app_clone.emit_to(
            "main",
            "download-task-start",
            &serde_json::json!(
                {
                    "taskId": task.id,
                    "url": task.url,
                    "filename": task.filename
                }
            ),
        );

        // 在异步任务中处理下载和解压
        tauri::async_runtime::spawn(async move {
            let result = download_and_extract(
                &task_clone.url,
                &task_clone.extract_dir,
                app_clone.clone(),
                &task_clone.id,
            )
            .await;

            // 发送任务完成事件通知
            let message = match &result {
                Ok(dir_path) => format!("{}", dir_path),
                Err(e) => e.to_string(),
            };

            // 记录下载完成日志
            if result.is_ok() {
                log_info!(
                    "下载任务 [{}] 完成: {}, 解压将在后台进行",
                    task_id,
                    filename
                );
            } else {
                log_error!(
                    "下载任务 [{}] 失败: {}, 错误: {}",
                    task_id,
                    filename,
                    message
                );
            }

            // 根据下载结果发送不同事件
            if result.is_ok() {
                let _ = app_clone.emit_to(
                    "main",
                    "download-complete",
                    &serde_json::json!(
                        {
                            "taskId": task_clone.id,
                            "success": true,
                            "message": message,
                            "filename": task_clone.filename.clone().unwrap_or("未知文件".to_string())
                        }
                    ),
                );
            } else {
                let _ = app_clone.emit_to(
                    "main",
                    "download-failed",
                    &serde_json::json!(
                        {
                            "taskId": task_clone.id,
                            "filename": task_clone.filename.clone().unwrap_or("未知文件".to_string()),
                            "error": message
                        }
                    ),
                );
            }

            // 任务完成后，从活跃任务集合中移除
            {
                {
                    let mut queue = (&*DOWNLOAD_QUEUE).lock().unwrap();
                    queue.remove_active_task(&task_id);
                    log_debug!(
                        "下载任务 [{}] 从活跃任务集合中移除，当前活跃任务数: {}",
                        task_id,
                        queue.active_tasks.len()
                    );
                }
            }
        });
    };

    // 获取任务ID的函数
    let get_task_id_fn = |task: &DownloadTask| task.id.clone();

    // 检查是否应继续处理的函数
    let should_continue_fn = || !is_app_shutting_down();

    // 使用QueueManager启动队列处理
    DOWNLOAD_MANAGER.start_processing(
        process_task_fn,
        get_task_id_fn,
        100, // 检查间隔时间（毫秒）
        should_continue_fn,
    );
}

/// 处理下载队列中的任务 - 持续监控队列并启动下载任务
///
/// 此函数会持续运行，定期检查队列并根据最大并发任务数启动新的下载任务，支持多文件同时下载。
/// 在处理每个任务时，会向前端发送任务开始和任务完成的事件通知。
///
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于发送事件通知
pub async fn process_download_queue(app_handle: AppHandle) {
    // 处理单个下载任务的函数
    let process_task_fn = move |task: DownloadTask| {
        let task_id = task.id.clone();
        let filename = task.filename.clone().unwrap_or("未知文件".to_string());
        let url = task.url.clone();

        log_info!(
            "开始处理下载任务 [{}]: {} (URL: {})",
            task_id,
            filename,
            url
        );

        let app_clone = app_handle.clone();
        let task_clone = task.clone();

        // 发送任务开始事件通知
        let _ = app_clone.emit_to(
            "main",
            "download-task-start",
            &serde_json::json!(
                {
                    "taskId": task.id,
                    "url": task.url,
                    "filename": task.filename
                }
            ),
        );

        // 在异步任务中处理下载和解压
        tauri::async_runtime::spawn(async move {
            let result = download_and_extract(
                &task_clone.url,
                &task_clone.extract_dir,
                app_clone.clone(),
                &task_clone.id,
            )
            .await;

            // 发送任务完成事件通知
            let message = match &result {
                Ok(dir_path) => format!("{}", dir_path),
                Err(e) => e.to_string(),
            };

            // 记录下载完成日志
            if result.is_ok() {
                log_info!(
                    "下载任务 [{}] 完成: {}, 解压将在后台进行",
                    task_id,
                    filename
                );
            } else {
                log_error!(
                    "下载任务 [{}] 失败: {}, 错误: {}",
                    task_id,
                    filename,
                    message
                );
            }

            // 根据下载结果发送不同事件
            if result.is_ok() {
                let _ = app_clone.emit_to(
                    "main",
                    "download-complete",
                    &serde_json::json!(
                        {
                            "taskId": task_clone.id,
                            "success": true,
                            "message": message,
                            "filename": task_clone.filename.clone().unwrap_or("未知文件".to_string())
                        }
                    ),
                );
            } else {
                let _ = app_clone.emit_to(
                    "main",
                    "download-failed",
                    &serde_json::json!(
                        {
                            "taskId": task_clone.id,
                            "filename": task_clone.filename.clone().unwrap_or("未知文件".to_string()),
                            "error": message
                        }
                    ),
                );
            }

            // 任务完成后，从活跃任务集合中移除
            {
                let mut queue = (&*DOWNLOAD_QUEUE).lock().unwrap();
                queue.remove_active_task(&task_id);
                log_debug!(
                    "下载任务 [{}] 从活跃任务集合中移除，当前活跃任务数: {}",
                    task_id,
                    queue.active_tasks.len()
                );
            }
        });
    };

    // 获取任务ID的函数
    let get_task_id_fn = |task: &DownloadTask| task.id.clone();

    // 检查是否应继续处理的函数
    let should_continue_fn = || !is_app_shutting_down();

    // 使用通用的process_queue函数处理下载队列
    process_queue(
        DOWNLOAD_QUEUE.clone(),
        process_task_fn,
        get_task_id_fn,
        100, // 检查间隔时间（毫秒）
        should_continue_fn,
    )
    .await;
}

/// 下载并解压文件 - 执行地图文件的下载和解压操作
///
/// 此函数首先使用aria2c下载文件，然后将解压任务添加到解压队列中，由解压队列异步处理解压操作。
/// 这样可以避免多个下载任务同时解压导致的资源竞争问题。
///
/// # 参数
/// - `url`: 要下载的文件URL
/// - `extract_dir`: 解压目标目录
/// - `app_handle`: Tauri应用句柄，用于发送下载进度事件
/// - `task_id`: 下载任务的唯一标识符
///
/// # 返回值
/// - 成功时返回包含下载文件路径的Ok
/// - 失败时返回包含错误信息的Err
pub async fn download_and_extract(
    url: &str,
    extract_dir: &str,
    app_handle: AppHandle,
    task_id: &str,
) -> Result<String, String> {
    log_info!("开始下载文件 [{}]: URL={}", task_id, url);

    // 下载文件（异步等待）
    log_info!("[{}] 开始调用download_via_aria2函数进行下载", task_id);
    let file_path = match download_via_aria2(url, app_handle.clone(), task_id).await {
        Ok(path) => {
            log_info!("文件下载成功 [{}]: 保存路径={}", task_id, path);
            path
        }
        Err(err) => {
            log_error!("文件下载失败 [{}]: 错误={}", task_id, err);
            // 检查是否是用户取消下载
            if err != "用户取消下载" {
                // 只对真正的错误显示对话框
                show_dialog(
                    &app_handle,
                    &format!("下载失败: {}", err),
                    MessageDialogKind::Error,
                    "下载失败",
                );
            }
            return Err(err);
        }
    };
    log_info!(
        "[{}] download_via_aria2函数调用完成，返回路径={}",
        task_id,
        file_path
    );

    // 创建解压任务并添加到解压队列
    let extract_task = ExtractTask {
        id: uuid::Uuid::new_v4().to_string(), // 生成唯一的解压任务ID
        file_path: file_path.clone(),
        extract_dir: extract_dir.to_string(),
        app_handle: app_handle.clone(),
        download_task_id: task_id.to_string(),
    };

    let extract_task_id = extract_task.id.clone();
    log_info!(
        "创建解压任务 [{}] 关联下载任务 [{}]: 文件={}, 解压目录={}",
        extract_task_id,
        task_id,
        file_path,
        extract_dir
    );

    // 创建解压队列处理的独立引用
    let extract_task_clone = extract_task.clone();

    // 将解压任务添加到解压队列管理器
    crate::extract_manager::EXTRACT_MANAGER.add_task(extract_task_clone);
    log_debug!("解压任务 [{}] 添加到队列", extract_task_id);

    // 检查解压队列管理器是否已启动
    {
        let queue = crate::extract_manager::EXTRACT_MANAGER
            .queue
            .lock()
            .unwrap();
        if !queue.processing_started {
            log_info!(
                "解压队列处理未启动，现在启动，最大并发解压任务数: {}",
                queue.max_concurrent_tasks
            );
            // 释放锁后启动处理（避免死锁）
            drop(queue);
            // 启动解压队列处理（使用独立的异步任务，避免引用问题）
            tauri::async_runtime::spawn(async move {
                start_extract_queue_manager();
            });
        }
    }

    // 已有一处启动解压队列的逻辑，不再重复启动
    // 避免多次启动导致的竞争条件和锁争用问题
    // 解压队列管理由QueueManager统一处理

    // 安全检查：如果解压队列不为空，但活跃任务为0，可能表示处理逻辑出现问题
    {
        let queue = crate::extract_manager::EXTRACT_MANAGER
            .queue
            .lock()
            .unwrap();
        if !queue.queue.is_empty() && queue.active_tasks.is_empty() {
            log_warn!("解压队列有任务但无活跃任务，这由QueueManager自动处理");
        }
    };

    // 返回下载成功的信息，并明确指出解压将异步进行
    Ok(format!("文件下载成功，解压将在后台进行: {}", file_path))
}
