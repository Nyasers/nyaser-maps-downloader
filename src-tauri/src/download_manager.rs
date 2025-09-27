// download_manager.rs 模块 - 负责管理地图文件的下载队列和下载过程

// 标准库导入
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

// 第三方库导入
use regex;
use serde_json;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::MessageDialogKind;

// 内部模块导入
use crate::{
    aria2c::download_via_aria2,
    dialog_manager::show_dialog,
    extract_manager::{start_extract_queue_manager, ExtractTask},
    init::is_app_shutting_down,
    log_debug, log_error, log_info, log_warn,
    queue_manager::{process_queue, TaskQueue},
};

/// 下载任务结构体 - 表示一个地图下载任务的基本信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    pub static ref DOWNLOAD_QUEUE: Arc<Mutex<TaskQueue<DownloadTask>>> =
        Arc::new(Mutex::new(TaskQueue::new(1)));

    // 添加全局HashMap来跟踪完整的活跃任务信息
    pub static ref ACTIVE_DOWNLOAD_TASKS: Arc<Mutex<std::collections::HashMap<String, DownloadTask>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
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

        // 将任务添加到全局活跃任务集合中
        {
            let mut active_tasks = (&*ACTIVE_DOWNLOAD_TASKS)
                .lock()
                .map_err(|e| format!("无法获取活跃任务锁: {:?}", e))
                .expect("获取活跃任务锁失败");
            active_tasks.insert(task_id.clone(), task.clone());
            log_debug!(
                "下载任务 [{}] 添加到活跃任务集合，当前活跃任务数: {}",
                task_id,
                active_tasks.len()
            );
        }

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

                // 同时从全局活跃任务HashMap中移除
                let mut active_tasks = (&*ACTIVE_DOWNLOAD_TASKS)
                    .lock()
                    .map_err(|e| format!("无法获取活跃任务锁: {:?}", e))
                    .expect("获取活跃任务锁失败");
                active_tasks.remove(&task_id);
                log_debug!(
                    "下载任务 [{}] 从全局活跃任务HashMap中移除，当前活跃任务数: {}",
                    task_id,
                    active_tasks.len()
                );
            }

            // 发送队列更新事件，确保前端正确更新队列显示
            let (total_tasks, _queue_size, active_tasks_count, waiting_tasks) = {
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

                (total, size, active, tasks)
            };

            let _ = app_clone.emit_to(
                "main",
                "download-queue-update",
                &serde_json::json!({
                    "queue": {"waiting_tasks": waiting_tasks,
                    "total_tasks": total_tasks,
                    "active_tasks": active_tasks_count}
                }),
            );

            // 在开始下一个任务前增加等待时间（1秒）
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
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

/// 获取下载队列配置文件路径
///
/// 返回下载队列配置文件的完整路径
pub fn get_download_queue_file_path() -> Result<PathBuf, String> {
    // 从全局应用句柄获取AppHandle实例
    let binding = crate::init::GLOBAL_APP_HANDLE
        .read()
        .map_err(|e| format!("无法获取应用句柄: {:?}", e))?;
    let app_handle = match &*binding {
        Some(handle) => handle,
        None => return Err("全局应用句柄未初始化".to_string()),
    };

    // 使用AppHandle获取应用本地数据目录
    let app_data_dir = match app_handle.path().app_local_data_dir() {
        Ok(it) => it,
        Err(err) => return Err(format!("无法获取应用数据目录: {:?}", err)),
    };

    // 确保应用数据目录存在
    std::fs::create_dir_all(&app_data_dir).map_err(|e| format!("无法创建应用数据目录: {:?}", e))?;

    // 返回下载队列文件路径
    Ok(app_data_dir.join("download_queue.json"))
}

/// 保存下载队列到文件
///
/// 此函数将当前下载队列中的活跃任务和等待任务保存到文件，以便应用重启后能够恢复
pub fn save_download_queue() -> Result<(), String> {
    log_info!("开始保存下载队列...");

    // 获取队列配置文件路径
    let queue_file_path = get_download_queue_file_path()?;

    // 获取下载队列的等待任务
    let waiting_tasks = {
        let queue = DOWNLOAD_QUEUE
            .lock()
            .map_err(|e| format!("无法获取下载队列锁: {:?}", e))?;
        queue.queue.clone()
    };

    // 获取完整的活跃任务信息
    let active_tasks = {
        let tasks = ACTIVE_DOWNLOAD_TASKS
            .lock()
            .map_err(|e| format!("无法获取活跃下载任务锁: {:?}", e))?;
        tasks.values().cloned().collect::<Vec<_>>()
    };

    // 创建一个包含所有任务的统一数组，active任务放在前面
    let mut tasks = Vec::new();
    tasks.extend(active_tasks.clone()); // 先添加活跃任务
    tasks.extend(waiting_tasks); // 再添加等待任务

    // 在保存前处理URL替换：将 https://op.nyase.ru/(.+)?(.*) 替换为 https://maps.nyase.ru/d/$1
    for task in &mut tasks {
        if task.url.starts_with("https://op.nyase.ru/") {
            // 先保存原始URL到临时变量，避免借用冲突
            let original_url = task.url.clone();

            // 使用正则表达式进行替换
            if let Some(captures) = regex::Regex::new(r"https://op\.nyase\.ru/(.+?)(\?.*)?$")
                .unwrap()
                .captures(&original_url)
            {
                if let Some(captured) = captures.get(1) {
                    // 保存捕获的部分到临时变量
                    let captured_path = captured.as_str().to_string();

                    // 创建新的URL
                    task.url = format!("https://maps.nyase.ru/d/{}", captured_path);
                    log_debug!("已将URL从 '{}' 替换为 '{}'", original_url, task.url);
                }
            }
        }
    }

    // 创建一个只包含tasks字段的结构体
    #[derive(serde::Serialize)]
    struct SavedQueue {
        tasks: Vec<DownloadTask>,
    }

    let saved_queue = SavedQueue { tasks };

    // 如果没有任务，则不创建配置文件
    if saved_queue.tasks.is_empty() {
        // 如果配置文件存在，则删除它
        if queue_file_path.exists() {
            if let Err(e) = fs::remove_file(&queue_file_path) {
                log_warn!("无法删除空的下载队列配置文件: {:?}", e);
            }
        }
        log_info!("下载队列为空，无需保存");
        return Ok(());
    }

    // 将队列数据序列化为JSON
    let json_data = serde_json::to_string_pretty(&saved_queue)
        .map_err(|e| format!("无法序列化下载队列: {:?}", e))?;

    // 写入文件
    fs::write(&queue_file_path, json_data)
        .map_err(|e| format!("无法写入下载队列配置文件: {:?}", e))?;

    log_info!(
        "下载队列已成功保存到: {}, 总任务数: {}, 活跃任务数: {}",
        queue_file_path.to_string_lossy(),
        saved_queue.tasks.len(),
        active_tasks.len()
    );
    Ok(())
}

/// 从文件加载下载队列
///
/// 此函数在应用启动时调用，尝试从文件恢复之前的下载队列
pub fn load_download_queue() -> Result<(), String> {
    log_info!("开始加载下载队列...");

    // 获取队列配置文件路径
    let queue_file_path = get_download_queue_file_path()?;

    // 检查配置文件是否存在
    if !queue_file_path.exists() {
        log_info!("下载队列配置文件不存在，无需加载");
        return Ok(());
    }

    // 读取配置文件
    let json_data = fs::read_to_string(&queue_file_path)
        .map_err(|e| format!("无法读取下载队列配置文件: {:?}", e))?;

    // 定义队列结构 - 只包含一个tasks字段
    #[derive(serde::Deserialize)]
    struct SavedQueue {
        tasks: Vec<DownloadTask>,
    }

    // 反序列化为新格式，不支持旧格式
    let saved_queue: SavedQueue =
        serde_json::from_str(&json_data).map_err(|e| format!("无法反序列化下载队列: {:?}", e))?;

    log_info!("成功加载下载队列: 总任务数={}", saved_queue.tasks.len());

    // 将任务添加到下载队列
    if !saved_queue.tasks.is_empty() {
        let mut queue = DOWNLOAD_QUEUE
            .lock()
            .map_err(|e| format!("无法获取下载队列锁: {:?}", e))?;

        // 添加所有任务
        for task in saved_queue.tasks {
            queue.add_task(task);
        }

        let loaded_tasks_count = queue.queue.len();

        log_info!(
            "成功加载下载队列: 等待任务数={}, 总计需要重新下载的任务数={}",
            loaded_tasks_count,
            loaded_tasks_count
        );
    } else {
        log_info!("加载的下载队列为空");
    }

    Ok(())
}

/// 处理下载队列 - 启动下载任务处理线程
///
/// 此函数会在后台线程中启动一个异步任务，用于处理下载队列中的任务。
/// 它会先更新下载队列状态，然后开始处理队列中的任务。
pub fn process_download() -> Result<(), String> {
    // 从全局应用句柄获取AppHandle实例
    let binding = crate::init::GLOBAL_APP_HANDLE
        .read()
        .map_err(|e| format!("无法获取应用句柄: {:?}", e))?;
    let app_handle = match &*binding {
        Some(handle) => handle,
        None => return Err("全局应用句柄未初始化".into()),
    };

    let app_clone = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        update_download_queue_status(&app_clone);
        tauri::async_runtime::spawn(async move {
            log_debug!("下载队列处理线程已创建，准备开始处理队列");
            process_download_queue(app_clone).await;
        });
    });
    Ok(())
}

/// 更新下载队列状态 - 获取当前队列状态并向前端发送更新
///
/// 此函数会获取当前下载队列的状态（等待任务、总任务数和活跃任务数），
/// 并向前端发送队列更新事件，用于刷新前端显示。
///
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于发送事件通知
pub fn update_download_queue_status(app_handle: &AppHandle) {
    log_info!("更新下载队列状态");

    // 获取队列状态信息
    let (total_tasks, active_tasks_count, waiting_tasks) = {
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

        (total, active, tasks)
    };

    // 发送队列更新事件通知
    let _ = app_handle.emit_to(
        "main",
        "download-queue-update",
        &serde_json::json!({
            "queue": {"waiting_tasks": waiting_tasks,
                       "total_tasks": total_tasks,
                       "active_tasks": active_tasks_count}
        }),
    );

    log_debug!(
        "下载队列状态更新完成: 等待任务数={}, 活跃任务数={}, 总任务数={}",
        waiting_tasks.len(),
        active_tasks_count,
        total_tasks
    );
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
                show_dialog(&app_handle, &err, MessageDialogKind::Error, "下载失败");
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
