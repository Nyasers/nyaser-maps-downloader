// extract_manager.rs 模块 - 负责管理文件的解压队列和解压过程

// 标准库导入
use std::{fs, path::PathBuf};

// 第三方库导入
use serde_json;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::MessageDialogKind;

// 内部模块导入
use crate::{
    dialog_manager::show_dialog, download_manager::DOWNLOAD_QUEUE, init::is_app_shutting_down,
    log_debug, log_error, log_info, log_utils::redirect_process_output, log_warn,
    queue_manager::QueueManager,
};

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
    /// 压缩包名称（不含扩展名），用于创建子文件夹
    pub archive_name: String,
}

// 创建全局解压队列管理器实例和7z资源路径常量
lazy_static::lazy_static! {
    pub static ref EXTRACT_MANAGER: QueueManager<ExtractTask> = QueueManager::new(1);

    /// 7zG.exe（GUI版本）路径常量
    pub static ref SEVENZG_PATH: PathBuf = crate::get_assets_path("bin/7zG.exe").expect("无法获取7zG.exe路径");

    /// 7z.exe（命令行版本）路径常量
    pub static ref SEVENZ_PATH: PathBuf = crate::get_assets_path("bin/7z.exe").expect("无法获取7z.exe路径");
}

// 从路径获取文件名
fn get_filename_from_path(file_path: &str) -> String {
    std::path::Path::new(file_path)
        .file_name()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or("未知文件")
        .to_string()
}

// 构建aria2文件路径
fn build_aria2_file_path(file_path: &str) -> PathBuf {
    let path = PathBuf::from(file_path);
    let mut aria2_path = path.into_os_string();
    aria2_path.push(".aria2");
    PathBuf::from(aria2_path)
}

// 等待aria2文件消失
fn wait_for_aria2_file(
    aria2_file_path: &PathBuf,
    extract_task_id: String,
    download_task_id: String,
    task: &ExtractTask,
) {
    if !aria2_file_path.exists() {
        log_debug!(
            "解压任务 [{}]: 没有找到.aria2临时文件，可以直接解压",
            extract_task_id
        );
        return;
    }

    log_debug!(
        "解压任务 [{}]: 开始等待.aria2临时文件消失: {}",
        extract_task_id,
        aria2_file_path.display()
    );

    let max_wait_seconds = 180;
    let mut wait_count = 0;
    let start_time = std::time::Instant::now();

    let aria2_file_path_str = aria2_file_path.to_string_lossy().to_string();
    let app_handle_clone = task.app_handle.clone();
    let extract_task_id_clone = extract_task_id.clone();
    let download_task_id_clone = download_task_id.clone();

    while aria2_file_path.exists() {
        wait_count += 1;

        let elapsed = start_time.elapsed().as_secs();
        if elapsed > max_wait_seconds {
            log_warn!(
                "解压任务 [{}]: 等待.aria2文件超时 ({}/{})秒",
                extract_task_id,
                elapsed,
                max_wait_seconds
            );

            log_info!(
                "解压任务 [{}]: 尝试继续下载任务 [{}]",
                extract_task_id,
                download_task_id_clone
            );

            if let Ok(Some(download_task)) = find_download_task_by_id(&download_task_id_clone) {
                log_info!("找到下载任务 [{}]，开始继续下载", download_task_id_clone);

                let task_file_path_clone = task.file_path.clone();
                std::thread::spawn(move || {
                    let result = {
                        if let Ok(mut manager_guard) = crate::aria2c::ARIA2_RPC_MANAGER.lock() {
                            if let Some(manager) = &mut *manager_guard {
                                let path_buf = PathBuf::from(&aria2_file_path_str);
                                let filename = path_buf
                                    .file_stem()
                                    .and_then(|os_str| os_str.to_str())
                                    .unwrap_or("未知文件");

                                manager.add_download_sync(
                                    &download_task.url,
                                    &std::path::Path::new(&aria2_file_path_str)
                                        .parent()
                                        .map(|p| p.to_string_lossy().to_string())
                                        .unwrap_or("\\".to_string()),
                                    filename,
                                )
                            } else {
                                Err("ARIA2 RPC管理器未初始化".to_string())
                            }
                        } else {
                            Err("无法获取ARIA2 RPC管理器锁".to_string())
                        }
                    };

                    match result {
                        Ok(gid) => {
                            log_info!(
                                "解压任务 [{}]: 成功继续下载任务 [{}]，新的GID: {}",
                                extract_task_id_clone,
                                download_task_id_clone,
                                gid
                            );
                            let _ = app_handle_clone.emit_to(
                                "main",
                                "download-resumed",
                                &serde_json::json!(
                                    {
                                        "taskId": download_task_id_clone,
                                        "filename": PathBuf::from(&task_file_path_clone)
                                            .file_name()
                                            .and_then(|os_str| os_str.to_str())
                                            .unwrap_or("未知文件"),
                                        "message": "成功继续下载任务"
                                    }
                                ),
                            );
                        }
                        Err(e) => {
                            log_error!(
                                "解压任务 [{}]: 继续下载任务 [{}] 失败: {}",
                                extract_task_id_clone,
                                download_task_id_clone,
                                e
                            );
                        }
                    }
                });
            } else {
                log_warn!(
                    "解压任务 [{}]: 未找到对应的下载任务 [{}]",
                    extract_task_id,
                    download_task_id_clone
                );
            }

            log_warn!("解压任务 [{}]: 继续解压流程", extract_task_id);
            break;
        }

        if wait_count % 20 == 0 {
            log_debug!(
                "解压任务 [{}]: 已等待 {} 秒，.aria2文件仍存在: {}",
                extract_task_id,
                wait_count / 2,
                aria2_file_path.display()
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

// 发送解压开始事件
fn send_extract_start_event(task: &ExtractTask, filename: &str) {
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
}

// 发送解压完成事件
fn send_extract_complete_event(task: &ExtractTask, success: bool, message: &str) {
    let _ = task.app_handle.emit_to(
        "main",
        "extract-complete",
        &serde_json::json!(
            {
                "taskId": task.download_task_id,
                "success": success,
                "message": message,
                "filename": "未知文件".to_string()
            }
        ),
    );
}

// 清理临时文件
fn cleanup_temp_file(task: &ExtractTask, extract_task_id: &str, success: bool) {
    if success {
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
}

// 重试解压
fn retry_extract(
    task: &ExtractTask,
    extract_task_id: &str,
    initial_result: Result<String, String>,
) -> Result<String, String> {
    const MAX_RETRY_COUNT: u32 = 3;
    let mut retry_count = 0;
    let mut final_result = initial_result;

    while final_result.is_err() && retry_count < MAX_RETRY_COUNT {
        retry_count += 1;
        log_warn!(
            "解压任务 [{}] 失败，开始第 {} 次重试: {}",
            extract_task_id,
            retry_count,
            final_result.unwrap_err()
        );

        std::thread::sleep(std::time::Duration::from_secs(2 * retry_count as u64));

        log_debug!(
            "解压任务 [{}] 第 {} 次重试中...",
            extract_task_id,
            retry_count
        );
        final_result = extract_with_7zip(
            &task.file_path,
            &task.extract_dir,
            &task.archive_name,
            &task.download_task_id,
        );
    }

    final_result
}

// 构建返回消息
fn build_result_message(
    result: &Result<String, String>,
    retry_count: u32,
    max_retry_count: u32,
) -> String {
    match result {
        Ok(msg) => {
            if retry_count > 0 {
                format!("{} (重试了{}次)", msg, retry_count)
            } else {
                msg.clone()
            }
        }
        Err(e) => {
            if retry_count >= max_retry_count {
                format!("解压失败（已尝试{}次）: {}", max_retry_count, e)
            } else {
                e.to_string()
            }
        }
    }
}

// 处理解压任务
fn process_extract_task(task: ExtractTask, extract_task_id: &str, download_task_id: &str) {
    let filename = get_filename_from_path(&task.file_path);
    let aria2_file_path = build_aria2_file_path(&task.file_path);

    wait_for_aria2_file(
        &aria2_file_path,
        extract_task_id.to_string(),
        download_task_id.to_string(),
        &task,
    );

    send_extract_start_event(&task, &filename);

    log_info!(
        "解压任务 [{}]: .aria2临时文件已消失，开始解压文件: {}",
        extract_task_id,
        task.file_path
    );

    let result = extract_with_7zip(
        &task.file_path,
        &task.extract_dir,
        &task.archive_name,
        &task.download_task_id,
    );

    let final_result = retry_extract(&task, extract_task_id, result);

    let success = final_result.is_ok();
    cleanup_temp_file(&task, extract_task_id, success);

    const MAX_RETRY_COUNT: u32 = 3;
    let retry_count = if success { 0 } else { MAX_RETRY_COUNT };
    let message = build_result_message(&final_result, retry_count, MAX_RETRY_COUNT);

    if !success && retry_count >= MAX_RETRY_COUNT {
        show_dialog(
            &task.app_handle,
            &message,
            MessageDialogKind::Error,
            "解压失败",
        );
    }

    if success {
        log_info!("解压任务 [{}] 完成: {}", extract_task_id, message);
    } else {
        log_error!("解压任务 [{}] 失败: {}", extract_task_id, message);
    }

    send_extract_complete_event(&task, success, &message);
}

/// 通过ID查找下载任务
///
/// 在下载队列中查找指定ID的下载任务
///
/// # 参数
/// - `task_id`: 要查找的下载任务ID
///
/// # 返回值
/// - 成功时返回`Ok(Some(DownloadTask))`表示找到任务
/// - 成功时返回`Ok(None)`表示未找到任务
/// - 失败时返回包含错误信息的`Err`
pub fn find_download_task_by_id(
    task_id: &str,
) -> Result<Option<crate::download_manager::DownloadTask>, String> {
    // 获取下载队列的锁
    let queue = DOWNLOAD_QUEUE
        .lock()
        .map_err(|e| format!("无法获取下载队列锁: {:?}", e))?;

    // 使用之前添加的find_task_by_id方法查找任务
    if let Some(task) = queue.find_task(task_id) {
        // 找到任务，返回克隆的任务对象
        Ok(Some(task.clone()))
    } else {
        // 未找到任务
        Ok(None)
    }
}

/// 启动解压队列管理器 - 使用通用队列管理功能处理解压任务
///
/// 此函数使用QueueManager的start_processing方法来处理解压任务，
/// 同时保留了优先处理没有aria2文件的任务的特殊逻辑。
pub fn start_extract_queue_manager() {
    let process_task_fn = |_task_id: String, task: &ExtractTask| {
        let extract_task_id = task.id.clone();
        let download_task_id = task.download_task_id.clone();

        log_info!(
            "开始处理解压任务 [{}] 关联下载任务 [{}]: 文件={}, 解压目录={}",
            extract_task_id,
            download_task_id,
            task.file_path,
            task.extract_dir
        );

        let task = task.clone();
        let task_id = task.id.clone();

        tauri::async_runtime::spawn(async move {
            process_extract_task(task, &extract_task_id, &download_task_id);

            match EXTRACT_MANAGER.queue.lock() {
                Ok(mut queue) => {
                    queue.remove_active_task(&task_id);
                    log_debug!(
                        "解压任务 [{}] 从活跃任务集合中移除，当前活跃解压任务数: {}",
                        extract_task_id,
                        queue.active_tasks.len()
                    );
                }
                Err(e) => {
                    log_error!("无法获取解压队列锁以移除任务 [{}]: {}", extract_task_id, e);
                }
            }
        });
    };

    let should_continue_fn = || !is_app_shutting_down();

    EXTRACT_MANAGER.start_processing(process_task_fn, 1000, should_continue_fn);
}

/// 使用7zG.exe解压文件 - 将压缩文件解压到指定目录
///
/// 此函数通过调用7zG.exe GUI工具执行文件解压操作，并记录解压过程。
/// 添加了7z文件格式预检查，避免尝试解压已损坏的文件。
/// 包含7z.dll和中文语言支持文件以确保完整功能。
///
/// # 参数
/// - `file_path`: 要解压的文件路径
/// - `extract_dir`: 解压目标目录
/// - `archive_name`: 压缩包名称（不含扩展名），用于创建子文件夹
/// - `task_id`: 任务ID，用于标识当前解压任务
///
/// # 返回值
/// - 成功时返回包含解压成功信息的Ok
/// - 失败时返回包含错误信息的Err
pub fn extract_with_7zip(
    file_path: &str,
    extract_dir: &str,
    archive_name: &str,
    task_id: &str,
) -> Result<String, String> {
    log_debug!(
        "开始解压操作: 文件={}, 目标目录={}, 子文件夹={}",
        file_path,
        extract_dir,
        archive_name
    );

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
        log_error!("解压失败: 文件大小过小（{}字节），可能已损坏", file_size);
        return Err(format!("解压失败: 文件大小过小，可能已损坏"));
    }

    // 添加短暂延迟，确保文件系统有足够时间完成文件写入和释放锁定
    log_debug!("文件存在且大小正常，等待100毫秒确保文件系统完成操作");
    std::thread::sleep(std::time::Duration::from_millis(100));

    // 使用7z.exe命令行版本验证文件是否是有效的压缩文件
    log_debug!("使用7z l命令验证压缩文件格式: {}", file_path);

    let list_args = [
        "l",         // 列出命令
        "-sccUTF-8", // 设置控制台代码页为UTF-8
        file_path,   // 要检查的文件
    ];

    log_debug!(
        "执行验证命令: {} {}",
        SEVENZ_PATH.display(),
        list_args.join(" ")
    );

    let mut list_command = std::process::Command::new(SEVENZ_PATH.as_path());
    list_command.args(&list_args);
    list_command.stdout(std::process::Stdio::piped());
    list_command.stderr(std::process::Stdio::piped());

    let list_output = list_command
        .output()
        .map_err(|e| format!("无法执行7z l命令: {}", e))?;

    if !list_output.status.success() {
        let stderr = String::from_utf8_lossy(&list_output.stderr);
        log_error!("7z l命令失败，文件可能不是有效的压缩文件: {}", stderr);
        return Err(format!(
            "文件验证失败: 不是有效的压缩文件或文件已损坏\n\n详细信息:\n{}",
            stderr
        ));
    }

    log_debug!("文件验证成功，是有效的压缩文件");

    // extract_dir 已经是 maps 目录（例如 E:\NMD_Data\maps），直接使用
    let maps_dir = PathBuf::from(extract_dir);

    // 确保 maps 目录存在
    if !maps_dir.exists() {
        log_debug!("maps目录不存在，尝试创建: {}", maps_dir.display());
        if let Err(e) = std::fs::create_dir_all(&maps_dir) {
            log_error!("创建maps目录失败: {}", e);
            return Err(format!("创建maps目录失败: {}", e));
        }
        log_info!("maps目录创建成功: {}", maps_dir.display());
    } else {
        log_debug!("maps目录已存在: {}", maps_dir.display());
    }

    // 创建以压缩包名称命名的子文件夹
    let target_dir = maps_dir.join(archive_name);
    log_debug!("创建目标解压目录: {}", target_dir.display());

    // 如果目标目录已存在，先删除
    if target_dir.exists() {
        log_debug!("目标目录已存在，先删除: {}", target_dir.display());
        if let Err(e) = std::fs::remove_dir_all(&target_dir) {
            log_warn!("删除已存在的目标目录失败: {}", e);
            return Err(format!("删除已存在的目标目录失败: {}", e));
        }
    }

    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        log_error!("创建目标解压目录失败: {}", e);
        return Err(format!("创建目标解压目录失败: {}", e));
    }

    /*
    构建7zG.exe命令行参数
        使用x命令解压
        -y表示自动确认所有提示
        -sccUTF-8设置控制台代码页
    */
    let args = [
        "x",         // 解压命令
        "-y",        // 自动确认
        "-sccUTF-8", // 设置控制台代码页为UTF-8
        file_path,   // 要解压的文件
    ];

    log_debug!(
        "执行解压命令: {} {}",
        SEVENZG_PATH.display(),
        args.join(" ")
    );

    // 执行7zG.exe命令
    let mut command = std::process::Command::new(SEVENZG_PATH.as_path());
    command.args(&args);

    // 设置工作目录为目标目录，这样7z会直接解压到这里
    command.current_dir(&target_dir);
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    // 启动进程（非阻塞）
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动7zG.exe进程: {}", e))?;

    // 获取stdout和stderr流并进行日志记录
    let stdout = child.stdout.take().ok_or("无法获取stdout流")?;
    let stderr = child.stderr.take().ok_or("无法获取stderr流")?;

    redirect_process_output(stdout, stderr, format!("7zG[{}]", task_id));

    // 等待进程结束并获取退出状态
    let output = child
        .wait_with_output()
        .map_err(|e| format!("等待7zG.exe进程结束时出错: {}", e))?;

    // 检查命令执行结果
    if output.status.success() {
        // 检查解压目录是否有文件
        let file_count = match std::fs::read_dir(&target_dir) {
            Ok(entries) => entries.count(),
            Err(e) => {
                log_error!("读取解压目录失败: {}", e);
                return Err(format!("读取解压目录失败: {}", e));
            }
        };

        if file_count > 0 {
            log_info!(
                "解压成功: 文件={}, 目标目录={}, 共解压 {} 个文件",
                file_path,
                target_dir.display(),
                file_count
            );

            // 自动挂载解压的文件
            log_info!("开始自动挂载组: {}", archive_name);
            match crate::commands::mount_group(archive_name.to_string()) {
                Ok(msg) => {
                    log_info!("自动挂载成功: {}", msg);
                }
                Err(e) => {
                    log_warn!("自动挂载失败: {}", e);
                }
            }

            Ok(format!(
                "解压成功: {} 文件已解压到 {}，共解压 {} 个文件",
                file_path,
                target_dir.display(),
                file_count
            ))
        } else {
            log_error!("解压失败: 解压目录为空，可能文件格式不支持或文件已损坏");
            // 清理空目录
            if let Err(e) = std::fs::remove_dir_all(&target_dir) {
                log_warn!("无法删除空的解压目录: {}", e);
            }
            Err("解压失败: 解压目录为空，可能文件格式不支持或文件已损坏".to_string())
        }
    } else {
        // 解析错误输出
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        log_error!("7zG.exe解压失败，stderr: {}, stdout: {}", stderr, stdout);
        // 清理目录
        if let Err(e) = std::fs::remove_dir_all(&target_dir) {
            log_warn!("无法删除解压目录: {}", e);
        }
        Err(format!("解压失败: {}\n\n详细信息:\n{}", stderr, stdout))
    }
}
