// extract_manager.rs 模块 - 负责管理文件的解压队列和解压过程

// 标准库导入
use std::{fs, io::Read, path::PathBuf};

// 第三方库导入
use serde_json;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::MessageDialogKind;

// 内部模块导入
use crate::{
    dialog_manager::show_dialog, dir_manager::DIR_MANAGER, download_manager::DOWNLOAD_QUEUE,
    init::is_app_shutting_down, log_debug, log_error, log_info, log_utils::redirect_process_output,
    log_warn, queue_manager::QueueManager,
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

// 创建全局解压队列管理器实例
lazy_static::lazy_static! {
    pub static ref EXTRACT_MANAGER: QueueManager<ExtractTask> = QueueManager::new(1);
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
    // 处理单个解压任务的函数
    let process_task_fn = |_task_id: String, task: &ExtractTask| {
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

        let task = task.clone();

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

                    // 在循环外部创建必要的克隆
                    let aria2_file_path_str = aria2_file_path.to_string_lossy().to_string();
                    let app_handle_clone = task.app_handle.clone();
                    let extract_task_id_clone = extract_task_id.clone();
                    let download_task_id_clone = task.download_task_id.clone();

                    while aria2_file_path.exists() {
                        wait_count += 1;

                        // 检查是否超过最大等待时间
                        let elapsed = start_time.elapsed().as_secs();
                        if elapsed > max_wait_seconds {
                            log_warn!(
                                "解压任务 [{}]: 等待.aria2文件超时 ({}/{})秒",
                                extract_task_id,
                                elapsed,
                                max_wait_seconds
                            );

                            // 尝试查找相关的下载任务，实现继续下载功能
                            log_info!(
                                "解压任务 [{}]: 尝试继续下载任务 [{}]",
                                extract_task_id,
                                download_task_id_clone
                            );

                            // 使用之前添加的find_task_by_id方法查找下载任务
                            if let Ok(Some(download_task)) =
                                find_download_task_by_id(&download_task_id_clone)
                            {
                                log_info!(
                                    "找到下载任务 [{}]，开始继续下载",
                                    download_task_id_clone
                                );

                                // 创建新的线程来继续下载任务
                                // 克隆task.file_path以避免所有权问题
                                let task_file_path_clone = task.file_path.clone();
                                std::thread::spawn(move || {
                                    // 从全局ARIA2_RPC_MANAGER获取实例
                                    let result = {
                                        if let Ok(mut manager_guard) =
                                            crate::aria2c::ARIA2_RPC_MANAGER.lock()
                                        {
                                            if let Some(manager) = &mut *manager_guard {
                                                // 从文件路径中提取文件名
                                                // 使用let绑定创建持久的PathBuf
                                                let path_buf = PathBuf::from(&aria2_file_path_str);
                                                let filename = path_buf
                                                    .file_stem()
                                                    .and_then(|os_str| os_str.to_str())
                                                    .unwrap_or("未知文件");

                                                // 调用add_download_sync方法
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
                                            // 发送继续下载成功事件
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

                            // 继续解压流程
                            log_warn!("解压任务 [{}]: 继续解压流程", extract_task_id);
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
                let result = extract_with_7zip(
                    &task.file_path,
                    &task.extract_dir,
                    &task.archive_name,
                    &task.download_task_id,
                );

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
                    final_result = extract_with_7zip(
                        &task.file_path,
                        &task.extract_dir,
                        &task.archive_name,
                        &task.download_task_id,
                    );
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

            // 确保任务完成后，从队列管理器的活跃任务集合中移除（无论解压成功或失败）
            match EXTRACT_MANAGER.queue.lock() {
                Ok(mut queue) => {
                    queue.remove_active_task(&task.id);
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

    // 获取任务ID的函数
    // 检查是否应继续处理的函数
    let should_continue_fn = || !is_app_shutting_down(); // 应用关闭时停止处理

    // 使用QueueManager启动队列处理
    EXTRACT_MANAGER.start_processing(
        process_task_fn,
        1000, // 检查间隔时间（毫秒）
        should_continue_fn,
    );
}

// 嵌入7zG.exe、7z.dll和语言文件作为资源
const SEVENZG_EXE_BYTES: &[u8] = include_bytes!("../bin/7zG.exe");
const SEVENZ_DLL_BYTES: &[u8] = include_bytes!("../bin/7z.dll");
const LANG_ZH_CN_BYTES: &[u8] = include_bytes!("../bin/Lang/zh-cn.txt");

// 全局标志，表示7z资源是否已释放
lazy_static::lazy_static! {
    static ref SEVENZ_RESOURCES_RELEASED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
}

/// 检查7z资源文件是否存在
fn check_7z_resources_exist(bin_dir: &PathBuf) -> bool {
    let sevenzg_exe_path = bin_dir.join("7zG.exe");
    let sevenz_dll_path = bin_dir.join("7z.dll");
    let lang_file_path = bin_dir.join("Lang").join("zh-cn.txt");

    sevenzg_exe_path.exists() && sevenz_dll_path.exists() && lang_file_path.exists()
}

/// 从嵌入式资源中释放7zG.exe、7z.dll和语言文件到二进制目录
///
/// 这是一个内部辅助函数，应用启动时调用一次，也可以在需要时重新调用
pub fn release_7z_resources() -> Result<PathBuf, String> {
    // 获取目录管理器
    let manager = DIR_MANAGER
        .lock()
        .map_err(|e| format!("无法锁定目录管理器: {:?}", e))?;

    // 如果还没有初始化目录管理器，先初始化
    if manager.is_none() {
        return Err("目录管理器未初始化".to_string());
    }

    let bin_dir = manager.as_ref().unwrap().bin_dir();

    // 检查资源是否已存在，如果存在则直接返回路径，避免重复释放
    if check_7z_resources_exist(&bin_dir) {
        log_debug!("7z资源已存在，不需要重新释放");
        return Ok(bin_dir.join("7zG.exe"));
    }

    // 创建Lang子目录
    let lang_dir = bin_dir.join("Lang");
    if let Err(err) = fs::create_dir_all(&lang_dir) {
        log_error!("无法创建Lang目录: {:?}", err);
        return Err(format!("无法创建Lang目录: {:?}", err));
    }

    // 释放7zG.exe
    let sevenzg_exe_path = bin_dir.join("7zG.exe");
    if let Err(err) = fs::write(&sevenzg_exe_path, SEVENZG_EXE_BYTES) {
        log_error!("无法写入7zG.exe到二进制目录: {:?}", err);
        return Err(format!("无法写入7zG.exe到二进制目录: {:?}", err));
    }

    // 释放7z.dll
    let sevenz_dll_path = bin_dir.join("7z.dll");
    if let Err(err) = fs::write(&sevenz_dll_path, SEVENZ_DLL_BYTES) {
        log_error!("无法写入7z.dll到二进制目录: {:?}", err);
        return Err(format!("无法写入7z.dll到二进制目录: {:?}", err));
    }

    // 释放语言文件
    let lang_file_path = lang_dir.join("zh-cn.txt");
    if let Err(err) = fs::write(&lang_file_path, LANG_ZH_CN_BYTES) {
        log_error!("无法写入zh-cn.txt到二进制目录: {:?}", err);
        return Err(format!("无法写入zh-cn.txt到二进制目录: {:?}", err));
    }

    // 确保文件可执行
    #[cfg(windows)]
    unsafe {
        use std::os::windows::ffi::OsStrExt;
        use winapi::um::fileapi::SetFileAttributesW;
        use winapi::um::winnt::FILE_ATTRIBUTE_NORMAL;

        let path_wide: Vec<u16> = sevenzg_exe_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        SetFileAttributesW(path_wide.as_ptr(), FILE_ATTRIBUTE_NORMAL);
    }

    // 设置资源已释放标志
    SEVENZ_RESOURCES_RELEASED.store(true, std::sync::atomic::Ordering::Relaxed);

    log_debug!(
        "7zG.exe、7z.dll和语言文件已成功释放到二进制目录: {}",
        bin_dir.display()
    );
    Ok(sevenzg_exe_path)
}

/// 初始化7z资源 - 在应用启动时调用
pub fn initialize_7z_resources() {
    log_info!("应用启动时初始化7z资源...");

    if let Err(e) = release_7z_resources() {
        log_error!("初始化7z资源失败: {}", e);
    } else {
        log_info!("7z资源初始化成功");
    }
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

    // 预检查7z文件格式
    match std::fs::File::open(&file) {
        Ok(mut file_handle) => {
            let mut header = [0u8; 100];
            match file_handle.read(&mut header) {
                Ok(bytes_read) => {
                    if bytes_read >= 6 {
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

    // 从嵌入式资源获取7zG.exe
    log_debug!("尝试获取7zG.exe、7z.dll和语言文件...");
    let sevenz_exe_path = release_7z_resources()?;
    let sevenz_exe_str = sevenz_exe_path
        .to_str()
        .ok_or("无法将7zG.exe路径转换为字符串".to_string())?;

    /*
    构建7zG.exe命令行参数
        使用x命令解压
        -y表示自动确认所有提示
        -sccUTF-8设置控制台代码页
        -t7z指定文件类型为7z
    */
    let args = [
        "x",         // 解压命令
        "-y",        // 自动确认
        "-sccUTF-8", // 设置控制台代码页为UTF-8
        "-t7z",      // 指定文件类型为7z
        file_path,   // 要解压的文件
    ];

    log_debug!("执行解压命令: {} {}", sevenz_exe_str, args.join(" "));

    // 执行7zG.exe命令
    let mut command = std::process::Command::new(sevenz_exe_str);
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
            Ok(format!(
                "解压成功: {} 文件已解压到 {}，共解压 {} 个文件",
                file_path,
                target_dir.display(),
                file_count
            ))
        } else {
            log_error!("解压失败: 解压目录为空，可能文件格式不支持");
            // 清理空目录
            if let Err(e) = std::fs::remove_dir_all(&target_dir) {
                log_warn!("无法删除空的解压目录: {}", e);
            }
            Err("解压失败: 解压目录为空，可能文件格式不支持".to_string())
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
