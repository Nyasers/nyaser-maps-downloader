// download_manager.rs 模块 - 负责管理地图文件的下载队列和下载过程

// 标准库导入
use std::{
    collections::{HashSet, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

// 第三方库导入
use serde_json;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::MessageDialogKind;
use tokio::time;

// 内部模块导入
use crate::{
    aria2c::download_via_aria2, dialog_manager::show_dialog, 
    extract_manager::{ExtractTask, EXTRACT_QUEUE, process_extract_queue},
    init::is_app_shutting_down, log_debug, log_error, log_info, 
    log_warn,
};

/// 下载任务结构体 - 表示一个地图下载任务的基本信息
#[derive(Debug)]
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

/// 下载队列结构体 - 管理下载任务的队列和处理状态
#[derive(Debug)]
pub struct DownloadQueue {
    /// 任务队列，按顺序存储待处理的下载任务
    pub queue: VecDeque<DownloadTask>,
    /// 标记队列处理是否已启动
    pub processing_started: bool,
    /// 最大并行下载任务数
    pub max_concurrent_tasks: u32,
    /// 当前活跃任务的集合
    pub active_tasks: HashSet<String>,
}

// 创建全局下载队列实例 - 使用lazy_static实现延迟初始化
lazy_static::lazy_static! {
    pub static ref DOWNLOAD_QUEUE: Arc<Mutex<DownloadQueue>> = Arc::new(Mutex::new(DownloadQueue {
        queue: VecDeque::new(),
        processing_started: false,
        max_concurrent_tasks: 1, // 限制为1个并发下载任务
        active_tasks: HashSet::new(),
    }));
}

/// 处理下载队列中的任务 - 持续监控队列并启动下载任务
///
/// 此函数会持续运行，定期检查队列并根据最大并发任务数启动新的下载任务，支持多文件同时下载。
/// 在处理每个任务时，会向前端发送任务开始和任务完成的事件通知。
///
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于发送事件通知
pub async fn process_download_queue(app_handle: AppHandle) {
    // 标记队列处理已启动
    {
        let mut queue = DOWNLOAD_QUEUE.lock().unwrap();
        queue.processing_started = true;
        log_info!(
            "下载队列处理已启动，最大并发任务数: {}",
            queue.max_concurrent_tasks
        );
    }

    // 创建一个持续运行的循环，定期检查队列并启动新任务
    loop {
        // 检查应用是否正在关闭，如果是则退出循环
        if is_app_shutting_down() {
            log_info!("检测到应用正在关闭，停止处理下载队列...");
            break;
        }

        // 检查是否可以继续运行
        let has_tasks = {
            let queue = DOWNLOAD_QUEUE.lock().unwrap();
            // 检查是否有任务或活跃任务
            !queue.queue.is_empty() || !queue.active_tasks.is_empty()
        };

        // 如果没有任务，等待一小段时间后继续检查
        if !has_tasks {
            // 使用异步sleep避免阻塞运行时
            time::sleep(Duration::from_millis(100)).await;
            continue; // 继续下一轮循环，避免无任务时进行其他操作
        }

        // 尝试启动新的下载任务
        let maybe_task = {
            let mut queue = DOWNLOAD_QUEUE.lock().unwrap();

            // 检查是否可以启动新任务
            if queue.active_tasks.len() < queue.max_concurrent_tasks as usize {
                // 从队列中取出一个任务
                if let Some(task) = queue.queue.pop_front() {
                    // 将任务添加到活跃任务集合
                    queue.active_tasks.insert(task.id.clone());
                    log_debug!(
                        "下载任务 [{}] 从队列移动到活跃任务集合，当前活跃任务数: {}",
                        task.id,
                        queue.active_tasks.len()
                    );
                    Some(task)
                } else {
                    None
                }
            } else {
                // 只在需要时记录，避免日志过多
                None
            }
        };

        // 如果有任务可以启动，处理该任务
        if let Some(task) = maybe_task {
            let task_id = task.id.clone();
            let filename = task.filename.clone().unwrap_or("未知文件".to_string());
            let url = task.url.clone();

            log_info!(
                "开始处理下载任务 [{}]: {} (URL: {})",
                task_id,
                filename,
                url
            );

            // 克隆app_handle用于异步任务
            let app_handle_clone = app_handle.clone();

            // 发送任务开始事件通知
            let _ = app_handle.emit_to(
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

            // 直接在当前线程处理下载和解压（用于调试）
            log_info!("[{}] 直接在当前线程执行下载和解压", task_id);
            let result = download_and_extract(
                &task.url,
                &task.extract_dir,
                app_handle_clone.clone(),
                &task.id,
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
                let _ = app_handle_clone.emit_to(
                    "main",
                    "download-complete",
                    &serde_json::json!(
                        {
                            "taskId": task.id,
                            "success": true,
                            "message": message,
                            "filename": task.filename.clone().unwrap_or("未知文件".to_string())
                        }
                    ),
                );
            } else {
                let _ = app_handle_clone.emit_to(
                    "main",
                    "download-failed",
                    &serde_json::json!(
                        {
                            "taskId": task.id,
                            "filename": task.filename.clone().unwrap_or("未知文件".to_string()),
                            "error": message
                        }
                    ),
                );
            }

            // 任务完成后，从活跃任务集合中移除
            {
                let mut queue = DOWNLOAD_QUEUE.lock().unwrap();
                queue.active_tasks.remove(&task.id);
                log_debug!(
                    "下载任务 [{}] 从活跃任务集合中移除，当前活跃任务数: {}",
                    task_id,
                    queue.active_tasks.len()
                );
            }
        }

        // 为了避免CPU占用过高，让出当前线程的执行权
        std::thread::yield_now();
    }
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
            // 显示下载失败对话框
            show_dialog(
                &app_handle,
                &format!("下载失败: {}", err),
                MessageDialogKind::Error,
                "下载失败",
            );
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

    // 将解压任务添加到解压队列
    {
        let mut queue = EXTRACT_QUEUE.lock().unwrap();
        queue.queue.push_back(extract_task_clone);
        log_debug!(
            "解压任务 [{}] 添加到队列，当前队列长度: {}",
            extract_task_id,
            queue.queue.len()
        );

        // 如果解压队列尚未开始处理，启动处理
        if !queue.processing_started {
            log_info!(
                "解压队列处理未启动，现在启动，最大并发解压任务数: {}",
                queue.max_concurrent_tasks
            );
            // 启动解压队列处理（使用独立的异步任务，避免引用问题）
            tauri::async_runtime::spawn(async move {
                process_extract_queue().await;
            });
        }
    }

    // 确保解压队列正在运行
    {
        let queue = EXTRACT_QUEUE.lock().unwrap();
        if !queue.processing_started {
            // 释放锁后启动处理（避免死锁）
            drop(queue);

            log_debug!("解压队列处理未启动，立即启动");
            // 直接调用process_extract_queue，使用单独的线程避免阻塞当前线程
            std::thread::spawn(|| {
                tauri::async_runtime::block_on(process_extract_queue());
            });
        } else if !queue.queue.is_empty() && queue.active_tasks.is_empty() {
            // 安全检查：如果解压队列不为空，但活跃任务为0，可能表示处理逻辑出现问题
            log_warn!("解压队列有任务但无活跃任务，重置处理状态");
            // 释放锁后重新启动处理（避免死锁）
            drop(queue);

            // 尝试重置处理状态并重新启动
            let mut queue_reset = EXTRACT_QUEUE.lock().unwrap();
            queue_reset.processing_started = false;

            // 直接调用process_extract_queue，使用单独的线程避免阻塞当前线程
            std::thread::spawn(|| {
                log_debug!("重新启动解压队列处理线程");
                tauri::async_runtime::block_on(process_extract_queue());
            });
        }
    };

    // 返回下载成功的信息，并明确指出解压将异步进行
    Ok(format!("文件下载成功，解压将在后台进行: {}", file_path))
}
