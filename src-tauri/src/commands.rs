// commands.rs 模块 - 定义应用程序的Tauri命令，处理前端与后端的通信

// 第三方库导入
use serde_json;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::MessageDialogKind;
use uuid::Uuid;

// 内部模块导入
use crate::{
    dialog_manager::show_dialog,
    dir_manager::DIR_MANAGER,
    download_manager::{process_download, process_download_queue, DownloadTask, DOWNLOAD_QUEUE},
    handle_deep_link, log_debug, log_error, log_info, log_warn,
    utils::get_file_name,
};

/// 打开文件管理器窗口
///
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于获取窗口实例和发送事件
///
/// # 返回值
/// - 成功时返回包含成功信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command]
pub fn open_file_manager_window(app_handle: AppHandle) -> Result<String, String> {
    log_info!("接收到打开文件管理器窗口请求");

    match app_handle.get_webview_window("file_manager") {
        Some(window) => {
            // 显示窗口
            if let Err(e) = window.show() {
                log_error!("显示文件管理器窗口失败: {:?}", e);
                return Err(format!("显示窗口失败: {:?}", e));
            }

            // 将窗口置于前台
            if let Err(e) = window.set_focus() {
                log_error!("设置文件管理器窗口焦点失败: {:?}", e);
                // 这个错误不影响窗口打开，所以不返回Err
            }

            // 重置窗口状态到Normal（非最大/最小化）
            if let Err(e) = window.unmaximize() {
                log_error!("重置文件管理器窗口最大化状态失败: {:?}", e);
                // 这个错误不影响窗口打开，所以不返回Err
            }

            // 从最小化状态恢复
            if let Err(e) = window.unminimize() {
                log_error!("恢复文件管理器窗口最小化状态失败: {:?}", e);
                // 这个错误不影响窗口打开，所以不返回Err
            }

            // 相对于主窗口居中对齐
            if let Some(main_window) = app_handle.get_webview_window("main") {
                if let (Ok(main_pos), Ok(main_size), Ok(child_size)) = (
                    main_window.inner_position(),
                    main_window.inner_size(),
                    window.inner_size(),
                ) {
                    // 计算居中位置
                    let x = main_pos.x + ((main_size.width as i32 - child_size.width as i32) / 2);
                    let y = main_pos.y + ((main_size.height as i32 - child_size.height as i32) / 2);

                    // 设置居中位置
                    if let Err(e) = window
                        .set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }))
                    {
                        log_error!("设置文件管理器窗口居中位置失败: {:?}", e);
                    }
                }
            }

            // 发送自定义事件到file_manager窗口，触发文件列表刷新
            if let Err(e) =
                app_handle.emit_to("file_manager", "refresh-file-list", &serde_json::json!({}))
            {
                log_error!("发送刷新文件列表事件失败: {:?}", e);
            }

            log_info!("文件管理器窗口已成功打开并发送了刷新文件列表事件");
            Ok("文件管理器窗口已打开".to_string())
        }
        None => {
            log_error!("未找到文件管理器窗口");
            Err("未找到文件管理器窗口配置".to_string())
        }
    }
}

/// 获取 /maps 目录下的文件列表（按子文件夹分组）
///
/// # 返回值
/// - 成功时返回包含分组文件信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command]
pub fn get_maps(app_handle: AppHandle) -> Result<serde_json::Value, String> {
    log_info!("接收到获取maps文件列表请求");

    // 尝试从配置文件读取 nmd_data 目录
    let nmd_data_dir = crate::config_manager::get_data_dir(&app_handle)?;

    // 初始化目录管理器
    match DIR_MANAGER.lock() {
        Ok(mut manager) => {
            if manager.is_none() {
                // 根据配置创建目录管理器
                let dir_manager = if let Some(ref data_dir) = nmd_data_dir {
                    log_info!("使用配置的 nmd_data 目录: {}", data_dir);
                    crate::dir_manager::DirManager::with_nmd_data_dir(std::path::PathBuf::from(data_dir))
                } else {
                    // 没有配置 nmd_data 目录，弹窗要求配置
                    log_warn!("未配置 nmd_data 目录，弹窗要求配置");
                    show_dialog(
                        &app_handle,
                        "请先配置数据存储目录。\n\n在文件管理器窗口中点击\"修改目录\"按钮进行配置。",
                        MessageDialogKind::Warning,
                        "未配置数据目录",
                    );
                    return Err("未配置数据存储目录，请先配置".to_string());
                };

                *manager = Some(dir_manager.map_err(|e| {
                    log_error!("目录管理器初始化失败: {}", e);
                    e
                })?);
            }
        }
        Err(e) => {
            log_error!("无法锁定目录管理器: {:?}", e);
            return Err(format!("无法锁定目录管理器: {:?}", e));
        }
    };

    // 获取 nmd_data 目录路径
    let nmd_data_path = match nmd_data_dir {
        Some(data_dir) => std::path::PathBuf::from(data_dir),
        None => {
            log_error!("无法获取 nmd_data 目录");
            return Err("无法获取 nmd_data 目录".to_string());
        }
    };

    // 构建 /maps 目录路径
    let maps_dir = nmd_data_path.join("maps");
    log_info!("maps目录: {}", maps_dir.display());

    // 检查 /maps 目录是否存在
    if !maps_dir.exists() {
        log_info!("maps目录不存在，返回空列表");
        return Ok(serde_json::Value::Array(vec![]));
    }

    // 读取 /maps 目录下的所有子文件夹
    let groups = match std::fs::read_dir(&maps_dir) {
        Ok(dir_entries) => {
            let mut group_list = Vec::new();

            for entry in dir_entries {
                match entry {
                    Ok(entry) => {
                        let path = entry.path();
                        if path.is_dir() {
                            if let Some(folder_name) = path.file_name() {
                                let folder_name_str = folder_name.to_string_lossy().to_string();

                                // 读取子文件夹中的所有文件
                                let files = match std::fs::read_dir(&path) {
                                    Ok(file_entries) => {
                                        let mut file_list = Vec::new();

                                        for file_entry in file_entries {
                                            match file_entry {
                                                Ok(file_entry) => {
                                                    let file_path = file_entry.path();
                                                    if file_path.is_file() {
                                                        if let Some(file_name) =
                                                            file_path.file_name()
                                                        {
                                                            let file_name_str = file_name
                                                                .to_string_lossy()
                                                                .to_string();

                                                            // 获取文件大小
                                                            let size = match std::fs::metadata(
                                                                &file_path,
                                                            ) {
                                                                Ok(meta) => meta.len(),
                                                                Err(e) => {
                                                                    log_warn!(
                                                                        "获取文件大小失败: {}, 错误: {:?}",
                                                                        file_name_str,
                                                                        e
                                                                    );
                                                                    0
                                                                }
                                                            };

                                                            // 添加到文件列表
                                                            file_list.push(serde_json::json!({
                                                                "name": file_name_str,
                                                                "size": size
                                                            }));
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    log_warn!("读取文件项失败: {:?}", e);
                                                    continue;
                                                }
                                            }
                                        }

                                        file_list
                                    }
                                    Err(e) => {
                                        log_warn!("读取文件夹 {} 失败: {:?}", folder_name_str, e);
                                        Vec::new()
                                    }
                                };

                                // 如果文件夹中有文件，添加到分组列表
                                if !files.is_empty() {
                                    group_list.push(serde_json::json!({
                                        "name": folder_name_str,
                                        "files": files
                                    }));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log_warn!("读取目录项失败: {:?}", e);
                        continue;
                    }
                }
            }

            group_list
        }
        Err(e) => {
            log_error!("读取maps目录失败: {:?}", e);
            return Err(format!("读取目录失败: {:?}", e));
        }
    };

    log_info!("找到{}个分组", groups.len());
    Ok(serde_json::Value::Array(groups))
}

/// 打开服务器列表窗口
///
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于获取窗口实例
///
/// # 返回值
/// - 成功时返回包含成功信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command]
pub fn open_server_list_window(app_handle: AppHandle) -> Result<String, String> {
    log_info!("接收到打开服务器列表窗口请求");

    match app_handle.get_webview_window("server_list") {
        Some(window) => {
            // 显示窗口
            if let Err(e) = window.show() {
                log_error!("显示服务器列表窗口失败: {:?}", e);
                return Err(format!("显示窗口失败: {:?}", e));
            }

            // 将窗口置于前台
            if let Err(e) = window.set_focus() {
                log_error!("设置服务器列表窗口焦点失败: {:?}", e);
                // 这个错误不影响窗口打开，所以不返回Err
            }

            // 重置窗口状态到Normal（非最大/最小化）
            if let Err(e) = window.unmaximize() {
                log_error!("重置服务器列表窗口最大化状态失败: {:?}", e);
                // 这个错误不影响窗口打开，所以不返回Err
            }

            // 从最小化状态恢复
            if let Err(e) = window.unminimize() {
                log_error!("恢复服务器列表窗口最小化状态失败: {:?}", e);
                // 这个错误不影响窗口打开，所以不返回Err
            }

            // 相对于主窗口居中对齐
            if let Some(main_window) = app_handle.get_webview_window("main") {
                if let (Ok(main_pos), Ok(main_size), Ok(child_size)) = (
                    main_window.inner_position(),
                    main_window.inner_size(),
                    window.inner_size(),
                ) {
                    // 计算居中位置
                    let x = main_pos.x + ((main_size.width as i32 - child_size.width as i32) / 2);
                    let y = main_pos.y + ((main_size.height as i32 - child_size.height as i32) / 2);

                    // 设置居中位置
                    if let Err(e) = window
                        .set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }))
                    {
                        log_error!("设置服务器列表窗口居中位置失败: {:?}", e);
                    }
                }
            }

            // 从嵌入式资源中读取JavaScript代码并执行
            std::thread::spawn(move || {
                // 将字节数组转换为字符串
                let js_code =
                    match std::str::from_utf8(include_bytes!("../asset/serverlist/main.js")) {
                        Ok(content) => content.to_string(),
                        Err(e) => {
                            log_error!("无法解析serverlist/main.js文件内容: {:?}", e);
                            return;
                        }
                    };

                // 执行JavaScript代码
                if let Err(e) = window.eval(&js_code) {
                    log_error!("在服务器列表窗口执行JavaScript失败: {:?}", e);
                }
            });

            log_info!("服务器列表窗口已成功打开");
            Ok("服务器列表窗口已打开".to_string())
        }
        None => {
            log_error!("未找到服务器列表窗口");
            Err("未找到服务器列表窗口配置".to_string())
        }
    }
}

/// 删除指定的文件（在 /maps 目录下）
///
/// # 参数
/// - `group_name`: 文件所在的组名（子文件夹名）
/// - `file_name`: 要删除的文件名
///
/// # 返回值
/// - 成功时返回包含成功信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command]
pub fn delete_map_file(group_name: String, file_name: String) -> Result<String, String> {
    log_info!("接收到删除文件请求: 组={}, 文件={}", group_name, file_name);

    // 获取 nmd_data 目录
    let nmd_data_dir = match DIR_MANAGER.lock() {
        Ok(manager) => {
            if manager.is_none() {
                return Err("目录管理器未初始化".to_string());
            }

            // 获取目录路径并克隆它，避免生命周期问题
            manager.as_ref().unwrap().downloads_dir().parent().ok_or_else(|| {
                log_error!("无法获取 nmd_data 目录");
                "无法获取 nmd_data 目录".to_string()
            })?.to_path_buf()
        }
        Err(e) => {
            log_error!("无法锁定目录管理器: {:?}", e);
            return Err(format!("无法锁定目录管理器: {:?}", e));
        }
    };

    // 构建完整的文件路径：nmd_data/maps/group_name/file_name
    let file_path = nmd_data_dir.join("maps").join(&group_name).join(&file_name);

    // 检查文件是否存在
    if !file_path.exists() {
        log_error!("文件不存在: {}", file_path.display());
        return Err(format!("文件不存在: {}", file_name));
    }

    // 检查是否为文件
    if !file_path.is_file() {
        log_error!("指定的路径不是文件: {}", file_path.display());
        return Err(format!("指定的路径不是文件: {}", file_name));
    }

    // 删除文件
    if let Err(e) = std::fs::remove_file(&file_path) {
        log_error!("删除文件失败: {}, 错误: {:?}", file_path.display(), e);
        return Err(format!("删除文件失败: {:?}", e));
    }

    log_info!("文件已成功删除: {}", file_path.display());
    Ok(format!("文件 {} 已成功删除", file_name))
}

/// 下载函数 - 将地图下载任务添加到下载队列
///
/// # 参数
/// - `url`: 要下载的文件URL
/// - `path`: 下载完成后保存的文件路径
/// - `app_handle`: Tauri应用句柄，用于发送事件通知
///
/// # 返回值
/// - 成功时返回包含成功信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command(async)]
pub async fn install(
    url: &str,
    savepath: &str,
    saveonly: bool,
    app_handle: AppHandle,
) -> Result<String, String> {
    log_info!("接收到下载请求: URL={}, Path={}", url, savepath);

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

        // 尝试从配置文件读取 nmd_data 目录
        let nmd_data_dir = crate::config_manager::get_data_dir(&app_handle)?;

        // 根据配置创建目录管理器
        let dir_manager = if let Some(data_dir) = nmd_data_dir {
            log_info!("使用配置的 nmd_data 目录: {}", data_dir);
            crate::dir_manager::DirManager::with_nmd_data_dir(std::path::PathBuf::from(data_dir))
        } else {
            // 没有配置 nmd_data 目录，弹窗要求配置
            log_warn!("未配置 nmd_data 目录，弹窗要求配置");
            show_dialog(
                &app_handle,
                "请先配置数据存储目录。\n\n在文件管理器窗口中点击\"修改目录\"按钮进行配置。",
                MessageDialogKind::Warning,
                "未配置数据目录",
            );
            return Err("未配置数据存储目录，请先配置".to_string());
        };

        *manager = Some(dir_manager.map_err(|e| {
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

    // 获取解压目录路径 - 使用 nmd_data/maps 而不是 L4D2 的 addons 目录
    log_debug!("尝试获取解压目录...");
    let nmd_data_dir = manager
        .as_ref()
        .unwrap()
        .downloads_dir()
        .parent()
        .ok_or_else(|| {
            log_error!("无法获取 nmd_data 目录");
            "无法获取 nmd_data 目录".to_string()
        })?;
    
    let maps_dir = nmd_data_dir.join("maps");
    let extract_dir = maps_dir.to_string_lossy().to_string();
    log_info!("解压目录设置为: {}", extract_dir);

    // 生成唯一的任务ID
    let task_id = Uuid::new_v4().to_string();
    log_info!("生成任务ID: {}", task_id);

    // 尝试从URL中提取文件名
    let filename = get_file_name(url).unwrap_or_else(|| {
        log_error!("无法从URL中提取文件名: {}", url);
        // 显示错误对话框
        show_dialog(
            &app_handle,
            &format!("无法从URL中提取文件名: {}", url),
            MessageDialogKind::Error,
            "错误",
        );
        "unknown".to_string()
    });

    // 创建下载任务
    let task = DownloadTask {
        id: task_id.clone(),
        url: url.to_string(),
        savepath: Some(savepath.to_string()),
        saveonly: saveonly,
        extract_dir: extract_dir,
        filename: Some(filename.clone()),
    };
    log_info!("创建下载任务: ID={}, URL={}", task_id, url);

    // 添加任务到下载队列
    log_debug!("尝试锁定下载队列并添加任务...");
    {
        let mut queue = (&*DOWNLOAD_QUEUE).lock().unwrap();
        queue.add_task(task.id.clone(), task);
        log_info!(
            "任务已添加到下载队列，当前队列长度: {}",
            queue.waiting_tasks.len()
        );
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
        if !queue.waiting_tasks.is_empty()
            && queue.active_tasks.is_empty()
            && queue.processing_started
        {
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
    let total_tasks = {
        let queue = (&*DOWNLOAD_QUEUE).lock().unwrap();
        let size = queue.waiting_tasks.len();
        let active = queue.active_tasks.len();
        let total = size + active;

        log_debug!(
            "当前队列中有 {} 个等待任务，{} 个活跃任务，总共 {} 个任务",
            size,
            active,
            total
        );
        total
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

/// 取消下载任务 - 从下载队列中移除指定的下载任务
///
/// 此函数会取消指定ID的下载任务，
/// 如果任务正在下载，则通过aria2c取消下载，
/// 如果任务在等待队列中，则直接从队列中移除。
///
/// # 参数
/// - `task_id`: 要取消的下载任务的唯一标识符
/// - `app_handle`: Tauri应用句柄，用于发送事件通知
///
/// # 返回值
/// - 成功时返回包含成功信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command(async)]
pub async fn cancel_download(
    task_id: &str,
    app_handle: AppHandle,
    reason: Option<&str>,
) -> Result<String, String> {
    // 处理取消下载原因，如果没有提供则默认为普通取消
    let cancel_reason = reason.unwrap_or("normal");
    log_info!(
        "接收到取消下载任务请求: 任务ID={}, 原因={}",
        task_id,
        cancel_reason
    );

    // 检查并处理等待队列中的任务
    let mut queue = (&*DOWNLOAD_QUEUE).lock().unwrap();

    // 查找并移除队列中的任务
    let original_len = queue.waiting_tasks.len();
    queue.waiting_tasks.retain(|task| task != task_id);

    if original_len != queue.waiting_tasks.len() {
        log_info!("任务 {} 已从等待队列中移除", task_id);
    }

    // 检查任务是否在活跃任务中
    let task_in_active = queue.active_tasks.iter().any(|task| task == task_id);
    if task_in_active {
        log_info!("任务 {} 正在下载中，需要通过aria2c取消", task_id);

        // 从活跃任务中移除，避免重复处理
        if let Some(index) = queue.active_tasks.iter().position(|task| task == task_id) {
            queue.active_tasks.remove(index);
        }

        // 将任务ID和取消原因添加到取消下载请求列表
        if let Ok(mut cancel_requests) = crate::aria2c::CANCEL_DOWNLOAD_REQUESTS.lock() {
            cancel_requests.insert(task_id.to_string(), cancel_reason.to_string());
            log_info!(
                "已将任务ID {} 添加到取消下载请求列表，取消原因: {}",
                task_id,
                cancel_reason
            );
        } else {
            log_error!("无法锁定取消下载请求列表");
        }

        // 发送取消下载事件给前端，包含任务ID
        let _ = app_handle.emit_to(
            "main",
            "download-cancel-requested",
            &serde_json::json!({ "taskId": task_id }),
        );
    }

    log_info!("取消下载任务处理完成: 任务ID={}", task_id);
    Ok(format!("已成功请求取消下载任务: {}", task_id))
}

/// 刷新下载队列状态 - 获取当前队列状态并向前端发送更新
///
/// 此函数会获取当前下载队列的状态（等待任务、总任务数和活跃任务数），
/// 并向前端发送队列更新事件，用于刷新前端显示。
///
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于发送事件通知
///
/// # 返回值
/// - 成功时返回包含成功信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command(async)]
pub async fn refresh_download_queue(app_handle: AppHandle) -> Result<String, String> {
    log_info!("接收到刷新下载队列请求");

    // 发送队列更新事件通知
    let (total_tasks, active_tasks, waiting_tasks) = {
        let queue = (&*DOWNLOAD_QUEUE).lock().unwrap();
        let active = queue
            .active_tasks
            .iter()
            .filter_map(|task_id| {
                queue.tasks.get(task_id).map(|task| {
                    serde_json::json!({"id": task.id, "url": task.url, "filename": task.filename})
                })
            })
            .collect::<Vec<_>>();

        // 构建等待任务列表（转换为可序列化的格式）
        let tasks = queue
            .waiting_tasks
            .iter()
            .filter_map(|task_id| {
                queue.tasks.get(task_id).map(|task| {
                    serde_json::json!({"id": task.id, "url": task.url, "filename": task.filename})
                })
            })
            .collect::<Vec<_>>();

        let total = active.len() + tasks.len();

        (total, active, tasks)
    };

    let _ = app_handle.emit_to(
        "main",
        "download-queue-update",
        &serde_json::json!({
            "queue": {"waiting_tasks": waiting_tasks,
                       "total_tasks": total_tasks,
                       "active_tasks": active_tasks}
        }),
    );

    log_info!(
        "刷新下载队列处理完成: 等待任务数={}, 活跃任务数={}, 总任务数={}",
        waiting_tasks.len(),
        active_tasks.len(),
        total_tasks
    );
    Ok(format!(
        "成功刷新下载队列，等待任务数: {}, 活跃任务数: {}",
        waiting_tasks.len(),
        active_tasks.len()
    ))
}

/// 取消所有排队任务但保留当前正在下载的任务
#[tauri::command(async)]
pub async fn cancel_all_downloads(app_handle: AppHandle) -> Result<String, String> {
    log_info!("接收到取消所有排队任务请求");

    let queue_tasks_count;

    // 只处理等待队列，保留活跃任务
    {
        let mut queue = (&*DOWNLOAD_QUEUE).lock().unwrap();

        // 记录等待队列中的任务数量
        queue_tasks_count = queue.waiting_tasks.len();

        // 清空等待队列
        queue.waiting_tasks.clear();

        // 获取活跃任务数量
        let active_tasks_count = queue.active_tasks.len();

        log_info!(
            "已清空下载队列中的等待任务: {}个任务被取消，保留{}个活跃任务",
            queue_tasks_count,
            active_tasks_count
        );
    }

    refresh_download_queue(app_handle).await?;

    log_info!(
        "取消所有排队任务处理完成: 共取消 {} 个任务",
        queue_tasks_count,
    );
    Ok(format!(
        "已成功取消所有排队任务，共取消 {} 个任务",
        queue_tasks_count
    ))
}

/// 前端加载完成通知命令
///
/// 由前端调用，通知后端下载拦截器已成功加载完成
#[tauri::command]
pub fn frontend_loaded() -> Result<String, String> {
    log_info!("接收到前端加载完成通知");

    process_download().map_err(|e| e.to_string())?;

    Ok("前端加载完成通知已收到".into())
}

#[tauri::command]
pub fn deep_link_ready(handle: AppHandle) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let args = std::env::args().collect::<Vec<_>>();
        handle_deep_link(handle.clone(), args);
    });
}
