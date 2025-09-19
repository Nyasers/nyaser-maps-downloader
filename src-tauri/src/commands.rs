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

// 根据构建模式选择使用的HTML文件
// 在debug模式下使用未压缩的.html文件，在release模式下使用压缩的.min.html文件
const DOWNLOAD_INTERCEPTOR_HTML: &[u8] = {
    #[cfg(debug_assertions)]
    {
        include_bytes!("../html/middleware.html")
    }

    #[cfg(not(debug_assertions))]
    {
        include_bytes!("../dist/middleware.html")
    }
};

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

/// 打开外部链接函数 - 在默认浏览器中打开指定的URL
///
/// # 参数
/// - `url`: 要打开的URL
///
/// # 返回值
/// - 成功时返回包含成功信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command]
pub fn open_external_link(url: &str) -> Result<String, String> {
    log_info!("接收到打开外部链接请求: URL={}", url);

    // 在Windows上使用ShellExecuteA打开URL
    #[cfg(target_os = "windows")]
    {
        use std::ffi::CString;
        use winapi::um::shellapi::ShellExecuteA;
        use winapi::um::winuser::SW_SHOW;

        let operation = CString::new("open").unwrap();
        let url_c = CString::new(url).unwrap();
        let result = unsafe {
            ShellExecuteA(
                std::ptr::null_mut(),
                operation.as_ptr(),
                url_c.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOW,
            )
        };

        if result as i32 > 32 {
            log_info!("成功打开外部链接: URL={}", url);
            Ok("链接已成功打开".to_string())
        } else {
            log_error!("打开外部链接失败: URL={}, 错误代码={:?}", url, result);
            Err(format!("无法打开链接，错误代码: {:?}", result))
        }
    }

    // 其他平台的实现可以在这里添加
    #[cfg(not(target_os = "windows"))]
    {
        log_error!("当前平台不支持打开外部链接");
        Err("当前平台不支持打开外部链接".to_string())
    }
}

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

/// 获取解压目录下以$nmd_开头的文件列表
///
/// # 返回值
/// - 成功时返回包含文件信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command]
pub fn get_nmd_files() -> Result<serde_json::Value, String> {
    log_info!("接收到获取nmd文件列表请求");

    // 获取解压目录
    let extract_dir = match DIR_MANAGER.lock() {
        Ok(mut manager) => {
            if manager.is_none() {
                *manager = Some(crate::dir_manager::DirManager::new().map_err(|e| {
                    log_error!("目录管理器初始化失败: {}", e);
                    e
                })?);
            }

            // 获取目录路径并克隆它，避免生命周期问题
            match manager.as_mut().unwrap().extract_dir() {
                Some(dir) => dir.to_path_buf(),
                None => {
                    log_error!("无法获取解压目录");
                    return Err("无法获取解压目录".to_string());
                }
            }
        }
        Err(e) => {
            log_error!("无法锁定目录管理器: {:?}", e);
            return Err(format!("无法锁定目录管理器: {:?}", e));
        }
    };

    log_info!("解压目录: {}", extract_dir.display());

    // 读取目录并过滤出以$nmd_开头的文件
    let files = match std::fs::read_dir(extract_dir) {
        Ok(dir_entries) => {
            let mut file_list = Vec::new();

            for entry in dir_entries {
                match entry {
                    Ok(entry) => {
                        let path = entry.path();
                        if let Some(file_name) = path.file_name() {
                            let file_name_str = file_name.to_string_lossy();
                            // 检查文件名是否以$nmd_开头且是文件（不是目录）
                            if file_name_str.starts_with("$nmd_") && path.is_file() {
                                // 获取文件大小
                                let size = match std::fs::metadata(&path) {
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
                        log_warn!("读取目录项失败: {:?}", e);
                        continue;
                    }
                }
            }

            file_list
        }
        Err(e) => {
            log_error!("读取解压目录失败: {:?}", e);
            return Err(format!("读取目录失败: {:?}", e));
        }
    };

    log_info!("找到{}个以$nmd_开头的文件", files.len());
    Ok(serde_json::Value::Array(files))
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

            log_info!("服务器列表窗口已成功打开");
            Ok("服务器列表窗口已打开".to_string())
        }
        None => {
            log_error!("未找到服务器列表窗口");
            Err("未找到服务器列表窗口配置".to_string())
        }
    }
}

/// 删除指定的nmd文件
///
/// # 参数
/// - `file_name`: 要删除的文件名
///
/// # 返回值
/// - 成功时返回包含成功信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command]
pub fn delete_nmd_file(file_name: String) -> Result<String, String> {
    log_info!("接收到删除nmd文件请求: {}", file_name);

    // 验证文件名格式
    if !file_name.starts_with("$nmd_") {
        log_error!("无效的文件名格式: {}", file_name);
        return Err("只能删除以$nmd_开头的文件".to_string());
    }

    // 获取解压目录
    let extract_dir = match DIR_MANAGER.lock() {
        Ok(mut manager) => {
            if manager.is_none() {
                *manager = Some(crate::dir_manager::DirManager::new().map_err(|e| {
                    log_error!("目录管理器初始化失败: {}", e);
                    e
                })?);
            }

            // 获取目录路径并克隆它，避免生命周期问题
            match manager.as_mut().unwrap().extract_dir() {
                Some(dir) => dir.to_path_buf(),
                None => {
                    log_error!("无法获取解压目录");
                    return Err("无法获取解压目录".to_string());
                }
            }
        }
        Err(e) => {
            log_error!("无法锁定目录管理器: {:?}", e);
            return Err(format!("无法锁定目录管理器: {:?}", e));
        }
    };

    // 构建完整的文件路径
    let file_path = extract_dir.join(&file_name);

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
/// - `app_handle`: Tauri应用句柄，用于发送事件通知
///
/// # 返回值
/// - 成功时返回包含成功信息的Ok
/// - 失败时返回包含错误信息的Err
#[tauri::command(async)]
pub async fn install(url: &str, app_handle: AppHandle) -> Result<String, String> {
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
    let filename = url
        .split('/')
        .last()
        .map(|s| s.to_string())
        .unwrap()
        .split('?')
        .next()
        .map(|s| s.to_string());
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
