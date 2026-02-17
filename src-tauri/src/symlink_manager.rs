use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufWriter, Read, Write},
    net::TcpStream,
    path::Path,
    process::Command,
    sync::Mutex,
    thread,
    time::Duration,
};

use crate::{
    dialog_manager::show_blocking_dialog, init::GLOBAL_APP_HANDLE, log_error, log_info, log_warn,
};
use tauri_plugin_dialog::MessageDialogKind;

/// 响应结构
#[derive(Debug, Serialize, Deserialize)]
pub struct SymlinkResponse {
    pub success: bool,
    pub message: String,
}

/// 全局状态结构
struct GlobalState {
    helper_port: Option<u16>,
    long_connection: Option<TcpStream>,
    helper_token: Option<String>,
}

lazy_static::lazy_static! {
    /// 全局状态
    static ref GLOBAL_STATE: Mutex<GlobalState> = Mutex::new(GlobalState {
        helper_port: None,
        long_connection: None,
        helper_token: None,
    });
}

/// 找到可用的端口号
fn find_available_port() -> Option<u16> {
    // 尝试绑定到 127.0.0.1:0，让系统分配一个可用的端口号
    match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => {
            // 获取绑定的端口号
            listener.local_addr().ok().map(|addr| addr.port())
        }
        Err(_) => None,
    }
}

/// 确保 helper 服务器已启动并建立长连接
///
/// # 返回值
/// - `Ok(())`：服务器成功启动并建立连接
/// - `Err(String)`：服务器启动失败或连接建立失败的错误信息
pub async fn ensure_server_running() -> Result<(), String> {
    // 获取 helper 路径
    let helper_path = std::env::current_exe()
        .map(|exe| {
            exe.parent()
                .unwrap_or_else(|| std::path::Path::new(""))
                .join("helper.exe")
        })
        .unwrap_or_else(|_| std::path::Path::new("helper.exe").to_path_buf());

    log_info!("helper 路径: {:?}", helper_path);

    // 检查是否已有端口并尝试连接，判断服务器是否已在运行
    let port = {
        let mut state = GLOBAL_STATE.lock().unwrap();
        if let Some(existing_port) = state.helper_port {
            // 先检查是否已有长连接存在并验证其可用性
            if let Some(ref long_conn) = state.long_connection {
                // 尝试克隆长连接以验证其可用性
                if let Ok(mut cloned) = long_conn.try_clone() {
                    // 尝试设置超时并写入一个空字节以验证连接
                    if cloned
                        .set_write_timeout(Some(Duration::from_secs(2)))
                        .is_ok()
                        && cloned.write_all(&[]).is_ok()
                    {
                        log_info!(
                            "长连接已存在且可用，helper 服务器已在运行，端口: {}",
                            existing_port
                        );
                        return Ok(());
                    } else {
                        // 长连接不可用，清除它
                        log_info!("长连接不可用，清除连接");
                        state.long_connection = None;
                    }
                } else {
                    // 长连接克隆失败，清除它
                    log_info!("长连接克隆失败，清除连接");
                    state.long_connection = None;
                }
            }

            // 尝试创建新的临时连接以验证服务器状态
            if let Ok(_) = TcpStream::connect(format!("127.0.0.1:{}", existing_port)) {
                log_info!("helper 服务器已在运行，端口: {}", existing_port);
                return Ok(());
            }
            // 连接失败，尝试使用相同的端口重新启动服务器
            log_info!(
                "helper 服务器连接失败，尝试使用相同端口重新启动: {}",
                existing_port
            );
            existing_port
        } else {
            // 没有现有端口，分配新的端口
            let new_port = find_available_port().unwrap_or_else(|| 0);

            log_info!("为 helper 服务器分配端口号: {}", new_port);
            state.helper_port = Some(new_port);
            new_port
        }
    };
    log_info!("启动 helper 服务器，端口: {}", port);

    // 生成UUID作为token
    use uuid::Uuid;
    let token = Uuid::new_v4().to_string();

    // 保存token到全局状态
    {
        let mut state = GLOBAL_STATE.lock().unwrap();
        state.helper_token = Some(token.clone());
    }

    // 启动服务器
    match Command::new(&helper_path)
        .args(["--port", &port.to_string(), "--token", &token])
        .spawn()
    {
        Ok(mut child) => {
            log_info!("helper 服务器已启动，进程ID: {:?}", child.id());

            // 监控 helper 进程的退出状态
            tokio::task::spawn_blocking(move || match child.wait() {
                Ok(exit_status) => {
                    if !exit_status.success() {
                        if let Some(code) = exit_status.code() {
                            if code == 0x5 {
                                // 退出码 0x5 表示访问被拒绝，通常是因为用户取消了 UAC 提升
                                log_error!("用户取消了 UAC 权限提升，无法建立符号链接");

                                // 显示弹窗提示用户
                                if let Ok(guard) = GLOBAL_APP_HANDLE.read() {
                                    if let Some(app_handle) = guard.as_ref() {
                                        show_blocking_dialog(
                                            app_handle,
                                            "您的系统要求以管理员权限建立符号链接\n\n获取管理员权限失败，无法完成挂载操作",
                                            "Nyaser Maps Downloader",
                                            MessageDialogKind::Error,
                                        );
                                    }
                                }
                            } else {
                                log_error!("helper 服务器异常退出，退出码: {:?}", code);
                            }
                        } else {
                            log_error!("helper 服务器异常退出，没有退出码");
                        }
                        let mut state = GLOBAL_STATE.lock().unwrap();
                        state.long_connection = None;
                        state.helper_port = None;
                    }
                }
                Err(e) => {
                    log_error!("等待 helper 服务器退出时出错: {:?}", e);
                }
            });

            // 在后台线程中建立长连接
            tokio::task::spawn_blocking(|| {
                establish_long_connection();
            });

            // 等待连接成功建立
            log_info!("等待与 helper 服务器的连接建立...");

            // 循环检查连接是否已建立
            loop {
                // 检查端口是否被清除（用户拒绝 UAC 授权）
                {
                    let state = GLOBAL_STATE.lock().unwrap();
                    if state.helper_port.is_none() {
                        log_info!("helper 服务器端口已被清除，退出等待");
                        return Err("用户取消了 UAC 权限提升或服务器启动失败".to_string());
                    }
                }

                // 检查是否已建立连接
                if get_long_connection().is_some() {
                    log_info!("连接已成功建立");
                    return Ok(());
                }

                // 等待一段时间后再次检查
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
        Err(e) => {
            log_error!("启动 helper 服务器失败: {:?}", e);
            Err(format!("启动 helper 服务器失败: {:?}", e))
        }
    }
}

/// 建立与 helper 服务器的长连接
fn establish_long_connection() {
    log_info!("建立与 helper 服务器的长连接");

    loop {
        // 获取服务器端口号
        let port = {
            let state = GLOBAL_STATE.lock().unwrap();
            state.helper_port.ok_or(())
        };

        if port.is_err() {
            log_info!("helper 服务器端口号未设置，退出连接尝试");
            return;
        }

        let port = port.unwrap();

        // 尝试连接到服务器
        match TcpStream::connect(format!("127.0.0.1:{}", port)) {
            Ok(mut stream) => {
                log_info!("长连接已建立");

                // 保存长连接
                {
                    let mut state = GLOBAL_STATE.lock().unwrap();
                    if let Ok(cloned) = stream.try_clone() {
                        state.long_connection = Some(cloned);
                    } else {
                        log_warn!("长连接克隆失败，无法保存连接");
                        continue;
                    }
                }

                // 保持连接打开，直到连接断开
                let mut buffer = [0; 1024];
                loop {
                    // 设置非阻塞读取
                    if let Err(e) = stream.set_nonblocking(true) {
                        log_info!("设置非阻塞读取失败: {:?}, 重新连接...", e);
                        {
                            let mut state = GLOBAL_STATE.lock().unwrap();
                            state.long_connection = None;
                        }
                        break;
                    }

                    // 尝试读取数据
                    match stream.read(&mut buffer) {
                        Ok(0) => {
                            // 连接已断开
                            log_info!("长连接已断开，重新连接...");
                            {
                                let mut state = GLOBAL_STATE.lock().unwrap();
                                state.long_connection = None;
                            }
                            break;
                        }
                        Ok(_) => {
                            // 忽略接收到的数据
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            // 没有数据可读，继续
                        }
                        Err(e) => {
                            log_info!("长连接错误: {:?}, 重新连接...", e);
                            {
                                let mut state = GLOBAL_STATE.lock().unwrap();
                                state.long_connection = None;
                            }
                            // 检查是否是远程主机强迫关闭连接的错误（错误码 10054）
                            if e.raw_os_error() == Some(10054) {
                                log_info!("helper 服务器已关闭，退出连接尝试...");
                                return;
                            }
                            break;
                        }
                    }

                    // 恢复为阻塞模式
                    if let Err(e) = stream.set_nonblocking(false) {
                        log_info!("设置阻塞读取失败: {:?}, 重新连接...", e);
                        {
                            let mut state = GLOBAL_STATE.lock().unwrap();
                            state.long_connection = None;
                        }
                        break;
                    }

                    // 保持连接打开
                    thread::sleep(Duration::from_secs(1));
                }
            }
            Err(e) => {
                log_info!("连接到 helper 服务器失败: {:?}, 重试...", e);
                // 检查是否是连接被拒绝的错误
                if e.kind() == std::io::ErrorKind::ConnectionRefused {
                    log_info!("helper 服务器可能正在启动或等待用户授权，继续尝试连接...");
                    // 清除连接状态，以便重新建立
                    {
                        let mut state = GLOBAL_STATE.lock().unwrap();
                        state.long_connection = None;
                    }
                    // 等待一段时间后再次尝试，给用户时间响应 UAC 提示
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
                // 对于其他错误，等待一段时间后再次尝试
                thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

/// 获取与 helper 服务器的长连接
fn get_long_connection() -> Option<std::net::TcpStream> {
    let state = GLOBAL_STATE.lock().unwrap();
    // 尝试克隆长连接
    if let Some(conn) = state.long_connection.as_ref() {
        if let Ok(cloned) = conn.try_clone() {
            return Some(cloned);
        } else {
            // 如果克隆失败，返回 None，但不重置连接
            // 让 establish_long_connection 函数处理连接状态
            log_warn!("长连接克隆失败，返回 None");
        }
    }
    None
}

/// 发送消息到 helper 服务器
async fn send_message_to_server(
    message_map: serde_json::Map<String, serde_json::Value>,
) -> Result<SymlinkResponse, String> {
    // 确保服务器已启动
    ensure_server_running().await?;

    // 添加token到消息中
    let mut message_with_token = message_map;
    let token = {
        let state = GLOBAL_STATE.lock().unwrap();
        state.helper_token.clone().unwrap_or_default()
    };
    message_with_token.insert("token".to_string(), serde_json::Value::String(token));

    // 序列化消息
    let message_json = serde_json::to_string(&message_with_token)
        .map_err(|e| format!("序列化消息失败: {:?}", e))?;

    // 尝试使用长连接发送消息
    if let Some(stream) = get_long_connection() {
        log_info!("使用长连接发送消息到 helper 服务器");

        // 设置超时
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| format!("设置读取超时失败: {:?}", e))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| format!("设置写入超时失败: {:?}", e))?;

        // 发送消息
        let mut writer = BufWriter::new(&stream);

        writeln!(writer, "{}", message_json).map_err(|e| format!("发送消息失败: {:?}", e))?;
        writer
            .flush()
            .map_err(|e| format!("刷新缓冲区失败: {:?}", e))?;

        // 读取响应
        let mut reader = std::io::BufReader::new(&stream);
        let mut response_json = String::new();
        reader
            .read_line(&mut response_json)
            .map_err(|e| format!("读取响应失败: {:?}", e))?;

        // 解析响应
        let response: SymlinkResponse =
            serde_json::from_str(&response_json).map_err(|e| format!("解析响应失败: {:?}", e))?;

        Ok(response)
    } else {
        // 如果仍然没有长连接，返回更具体的错误信息
        Err(
            "无法获取与 helper 服务器的连接，可能是因为用户拒绝了 UAC 授权或服务器启动失败"
                .to_string(),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymlinkInfo {
    pub name: String,
    pub path: String,
    pub target_path: String,
    pub target_exists: bool,
}

pub fn get_all_file_symlinks_in_dir(dir_path: &str) -> Result<Vec<SymlinkInfo>, String> {
    log_info!("开始扫描目录中的文件符号链接: {}", dir_path);

    let dir = Path::new(dir_path);

    if !dir.exists() {
        return Err(format!("目录不存在: {}", dir_path));
    }

    if !dir.is_dir() {
        return Err(format!("路径不是目录: {}", dir_path));
    }

    let mut symlinks = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            log_error!("无法读取目录: {}, 错误: {:?}", dir_path, e);
            return Err(format!("无法读取目录: {:?}", e));
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log_warn!("读取目录项失败: {:?}", e);
                continue;
            }
        };

        let path = entry.path();

        if !path.is_symlink() {
            continue;
        }

        let metadata = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) => {
                log_warn!("无法获取符号链接元数据: {:?}, 错误: {:?}", path, e);
                continue;
            }
        };

        if metadata.file_type().is_file() {
            let name = match path.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => {
                    log_warn!("无法获取符号链接文件名: {:?}", path);
                    continue;
                }
            };

            let path_str = path.to_string_lossy().to_string();

            let target_path = match fs::read_link(&path) {
                Ok(target) => {
                    let canonical_target = match fs::canonicalize(&target) {
                        Ok(p) => p.to_string_lossy().to_string(),
                        Err(_) => target.to_string_lossy().to_string(),
                    };
                    canonical_target
                }
                Err(e) => {
                    log_warn!("无法读取符号链接目标: {:?}, 错误: {:?}", path, e);
                    continue;
                }
            };

            let target_exists = Path::new(&target_path).exists();

            log_info!("找到文件符号链接: {}", path_str);

            symlinks.push(SymlinkInfo {
                name,
                path: path_str.clone(),
                target_path,
                target_exists,
            });
        }
    }

    log_info!("扫描完成，共找到 {} 个文件符号链接", symlinks.len());
    Ok(symlinks)
}

pub async fn create_file_symlink(
    target_path: &str,
    link_dir: &str,
    link_name: &str,
) -> Result<String, String> {
    // 使用服务器模式创建符号链接
    let mut args_map = serde_json::Map::new();
    args_map.insert(
        "target".to_string(),
        serde_json::Value::String(target_path.to_string()),
    );
    args_map.insert(
        "path".to_string(),
        serde_json::Value::String(link_dir.to_string()),
    );
    args_map.insert(
        "name".to_string(),
        serde_json::Value::String(link_name.to_string()),
    );

    let mut message_map = serde_json::Map::new();
    message_map.insert(
        "cmd".to_string(),
        serde_json::Value::String("create".to_string()),
    );
    message_map.insert("args".to_string(), serde_json::Value::Object(args_map));

    match send_message_to_server(message_map).await {
        Ok(response) => {
            if response.success {
                log_info!("符号链接创建成功: {}", response.message);
                Ok(response.message)
            } else {
                log_error!("符号链接创建失败: {}", response.message);
                Err(response.message)
            }
        }
        Err(e) => {
            log_error!("与服务器通信失败: {:?}", e);
            Err(e)
        }
    }
}

pub fn delete_file_symlink(link_path: &str) -> Result<String, String> {
    log_info!("开始删除文件符号链接: {}", link_path);

    let path = Path::new(link_path);

    if !path.exists() {
        return Err(format!("符号链接不存在: {}", link_path));
    }

    if !path.is_symlink() {
        return Err(format!("路径不是符号链接: {}", link_path));
    }

    fs::remove_file(path).map_err(|e| {
        log_error!("删除符号链接失败: {:?}, 错误: {:?}", path, e);
        format!("删除符号链接失败: {:?}", e)
    })?;

    log_info!("文件符号链接删除成功: {}", link_path);
    Ok(format!("符号链接删除成功: {}", link_path))
}
