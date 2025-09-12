// download_manager.rs 模块 - 负责管理地图文件的下载队列和下载/解压过程

// 标准库导入
use std::{
    collections::{HashSet, VecDeque},
    fs,
    io::Read,
    path::PathBuf,
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
    aria2c::download_via_aria2, dialog_manager::show_dialog, dir_manager::get_global_temp_dir,
    init::is_app_shutting_down, log_debug, log_error, log_info, log_warn,
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

/// 解压任务结构体 - 表示一个文件解压任务
#[derive(Debug, Clone)]
pub struct ExtractTask {
    /// 任务唯一标识符
    pub id: String,
    /// 要解压的文件路径
    pub file_path: String,
    /// 解压目标目录
    pub extract_dir: String,
    /// 应用句柄，用于发送事件通知
    pub app_handle: AppHandle,
    /// 下载任务ID，用于关联下载和解压操作
    pub download_task_id: String,
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

/// 解压队列结构体 - 管理解压任务的队列和处理状态
#[derive(Debug)]
pub struct ExtractQueue {
    /// 任务队列，按顺序存储待处理的解压任务
    pub queue: VecDeque<ExtractTask>,
    /// 标记队列处理是否已启动
    pub processing_started: bool,
    /// 最大并行解压任务数
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

    // 创建全局解压队列实例
    pub static ref EXTRACT_QUEUE: Arc<Mutex<ExtractQueue>> = Arc::new(Mutex::new(ExtractQueue {
        queue: VecDeque::new(),
        processing_started: false,
        max_concurrent_tasks: 1, // 限制为1个并发解压任务
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

/// 处理解压队列中的任务 - 持续监控队列并启动解压任务
///
/// 此函数会持续运行，定期检查解压队列并根据最大并发任务数启动新的解压任务。
/// 在处理每个任务时，会执行解压操作并发送解压完成的事件通知。
pub async fn process_extract_queue() {
    // 标记解压队列处理已启动
    {
        let mut queue = EXTRACT_QUEUE.lock().unwrap();
        queue.processing_started = true;
        log_info!(
            "解压队列处理已启动，最大并发解压任务数: {}",
            queue.max_concurrent_tasks
        );
    }

    // 创建一个持续运行的循环，定期检查队列并启动新任务
    loop {
        // 检查是否可以继续运行
        let (has_tasks, queue_len, active_tasks_len) = {
            let queue = EXTRACT_QUEUE.lock().unwrap();
            let queue_has_tasks = !queue.queue.is_empty();
            let active_has_tasks = !queue.active_tasks.is_empty();
            (
                queue_has_tasks || active_has_tasks,
                queue.queue.len(),
                queue.active_tasks.len(),
            )
        };

        // 仅在有任务时记录状态日志，避免无任务时空转日志
        if has_tasks {
            log_debug!(
                "解压队列状态检查: 队列长度={}, 活跃任务数={}",
                queue_len,
                active_tasks_len
            );
        }

        // 使用异步sleep避免阻塞运行时
        time::sleep(Duration::from_millis(1000)).await;

        // 如果没有任务，等待一小段时间后继续检查
        if !has_tasks {
            time::sleep(Duration::from_millis(50)).await;
            continue; // 继续下一轮循环，避免无任务时进行其他操作
        }

        // 尝试启动新的解压任务 - 优先处理没有aria2文件的任务
        let maybe_task = {
            let mut queue = EXTRACT_QUEUE.lock().unwrap();

            // 检查是否可以启动新任务
            if queue.active_tasks.len() < queue.max_concurrent_tasks as usize {
                // 首先尝试找到没有aria2文件的任务
                let index_without_aria2 = queue.queue.iter().position(|task| {
                    let aria2_file_path = {
                        let mut path = std::path::PathBuf::from(&task.file_path);
                        path.set_extension("7z.aria2");
                        path
                    };
                    // 返回没有aria2文件的任务
                    !aria2_file_path.exists()
                });

                let task = if let Some(index) = index_without_aria2 {
                    // 找到没有aria2文件的任务，从队列中移除
                    let task = queue.queue.remove(index).unwrap();
                    log_info!(
                        "优先处理没有aria2文件的解压任务 [{}] 关联下载任务 [{}]",
                        task.id,
                        task.download_task_id
                    );
                    Some(task)
                } else if let Some(task) = queue.queue.pop_front() {
                    // 没有找到无aria2文件的任务，按原顺序处理
                    Some(task)
                } else {
                    // 队列空了
                    None
                };

                if let Some(task) = task {
                    // 将任务添加到活跃任务集合
                    queue.active_tasks.insert(task.id.clone());
                    log_debug!("解压任务 [{}] 从队列移动到活跃任务集合，当前活跃任务数: {}, 剩余队列任务数: {}", 
                              task.id, queue.active_tasks.len(), queue.queue.len());
                    Some(task)
                } else {
                    // 队列空了但活跃任务可能还在处理中
                    log_debug!(
                        "解压队列已空但活跃任务数: {}, 继续等待任务完成",
                        queue.active_tasks.len()
                    );
                    None
                }
            } else {
                // 已达到最大并发任务数
                log_debug!(
                    "已达到最大并发解压任务数: {}, 等待任务完成",
                    queue.max_concurrent_tasks
                );
                None
            }
        };

        // 如果没有获取到任务，给一点时间让其他任务有机会运行
        if maybe_task.is_none() {
            time::sleep(Duration::from_millis(50)).await;
        }

        // 如果有任务可以启动，处理该任务
        if let Some(task) = maybe_task {
            let extract_task_id = task.id.clone();
            let download_task_id = task.download_task_id.clone();
            let file_path = task.file_path.clone();
            let extract_dir = task.extract_dir.clone();

            log_info!(
                "开始处理解压任务 [{}] 关联下载任务 [{}]: 文件={}, 解压目录={}",
                extract_task_id,
                download_task_id,
                file_path,
                extract_dir
            );

            // 在新的异步任务中处理解压操作
            tauri::async_runtime::spawn(async move {
                // 创建一个作用域来确保任务无论成功失败都会从活跃集合中移除
                {
                    // 获取文件名（从文件路径中提取）
                    let filename = std::path::Path::new(&task.file_path)
                        .file_name()
                        .and_then(|os_str| os_str.to_str())
                        .unwrap_or("未知文件")
                        .to_string();

                    // 执行解压操作前，检查.aria2临时文件是否存在
                    let aria2_file_path = {
                        let mut path = std::path::PathBuf::from(&task.file_path);
                        path.set_extension("7z.aria2");
                        path
                    };

                    // 如果存在.aria2临时文件，则等待其消失（优化等待逻辑）
                    if aria2_file_path.exists() {
                        log_debug!(
                            "解压任务 [{}]: 开始等待.aria2临时文件消失: {}",
                            extract_task_id,
                            aria2_file_path.display()
                        );

                        // 优化的等待逻辑：增加超时机制并提高检查频率
                        let max_wait_seconds = 180; // 最多等待3分钟
                        let mut wait_count = 0;
                        let start_time = std::time::Instant::now();

                        while aria2_file_path.exists() {
                            wait_count += 1;

                            // 检查是否超过最大等待时间
                            let elapsed = start_time.elapsed().as_secs();
                            if elapsed > max_wait_seconds {
                                log_warn!(
                                    "解压任务 [{}]: 等待.aria2文件超时 ({}/{}秒)，继续执行解压",
                                    extract_task_id,
                                    elapsed,
                                    max_wait_seconds
                                );
                                break;
                            }

                            if wait_count % 20 == 0 {
                                // 每10秒记录一次日志
                                log_debug!(
                                    "解压任务 [{}]: 已等待 {} 秒，.aria2文件仍存在: {}",
                                    extract_task_id,
                                    wait_count / 2,
                                    aria2_file_path.display()
                                );
                            }

                            // 每500毫秒检查一次文件是否存在
                            std::thread::sleep(std::time::Duration::from_millis(500));
                        }
                    } else {
                        log_debug!(
                            "解压任务 [{}]: 没有找到.aria2临时文件，可以直接解压",
                            extract_task_id
                        );
                    }

                    // 发送解压开始事件
                    let _ = task.app_handle.emit_to(
                        "main",
                        "extract-start",
                        &serde_json::json!(
                            {
                                "taskId": task.download_task_id,
                                "filename": filename,
                                "extractDir": task.extract_dir
                            }
                        ),
                    );

                    log_info!(
                        "解压任务 [{}]: .aria2临时文件已消失，开始解压文件: {}",
                        extract_task_id,
                        task.file_path
                    );
                    let result = extract_with_7zip(&task.file_path, &task.extract_dir);

                    // 定义最大重试次数和解压重试函数
                    const MAX_RETRY_COUNT: u32 = 3;
                    let mut retry_count = 0;
                    let mut final_result = result;

                    // 解压失败时进行重试
                    while final_result.is_err() && retry_count < MAX_RETRY_COUNT {
                        retry_count += 1;
                        log_warn!(
                            "解压任务 [{}] 失败，开始第 {} 次重试: {}",
                            extract_task_id,
                            retry_count,
                            final_result.unwrap_err()
                        );

                        // 等待一段时间后再重试
                        std::thread::sleep(std::time::Duration::from_secs(2 * retry_count as u64));

                        // 再次尝试解压
                        log_debug!(
                            "解压任务 [{}] 第 {} 次重试中...",
                            extract_task_id,
                            retry_count
                        );
                        final_result = extract_with_7zip(&task.file_path, &task.extract_dir);
                    }

                    // 只有在解压成功的情况下才删除临时文件
                    // 如果解压失败，保留临时文件以便可能的重试或手动处理
                    if final_result.is_ok() {
                        if let Err(e) = fs::remove_file(&task.file_path) {
                            log_warn!(
                                "解压任务 [{}]: 无法删除临时文件 {}: {}",
                                extract_task_id,
                                task.file_path,
                                e
                            );
                        } else {
                            log_debug!(
                                "解压任务 [{}]: 成功删除临时文件: {}",
                                extract_task_id,
                                task.file_path
                            );
                        }
                    } else {
                        log_warn!(
                            "解压任务 [{}]: 解压失败，保留临时文件以便排查: {}",
                            extract_task_id,
                            task.file_path
                        );
                    }

                    // 更新结果为最终尝试的结果
                    let result = final_result;

                    // 构建返回消息
                    let message = match &result {
                        Ok(msg) => {
                            if retry_count > 0 {
                                format!("{} (重试了{}次)", msg, retry_count)
                            } else {
                                msg.clone()
                            }
                        }
                        Err(e) => {
                            if retry_count >= MAX_RETRY_COUNT {
                                // 显示解压失败对话框
                                show_dialog(
                                    &task.app_handle,
                                    &format!("解压失败（已尝试{}次）: {}", MAX_RETRY_COUNT, e),
                                    MessageDialogKind::Error,
                                    "解压失败",
                                );
                                format!("解压失败（已尝试{}次）: {}", MAX_RETRY_COUNT, e)
                            } else {
                                e.to_string()
                            }
                        }
                    };

                    // 记录解压完成日志
                    if result.is_ok() {
                        log_info!("解压任务 [{}] 完成: {}", extract_task_id, message);
                    } else {
                        log_error!("解压任务 [{}] 失败: {}", extract_task_id, message);
                    }

                    // 发送解压完成事件通知
                    let _ = task.app_handle.emit_to(
                        "main",
                        "extract-complete",
                        &serde_json::json!(
                            {
                                "taskId": task.download_task_id,
                                "success": result.is_ok(),
                                "message": message,
                                "filename": "未知文件".to_string() // 文件名已在下载完成时处理
                            }
                        ),
                    );
                }

                // 确保任务完成后，从活跃任务集合中移除（无论解压成功或失败）
                match EXTRACT_QUEUE.lock() {
                    Ok(mut queue) => {
                        if queue.active_tasks.remove(&task.id) {
                            log_debug!(
                                "解压任务 [{}] 从活跃任务集合中成功移除，当前活跃解压任务数: {}",
                                extract_task_id,
                                queue.active_tasks.len()
                            );
                        } else {
                            log_warn!(
                                "解压任务 [{}] 未在活跃任务集合中找到，可能已被移除",
                                extract_task_id
                            );
                        }
                    }
                    Err(e) => {
                        log_error!("无法获取解压队列锁以移除任务 [{}]: {}", extract_task_id, e);
                    }
                }
            });
        }

        // 优化线程执行权让出，使用异步sleep替代yield_now，减少CPU占用
        time::sleep(Duration::from_millis(10)).await;
    }
}

// 嵌入7za.exe作为资源
const SEVENZ_EXE_BYTES: &[u8] = include_bytes!("../bin/7za.exe");

/// 从嵌入式资源中释放7za.exe到临时目录
///
/// 这是一个内部辅助函数，仅在需要时被调用
fn release_7za_exe() -> Result<PathBuf, String> {
    // 获取统一的临时目录
    let temp_dir = get_global_temp_dir()?;

    // 释放资源到临时文件
    let temp_path = temp_dir.join("7za.exe");
    if let Err(err) = fs::write(&temp_path, SEVENZ_EXE_BYTES) {
        log_error!("无法写入7za.exe到临时目录: {:?}", err);
        return Err(format!("无法写入7za.exe到临时目录: {:?}", err));
    }

    // 确保文件可执行
    #[cfg(windows)]
    unsafe {
        use std::os::windows::ffi::OsStrExt;
        use winapi::um::fileapi::SetFileAttributesW;
        use winapi::um::winnt::FILE_ATTRIBUTE_NORMAL;

        let path_wide: Vec<u16> = temp_path.as_os_str().encode_wide().chain(Some(0)).collect();
        SetFileAttributesW(path_wide.as_ptr(), FILE_ATTRIBUTE_NORMAL);
    }

    log_debug!("7za.exe已成功释放到临时目录: {}", temp_path.display());
    Ok(temp_path)
}

/// 使用7za.exe解压文件 - 将压缩文件解压到指定目录
///
/// 此函数通过调用7za.exe命令行工具执行文件解压操作。
/// 添加了7z文件格式预检查，避免尝试解压已损坏的文件。
///
/// # 参数
/// - `file_path`: 要解压的文件路径
/// - `extract_dir`: 解压目标目录
///
/// # 返回值
/// - 成功时返回包含解压成功信息的Ok
/// - 失败时返回包含错误信息的Err
pub fn extract_with_7zip(file_path: &str, extract_dir: &str) -> Result<String, String> {
    log_debug!("开始解压操作: 文件={}, 目标目录={}", file_path, extract_dir);

    // 检查文件是否存在
    let file = PathBuf::from(file_path);
    if !file.exists() {
        log_error!("解压失败: 文件不存在: {}", file_path);
        return Err(format!("文件不存在: {}", file_path));
    }

    // 检查文件大小是否合理
    let file_size = match file.metadata() {
        Ok(meta) => meta.len(),
        Err(e) => {
            log_error!("获取文件大小失败: {}", e);
            return Err(format!("获取文件大小失败: {}", e));
        }
    };

    if file_size < 1 {
        // 如果文件太小（小于1字节），很可能是损坏的
        log_error!("解压失败: 文件大小过小（{}字节），可能已损坏", file_size);
        return Err(format!("解压失败: 文件大小过小，可能已损坏"));
    }

    // 添加短暂延迟，确保文件系统有足够时间完成文件写入和释放锁定
    // 这解决了下载完成后立即尝试解压时可能遇到的文件访问问题
    log_debug!("文件存在且大小正常，等待100毫秒确保文件系统完成操作");
    std::thread::sleep(std::time::Duration::from_millis(100));

    // 预检查7z文件格式
    // 7z文件以0x37 0x7A 0xBC 0xAF 0x27 0x1C作为魔数
    // 优化：只读取文件头部而不是整个文件，提高效率并避免内存问题
    match std::fs::File::open(&file) {
        Ok(mut file_handle) => {
            let mut header = [0u8; 100];
            match file_handle.read(&mut header) {
                Ok(bytes_read) => {
                    if bytes_read >= 6 {
                        // 宽松验证：只检查前两个字节是否是'7z'
                        // 这样可以避免因编码或读取问题导致的误判，提高兼容性
                        if header[0] == 0x37 && header[1] == 0x7A {
                            log_debug!("文件通过7z魔数检查，确认是有效的7z文件格式");
                        } else {
                            log_warn!(
                                "7z文件魔数不匹配: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                                header[0],
                                header[1],
                                header[2],
                                header[3],
                                header[4],
                                header[5]
                            );
                            log_warn!("文件魔数不匹配，但继续尝试解压以提高兼容性");
                        }
                    } else {
                        log_warn!("读取的文件头部数据不足，无法完整检查魔数，但继续尝试解压");
                    }
                }
                Err(e) => {
                    log_warn!("无法读取文件内容进行预检查: {}, 继续尝试解压", e);
                }
            }
        }
        Err(e) => {
            log_warn!("无法打开文件进行预检查: {}, 继续尝试解压", e);
        }
    }

    // 确保解压目录存在，如果不存在则创建
    let extract_path = PathBuf::from(extract_dir);
    if !extract_path.exists() {
        log_debug!("解压目录不存在，尝试创建: {}", extract_dir);
        if let Err(e) = std::fs::create_dir_all(&extract_path) {
            log_error!("创建解压目录失败: {}", e);
            return Err(format!("创建解压目录失败: {}", e));
        }
        log_info!("解压目录创建成功: {}", extract_dir);
    } else {
        log_debug!("解压目录已存在: {}", extract_dir);
    }

    // 从嵌入式资源获取7za.exe
    log_debug!("尝试获取7za.exe...");
    let sevenz_exe_path = release_7za_exe()?;
    let sevenz_exe_str = sevenz_exe_path
        .to_str()
        .ok_or("无法将7za.exe路径转换为字符串".to_string())?;

    // 构建7za.exe命令行参数
    // 使用x命令解压，-y表示自动确认所有提示
    // 不再使用-o参数，而是将工作目录设置为解压目录
    let args = [
        "x",       // 解压命令
        "-y",      // 自动确认
        file_path, // 要解压的文件
    ];

    log_debug!("执行解压命令: {} {}", sevenz_exe_str, args.join(" "));
    log_debug!("设置工作目录为: {}", extract_dir);

    // 执行7za.exe命令，设置工作目录为解压目录
    let mut command = std::process::Command::new(sevenz_exe_str);
    command.args(&args).current_dir(extract_dir); // 设置工作目录为解压目录

    // 隐藏窗口运行
    use std::os::windows::process::CommandExt;

    command.creation_flags(0x08000000); // CREATE_NO_WINDOW 标志

    let output = command
        .output()
        .map_err(|e| format!("无法执行7za.exe命令: {}", e))?;

    // 检查命令执行结果
    if output.status.success() {
        // 检查解压目录是否有文件
        let has_files = match std::fs::read_dir(&extract_path) {
            Ok(mut entries) => entries.next().is_some(),
            Err(_) => false,
        };

        if has_files {
            log_info!("解压成功: 文件={}, 目标目录={}", file_path, extract_dir);
            return Ok(format!(
                "解压成功: {} 文件已解压到 {}",
                file_path, extract_dir
            ));
        } else {
            log_error!("解压失败: 解压目录为空，可能文件格式不支持");
            return Err("解压失败: 解压目录为空，可能文件格式不支持".to_string());
        }
    } else {
        // 解析错误输出
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        log_error!("7za.exe解压失败，stderr: {}, stdout: {}", stderr, stdout);
        return Err(format!("解压失败: {}\n\n详细信息:\n{}", stderr, stdout));
    }
}
