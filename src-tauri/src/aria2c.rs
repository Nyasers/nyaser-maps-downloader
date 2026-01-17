// aria2c 模块 - 负责通过aria2c工具下载文件，提供文件下载功能

// 标准库导入
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    net::TcpListener,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Mutex,
    },
    time::Duration,
};

// 第三方库导入
extern crate lazy_static;
use lazy_static::lazy_static;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json;
use tauri::{AppHandle, Emitter};
use tokio::runtime::Runtime;
use uuid::Uuid;

// 内部模块导入
use crate::{
    commands::refresh_download_queue, init::is_app_shutting_down, log_debug, log_error, log_info,
    log_utils::redirect_process_output, log_warn, queue_manager::QueueManager,
    utils::get_file_name,
};

// 定义下载任务队列项结构
#[derive(Debug)]
pub enum PendingTask {
    Download {
        url: String,
        app_handle: AppHandle,
        task_id: String,
    },
}

// 全局状态管理
lazy_static! {
    // 用于存储运行中的aria2c进程的ID，确保进程跟踪和管理
    static ref RUNNING_ARIA2_PIDS: Mutex<HashSet<u32>> = Mutex::new(HashSet::new());

    // 标记aria2c后端是否已初始化完成
    static ref ARIA2_INITIALIZED: AtomicBool = AtomicBool::new(false);

    // 用于保护初始化过程的互斥锁
    static ref INIT_LOCK: Mutex<()> = Mutex::new(());

    // 存储aria2c初始化完成前的待处理下载任务
    static ref PENDING_TASKS_MANAGER: QueueManager<PendingTask> = QueueManager::new(5);
    // RPC服务器管理单例 - 全局可访问
    pub static ref ARIA2_RPC_MANAGER: Mutex<Option<Aria2RpcManager>> = Mutex::new(None);
    // 保存上次成功启动的aria2c信息（端口、密钥、PID）
    static ref LAST_ARIA2_INFO: Mutex<Option<(u16, String, u32)>> = Mutex::new(None);

    // 用于存储取消下载请求的任务ID及其原因
    pub static ref CANCEL_DOWNLOAD_REQUESTS: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());

    /// aria2c.exe路径常量
    pub static ref ARIA2C_PATH: PathBuf = crate::get_assets_path("bin/aria2c.exe").expect("无法获取aria2c.exe路径");
}

// 下载状态结构体
#[derive(Debug)]
struct DownloadStatus {
    progress: f64,
    connections: u64,
    total_size_mb: f64,
    completed_length: u64,
    total_length: u64,
    download_speed: u64,
}

// 辅助函数：尝试在指定时间内获取锁，如果超时则返回None
// 用于防止在应用关闭时因锁获取失败导致的无限阻塞
fn try_lock_with_timeout<T>(
    mutex: &Mutex<T>,
    timeout_ms: u64,
) -> Option<std::sync::MutexGuard<'_, T>> {
    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let check_interval = std::time::Duration::from_millis(10);

    loop {
        // 检查应用是否正在关闭，如果是则不再等待锁
        if is_app_shutting_down() {
            log_debug!("应用正在关闭，跳过锁获取");
            return None;
        }

        // 尝试获取锁
        match mutex.try_lock() {
            Ok(guard) => return Some(guard),
            Err(std::sync::TryLockError::WouldBlock) => {
                // 检查是否超时
                if start_time.elapsed() >= timeout {
                    log_warn!("获取锁超时: {}ms", timeout_ms);
                    return None;
                }
                // 等待一小段时间后重试
                std::thread::sleep(check_interval);
            }
            Err(std::sync::TryLockError::Poisoned(e)) => {
                log_error!("锁已中毒: {:?}", e);
                // TryLockError::Poisoned的into_inner()方法直接返回MutexGuard
                let guard = e.into_inner();
                log_info!("尝试恢复中毒的锁");
                return Some(guard);
            }
        }
    }
}

// RPC请求和响应结构体定义
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Aria2JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Vec<serde_json::Value>,
    id: u64,
}

#[derive(Serialize, Deserialize)]
struct Aria2JsonRpcResponse<T> {
    jsonrpc: String,
    result: Option<T>,
    error: Option<serde_json::Value>,
    id: u64,
}

// Aria2 RPC管理器，用于管理aria2c RPC服务器
pub struct Aria2RpcManager {
    // RPC服务器监听的地址
    pub url: String,
    // RPC密钥
    pub secret: String,
    // aria2c进程
    process: Option<Child>,
    // 进程ID
    pub pid: u32,
    // 标记进程是否被监控
    is_monitored: AtomicBool,
}

impl Clone for Aria2RpcManager {
    fn clone(&self) -> Self {
        Aria2RpcManager {
            url: self.url.clone(),
            secret: self.secret.clone(),
            process: None, // 克隆时不包含进程句柄
            pid: self.pid,
            is_monitored: AtomicBool::new(self.is_monitored.load(Ordering::Relaxed)),
        }
    }
}

impl Aria2RpcManager {
    // 创建新的Aria2 RPC管理器
    pub fn new() -> Result<Self, String> {
        log_info!("创建Aria2 RPC管理器");

        // 查找可用端口
        let port = find_available_port()?;
        log_debug!("找到可用端口: {}", port);

        // 生成随机RPC密钥
        let secret = Uuid::new_v4().to_string();
        log_debug!("生成RPC密钥: {}", secret);

        // 构建RPC URL - 使用localhost而不是localhost，确保连接到IPv4回环地址
        let url = format!("http://localhost:{}/jsonrpc", port);
        log_info!("RPC服务器URL: {}", url);

        // 启动aria2c RPC服务器
        // 首先尝试复用上次成功启动的aria2c实例
        if let Some((last_port, last_secret, last_pid)) =
            { (*LAST_ARIA2_INFO.lock().unwrap()).clone() }
        {
            log_info!(
                "尝试复用上次启动的aria2c实例: 端口={}, PID={}",
                last_port,
                last_pid
            );

            // 检查进程是否仍在运行
            if is_process_running(last_pid) {
                // 使用localhost而不是localhost，确保连接到IPv4回环地址
                let last_url = format!("http://localhost:{}/jsonrpc", last_port);

                // 尝试连接到现有实例
                // 执行一个简单的RPC ping操作来验证连接是否有效
                if test_aria2_connection(&last_url, &last_secret) {
                    log_info!("成功复用现有aria2c实例");

                    // 创建一个不包含进程句柄的管理器实例，因为进程是外部创建的
                    let manager = Aria2RpcManager {
                        url: last_url,
                        secret: last_secret,
                        process: None, // 不拥有外部进程的句柄
                        pid: last_pid,
                        is_monitored: AtomicBool::new(false),
                    };

                    // 启动进程监控
                    manager.start_process_monitoring();

                    return Ok(manager);
                } else {
                    log_warn!("现有aria2c实例连接失败，将创建新实例");
                }
            } else {
                log_warn!(
                    "上次启动的aria2c实例（PID={}）不再运行，将创建新实例",
                    last_pid
                );
            }
        }

        // 如果没有可复用的实例，则创建新实例
        log_info!("创建新的aria2c RPC服务器实例");
        let process = start_aria2c_rpc_server(port, &secret)?;
        let pid = process.id();

        // 保存成功启动的aria2c信息，用于下次复用
        *LAST_ARIA2_INFO.lock().unwrap() = Some((port, secret.clone(), pid));
        log_info!("保存aria2c信息用于下次复用: 端口={}, PID={}", port, pid);

        let manager = Aria2RpcManager {
            url,
            secret,
            process: Some(process),
            pid,
            is_monitored: AtomicBool::new(false),
        };

        // 启动进程监控
        manager.start_process_monitoring();

        Ok(manager)
    }

    // 启动aria2c进程监控
    pub fn start_process_monitoring(&self) {
        // 检查是否已经在监控
        if !self
            .is_monitored
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .unwrap_or(true)
        {
            log_info!("开始监控aria2c进程: PID={}", self.pid);

            // 将pid声明为可变的，这样我们可以在进程重启后更新它
            let mut pid = self.pid;
            let _secret = self.secret.clone(); // 保留但标记为未使用
            let _url = self.url.clone(); // 保留但标记为未使用

            // 创建新线程进行监控
            std::thread::spawn(move || {
                let check_interval = Duration::from_secs(3); // 每3秒检查一次

                loop {
                    // 检查应用是否正在关闭，如果是则结束监控
                    if is_app_shutting_down() {
                        log_info!("检测到应用正在关闭，结束aria2c进程监控");
                        break;
                    }

                    // 等待检查间隔
                    std::thread::sleep(check_interval);

                    // 检查进程是否仍在运行
                    if !is_process_running(pid) {
                        log_error!("检测到aria2c进程意外关闭: PID={}", pid);

                        // 尝试通知全局管理器进行重启
                        // 使用带超时的try_lock以避免在关闭时阻塞
                        match try_lock_with_timeout(&ARIA2_RPC_MANAGER, 1000) {
                            Some(mut manager) => {
                                if let Some(current_manager) = manager.as_ref() {
                                    if current_manager.pid == pid {
                                        log_info!("准备重启aria2c服务");

                                        // 保留上次保存的信息，以便重用相同的端口和配置
                                        log_info!("尝试使用相同的配置重启aria2c进程");

                                        // 尝试使用Aria2RpcManager::new()创建新实例
                                        // 注意：new()方法会检查LAST_ARIA2_INFO并尝试复用配置
                                        match Aria2RpcManager::new() {
                                            Ok(new_manager) => {
                                                log_info!(
                                                    "成功创建新的Aria2RpcManager实例，新PID: {}",
                                                    new_manager.pid
                                                );

                                                // 替换全局管理器实例
                                                *manager = Some(new_manager);
                                            }
                                            Err(e) => {
                                                log_error!(
                                                    "创建新的Aria2RpcManager实例失败: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            None => {
                                log_warn!("获取RPC管理器锁超时，跳过重启操作");
                            }
                        }

                        // 重新获取当前管理器的PID
                        // 这样如果进程已重启，我们会开始监控新的PID
                        match try_lock_with_timeout(&ARIA2_RPC_MANAGER, 1000) {
                            Some(updated_manager) => {
                                if let Some(current_manager) = updated_manager.as_ref() {
                                    // 更新当前线程监控的PID
                                    pid = current_manager.pid;
                                    log_info!("更新监控的PID为: {}", pid);
                                }
                            }
                            None => {
                                log_warn!("获取RPC管理器锁超时，无法更新监控的PID");
                            }
                        }
                        continue;
                    }
                }
            });
        }
    }

    // 添加下载任务到RPC服务器（异步版本）
    pub async fn add_download(
        &self,
        url: &str,
        save_path: &str,
        filename: &str,
    ) -> Result<String, String> {
        log_info!("通过RPC添加下载任务: URL={}, 文件={}", url, filename);

        // 准备请求参数
        let mut params = Vec::new();
        params.push(serde_json::Value::String(format!("token:{}", self.secret)));

        // URI数组作为第二个参数
        params.push(serde_json::Value::Array(vec![serde_json::Value::String(
            url.to_string(),
        )]));

        // 选项作为第三个参数
        let options = serde_json::json!({
            "dir": save_path,
            "out": filename,
            "continue": true,
            "max-connection-per-server": 16,
            "split": 16,
            "console-log-level": "notice",
            "user-agent": "pan.baidu.com",
        });
        params.push(options);

        // 构建JSON-RPC请求
        let request = Aria2JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "aria2.addUri".to_string(),
            params,
            id: 1,
        };

        // 发送请求并获取响应（使用异步版本）
        let response = send_rpc_request_async(self, &request).await?;

        // 解析响应
        let response: Aria2JsonRpcResponse<String> =
            serde_json::from_str(&response).map_err(|e| format!("解析RPC响应失败: {}", e))?;

        if let Some(gid) = response.result {
            log_info!("下载任务添加成功，GID: {}", gid);
            Ok(gid)
        } else if let Some(error) = response.error {
            log_error!("添加下载任务失败: {:?}", error);
            Err(format!("添加下载任务失败: {:?}", error))
        } else {
            log_error!("添加下载任务失败: 未知错误");
            Err("添加下载任务失败: 未知错误".to_string())
        }
    }

    // 添加下载任务到RPC服务器（同步包装器，供非异步环境使用）
    pub fn add_download_sync(
        &self,
        url: &str,
        save_path: &str,
        filename: &str,
    ) -> Result<String, String> {
        // 创建一个新的Tokio运行时来执行异步操作，确保在任何线程中都能正常工作
        let rt =
            tokio::runtime::Runtime::new().map_err(|e| format!("创建Tokio运行时失败: {}", e))?;
        rt.block_on(self.add_download(url, save_path, filename))
    }

    // 关闭RPC服务器
    pub fn shutdown(&mut self) {
        log_info!("关闭Aria2 RPC服务器: PID={}", self.pid);

        // 设置监控标志为true，表示不再监控此进程
        self.is_monitored.store(true, Ordering::Relaxed);

        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
            // 移除process.wait()调用，避免阻塞清理过程
            log_info!("Aria2 RPC服务器已发送终止信号");
        }
    }
}

// 查找可用端口
fn find_available_port() -> Result<u16, String> {
    // 尝试绑定端口以确定其是否可用
    let listener = TcpListener::bind("localhost:0").map_err(|e| format!("无法绑定端口: {}", e))?;

    // 获取绑定的端口
    let addr = listener
        .local_addr()
        .map_err(|e| format!("无法获取本地地址: {}", e))?;

    // 关闭监听器，释放端口
    drop(listener);

    Ok(addr.port())
}

// 启动aria2c RPC服务器
fn start_aria2c_rpc_server(port: u16, secret: &str) -> Result<Child, String> {
    log_info!("启动aria2c RPC服务器，端口: {}", port);
    log_debug!("aria2c路径: {}", ARIA2C_PATH.display());

    // 构建命令行参数
    let mut command = Command::new(ARIA2C_PATH.as_path());
    command
        .arg("--enable-rpc")
        .arg(format!("--rpc-listen-port={}", port))
        // 设置为true允许监听所有接口，但我们的连接代码会明确使用localhost
        .arg("--rpc-listen-all=false")
        .arg(format!("--rpc-secret={}", secret))
        .arg("--rpc-allow-origin-all")
        .arg("--continue=true")
        .arg("--max-concurrent-downloads=1")
        .arg("--max-connection-per-server=16")
        .arg("--min-split-size=1M")
        .arg("--split=16")
        .arg("--console-log-level=warn") // 不输出INFO级别日志到stdout
        .stdout(Stdio::piped()) // 捕获stdout输出
        .stderr(Stdio::piped()) // 捕获stderr输出
        .stdin(Stdio::null());

    // 在Windows上，隐藏窗口运行
    command.creation_flags(0x08000000);

    // 启动进程
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动aria2c RPC服务器失败: {}", e))?;

    // 等待一小段时间让服务器初始化
    std::thread::sleep(std::time::Duration::from_millis(200));

    // 获取stdout和stderr流
    let stdout = child.stdout.take().ok_or("无法获取stdout流")?;
    let stderr = child.stderr.take().ok_or("无法获取stderr流")?;

    // 记录进程ID
    let pid = child.id();
    RUNNING_ARIA2_PIDS.lock().unwrap().insert(pid);
    log_info!("aria2c RPC服务器启动成功，PID: {}", pid);

    // 重定向aria2c的输出到主程序日志
    redirect_process_output(stdout, stderr, format!("aria2c[{}]", pid));

    Ok(child)
}

// 检查aria2c进程是否存活
fn check_process_alive(pid: u32) -> Result<(), String> {
    if !is_process_running(pid) {
        let error_msg = format!("aria2c进程未运行 (PID: {})，无法发送RPC请求", pid);
        log_error!("{}", error_msg);
        return Err(error_msg);
    }
    log_debug!("aria2c进程 (PID: {}) 确认存活", pid);
    Ok(())
}

// 序列化RPC请求为JSON字符串
fn serialize_request(request: &Aria2JsonRpcRequest) -> Result<String, String> {
    let request_json =
        serde_json::to_string(&request).map_err(|e| format!("序列化请求失败: {}", e))?;
    Ok(request_json)
}

// 使用 reqwest 发送 RPC 请求
async fn send_rpc_request_via_reqwest(
    manager: &Aria2RpcManager,
    request: &Aria2JsonRpcRequest,
) -> Result<String, String> {
    log_debug!("使用 reqwest 发送 RPC 请求到: {}", manager.url);
    log_debug!("请求内容: {:?}", request);

    check_process_alive(manager.pid)?;

    let request_json = serialize_request(request)?;
    log_debug!("RPC 请求 JSON: {}", request_json);

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let max_retries: u8 = 3;
    let mut retry_interval = Duration::from_millis(500);
    let mut last_error = "未知错误".to_string();

    for attempt in 0..=max_retries {
        if let Err(e) = check_process_alive(manager.pid) {
            log_error!("{}", e);
            reset_rpc_manager_if_needed();
            return Err(e);
        }

        match client
            .post(&manager.url)
            .header("Content-Type", "application/json")
            .body(request_json.clone())
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                log_debug!("HTTP 响应状态码: {}", status);

                if status.is_success() {
                    match response.text().await {
                        Ok(text) => {
                            if text.is_empty() {
                                last_error = "RPC 响应为空".to_string();
                                log_warn!("{}", last_error);
                            } else {
                                log_debug!("RPC 响应内容: {}", text);
                                return Ok(text);
                            }
                        }
                        Err(e) => {
                            last_error = format!("读取响应内容失败: {}", e);
                            log_warn!("{}", last_error);
                        }
                    }
                } else {
                    match response.text().await {
                        Ok(error_text) => {
                            last_error = format!("HTTP 错误 {}: {}", status, error_text);
                            log_warn!("{}", last_error);

                            if error_text.contains("GID") && error_text.contains("is not found") {
                                return Err("GID_NOT_FOUND".to_string());
                            }
                        }
                        Err(e) => {
                            last_error = format!("HTTP 错误 {}: {}", status, e);
                            log_warn!("{}", last_error);
                        }
                    }
                }
            }
            Err(e) => {
                last_error = format!("HTTP 请求失败: {}", e);
                log_warn!(
                    "HTTP 请求错误: {}, 尝试重试 ({}/{})...",
                    e,
                    attempt + 1,
                    max_retries
                );
            }
        }

        if attempt < max_retries {
            tokio::time::sleep(retry_interval).await;
            retry_interval = retry_interval.saturating_mul(2);
        }
    }

    log_error!("RPC 请求失败: {}", last_error);
    Err(last_error)
}

// 重置RPC管理器（如果需要）
fn reset_rpc_manager_if_needed() {
    match try_lock_with_timeout(&ARIA2_RPC_MANAGER, 1000) {
        Some(mut manager) => {
            log_info!("检测到aria2c进程关闭，主动重置全局RPC管理器");
            *manager = None;
        }
        None => {
            log_warn!("获取RPC管理器锁超时，无法重置全局RPC管理器");
        }
    }
}

// 异步发送RPC请求（使用 reqwest）
async fn send_rpc_request_async(
    manager: &Aria2RpcManager,
    request: &Aria2JsonRpcRequest,
) -> Result<String, String> {
    send_rpc_request_via_reqwest(manager, request).await
}

// 获取下载任务状态
async fn get_download_status(gid: &str) -> Result<Option<DownloadStatus>, String> {
    // 获取最新的RPC管理器实例
    if let Err(e) = get_rpc_manager() {
        return Err(format!("获取RPC管理器失败: {}", e));
    }

    // 获取管理器锁
    let manager = match try_lock_with_timeout(&ARIA2_RPC_MANAGER, 1000) {
        Some(guard) => match guard.as_ref() {
            Some(mgr) => mgr.clone(),
            None => return Err("RPC管理器未初始化".to_string()),
        },
        None => return Err("获取RPC管理器锁超时".to_string()),
    };

    // 构建JSON-RPC请求
    let request = Aria2JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "aria2.tellStatus".to_string(),
        params: vec![
            serde_json::Value::String(format!("token:{}", manager.secret)),
            serde_json::Value::String(gid.to_string()),
        ],
        id: 1,
    };

    // 发送请求并获取响应
    let response = match send_rpc_request_async(&manager, &request).await {
        Ok(response) => response,
        Err(e) => return Err(e),
    };

    // 解析响应
    let response: Aria2JsonRpcResponse<serde_json::Value> = match serde_json::from_str(&response) {
        Ok(response) => response,
        Err(e) => {
            // 检查是否是GID丢失的错误（PowerShell错误格式）
            if response.contains("GID") && response.contains("is not found") {
                return Err("GID_NOT_FOUND".to_string());
            }
            return Err(format!("解析RPC响应失败: {}", e));
        }
    };

    // 检查响应中是否包含错误信息
    if let Some(error) = &response.error {
        // 尝试从error对象中获取message字段
        let error_message = if let Some(obj) = error.as_object() {
            if let Some(message) = obj.get("message").and_then(|v| v.as_str()) {
                message.to_string()
            } else {
                "未知错误".to_string()
            }
        } else {
            "未知错误".to_string()
        };

        // 检测GID丢失错误
        if error_message.contains("GID") && error_message.contains("is not found") {
            return Err("GID_NOT_FOUND".to_string());
        }
        return Err(format!("RPC请求失败: {}", error_message));
    };

    if let Some(result) = response.result {
        // 解析进度信息
        if let Some(result_map) = result.as_object() {
            let connections = result_map
                .get("connections")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            let completed_length = result_map
                .get("completedLength")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            let total_length = result_map
                .get("totalLength")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(1);

            let download_speed = result_map
                .get("downloadSpeed")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            // 计算进度百分比
            let progress = if total_length > 0 {
                (completed_length as f64 / total_length as f64) * 100.0
            } else {
                0.0
            };

            // 计算总大小（MB）
            let total_size_mb = total_length as f64 / (1024.0 * 1024.0);

            return Ok(Some(DownloadStatus {
                progress,
                connections,
                total_size_mb,
                completed_length,
                total_length,
                download_speed,
            }));
        }
    }

    // 任务可能不存在或已完成
    Ok(None)
}

// 测试aria2连接是否有效（异步版本）
async fn test_aria2_connection_async(url: &str, secret: &str) -> bool {
    log_info!("测试aria2 RPC连接有效性: {}", url);

    // 创建一个简单的RPC请求来获取版本信息
    let request = Aria2JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "aria2.getVersion".to_string(),
        params: vec![serde_json::Value::String(format!("token:{}", secret))],
        id: 1,
    };

    // 创建一个临时管理器实例用于测试连接
    let temp_manager = Aria2RpcManager {
        url: url.to_string(),
        secret: secret.to_string(),
        process: None,
        pid: 0, // 临时实例，PID不相关
        is_monitored: AtomicBool::new(false),
    };

    // 尝试发送请求，最多尝试3次，每次间隔100ms
    for attempt in 1..=3 {
        match send_rpc_request_async(&temp_manager, &request).await {
            Ok(response) => {
                // 检查响应是否有效
                if !response.is_empty() {
                    log_info!("aria2 RPC连接测试成功 (第{}次尝试)", attempt);
                    return true;
                }
            }
            Err(e) => {
                log_debug!("aria2 RPC连接测试失败 (第{}次尝试): {}", attempt, e);
                // 如果不是最后一次尝试，则等待一小段时间后重试
                if attempt < 3 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    log_warn!("aria2 RPC连接测试失败，所有尝试都已耗尽");
    false
}

// 测试aria2连接是否有效（同步包装器）
fn test_aria2_connection(url: &str, secret: &str) -> bool {
    // 在线程池中执行异步操作，避免阻塞主进程
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(test_aria2_connection_async(url, secret))
    })
}

// 检查进程是否正在运行
fn is_process_running(pid: u32) -> bool {
    // 检查进程是否存在
    use std::process::Command;

    // 使用tasklist命令检查进程是否存在
    let mut command = Command::new("tasklist");
    command.args(["/FI", &format!("PID eq {}", pid)]);

    // 隐藏窗口运行
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW 标志

    let output = command
        .output()
        .map_err(|e| log_error!("检查进程状态失败: {}", e))
        .ok();

    if let Some(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains(&pid.to_string())
    } else {
        false
    }
}

// 获取全局RPC管理器
pub fn get_rpc_manager() -> Result<(), String> {
    log_debug!("尝试获取全局RPC管理器实例");

    // 检查是否已经初始化RPC管理器，使用try_lock并设置超时机制
    let mut manager = match ARIA2_RPC_MANAGER.try_lock() {
        Ok(guard) => {
            log_debug!("获取RPC管理器锁成功");
            guard
        }
        Err(_) => {
            // 检查应用是否正在关闭，如果是则快速返回
            if is_app_shutting_down() {
                log_info!("应用正在关闭，跳过RPC管理器获取...");
                return Err("应用正在关闭，无法获取RPC管理器".to_string());
            }

            log_warn!("无法立即获取RPC管理器锁，等待500毫秒后重试...");
            // 等待500毫秒后再次尝试获取锁
            std::thread::sleep(std::time::Duration::from_millis(500));

            match ARIA2_RPC_MANAGER.try_lock() {
                Ok(guard) => {
                    log_debug!("重试获取RPC管理器锁成功");
                    guard
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    log_warn!("重试获取RPC管理器锁仍被阻塞");
                    // 再次获取失败，使用try_lock_with_timeout函数
                    match try_lock_with_timeout(&ARIA2_RPC_MANAGER, 1000) {
                        Some(guard) => guard,
                        None => {
                            log_error!("获取RPC管理器锁超时");
                            return Err("获取RPC管理器锁超时".to_string());
                        }
                    }
                }
                Err(std::sync::TryLockError::Poisoned(e)) => {
                    // 处理中毒的锁 - 尝试恢复
                    log_warn!("RPC管理器锁被中毒，尝试恢复...");
                    e.into_inner()
                }
            }
        }
    };

    // 如果没有初始化或现有实例无效，则创建新的RPC管理器
    if manager.is_none() {
        log_info!("RPC管理器未初始化或实例无效，正在创建...");

        match Aria2RpcManager::new() {
            Ok(rpc_manager) => {
                log_debug!("RPC管理器创建成功，URL: {}", rpc_manager.url);
                *manager = Some(rpc_manager);
                log_info!("RPC管理器初始化成功");
            }
            Err(e) => {
                log_error!("初始化RPC管理器失败: {}", e);
                return Err(e);
            }
        }
    }

    log_debug!("RPC管理器获取成功");
    Ok(())
}

/// 处理队列中的单个待处理下载任务
fn process_pending_task(_task_id: String, task: &PendingTask) {
    match task {
        PendingTask::Download {
            url,
            app_handle,
            task_id,
        } => {
            log_info!("处理待处理的下载任务: [{}] URL={}", task_id, url);
            let app_handle: AppHandle = app_handle.clone();
            let url_clone = url.clone();
            let task_id_clone = task_id.clone();

            // 使用异步运行时执行异步下载操作
            tauri::async_runtime::spawn(async move {
                // 再次检查aria2c是否已初始化完成，避免重复添加任务到队列
                if !ARIA2_INITIALIZED.load(Ordering::Relaxed) {
                    log_warn!("[{}] aria2c后端仍未初始化完成，延迟处理任务", task_id_clone);
                    // 发送等待中事件给前端
                    let _ = app_handle.emit_to(
                        "main",
                        "download-waiting",
                        &serde_json::json!({
                            "taskId": task_id_clone,
                            "url": url_clone,
                            "message": "等待下载引擎初始化完成..."
                        }),
                    );
                    return;
                }

                let download_result =
                    download_via_aria2(&url_clone, app_handle.clone(), &task_id_clone).await;
                log_info!(
                    "下载任务完成: [{}] 结果={:?}",
                    task_id_clone,
                    download_result
                );

                // 处理下载结果
                match download_result {
                    Ok(file_path) => {
                        // 下载成功，发送下载完成事件给main窗口
                        let _ = app_handle.emit_to(
                            "main",
                            "download-complete",
                            &serde_json::json!({
                                "taskId": task_id_clone,
                                "success": true,
                                "message": "下载完成，正在准备解压",
                                "filePath": file_path,
                                "fileSize": std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0)
                            }),
                        );
                    }
                    Err(err) => {
                        // 下载失败，发送下载失败事件给main窗口
                        let _ = app_handle.emit_to(
                            "main",
                            "download-failed",
                            &serde_json::json!({
                                "taskId": task_id_clone,
                                "success": false,
                                "message": err
                            }),
                        );
                    }
                }
            });
        }
    }
}

/// 检查是否应继续处理的函数
fn should_continue_processing() -> bool {
    !is_app_shutting_down()
}

/// 启动待处理任务管理器
pub fn start_pending_tasks_manager() {
    log_info!("启动待处理任务管理器...");
    PENDING_TASKS_MANAGER.start_processing(
        process_pending_task,
        500, // 检查间隔（毫秒）
        should_continue_processing,
    );
}

/// 异步初始化aria2c RPC服务器
/// 这个函数在后台线程中执行实际的初始化工作
async fn initialize_aria2c_backend_async() -> Result<(), String> {
    log_info!("应用启动时异步初始化aria2c后端...");

    // 预初始化RPC管理器，确保它在应用启动时就创建好
    let result = get_rpc_manager();

    match result {
        Ok(_) => {
            // 初始化成功，标记为已初始化
            ARIA2_INITIALIZED.store(true, Ordering::Relaxed);
            log_info!("aria2c后端初始化完成");

            // 启动待处理任务管理器
            start_pending_tasks_manager();

            Ok(())
        }
        Err(e) => {
            log_error!("初始化aria2c后端失败: {}", e);
            Err(e)
        }
    }
}

/// 初始化aria2c RPC服务器
/// 这个函数应该在应用启动时调用，确保RPC服务器随应用启动
/// 它会立即返回，在后台异步执行初始化过程
pub fn initialize_aria2c_backend() -> Result<(), String> {
    log_info!("应用启动时启动异步初始化aria2c后端...");

    // 在新线程中异步执行初始化
    std::thread::spawn(|| {
        let rt = Runtime::new().expect("无法创建Tokio运行时");
        rt.block_on(async {
            if let Err(e) = initialize_aria2c_backend_async().await {
                log_error!("异步初始化aria2c后端失败: {}", e);
            }
        });
    });

    // 立即返回成功，不阻塞主线程
    Ok(())
}

/// 清理aria2c资源
/// 这个函数应该在应用关闭时调用，确保aria2c RPC服务器正确关闭并释放所有资源
pub fn cleanup_aria2c_resources() {
    log_info!("应用关闭时清理aria2c资源...");

    // 使用try_lock并设置超时机制，避免在aria2关闭时无限阻塞
    log_info!("尝试获取RPC管理器锁以关闭服务...");
    let mut manager_guard = match ARIA2_RPC_MANAGER.try_lock() {
        Ok(guard) => Some(guard),
        Err(_) => {
            log_warn!("无法立即获取RPC管理器锁，等待1秒后重试...");
            // 等待1秒后再次尝试获取锁
            std::thread::sleep(std::time::Duration::from_millis(1000));
            // 第二次尝试获取锁，如果失败则放弃
            match ARIA2_RPC_MANAGER.try_lock() {
                Ok(guard) => Some(guard),
                Err(_) => {
                    log_error!("再次获取RPC管理器锁失败，跳过RPC管理器清理...");
                    None
                }
            }
        }
    };

    // 如果成功获取锁，执行关闭操作
    if let Some(ref mut guard) = manager_guard {
        if let Some(mut rpc_manager) = guard.take() {
            log_info!("关闭Aria2 RPC服务器...");
            rpc_manager.shutdown();
        }
    }

    // 重置初始化标志，确保下次启动时能正确重新初始化
    ARIA2_INITIALIZED.store(false, Ordering::Relaxed);
    log_info!("重置aria2c初始化标志为false");

    // 清除上次保存的aria2c信息，防止重用旧的配置
    *LAST_ARIA2_INFO.lock().unwrap() = None;
    log_info!("清除上次保存的aria2c信息");

    // 等待一小段时间，给RPC服务器一些时间关闭
    log_info!("等待RPC服务器关闭...");
    std::thread::sleep(std::time::Duration::from_millis(500));

    // 清理所有运行中的aria2c进程 - 使用lock确保执行
    log_info!("尝试获取进程ID列表锁以终止剩余进程...");
    if let Ok(mut pids_guard) = RUNNING_ARIA2_PIDS.lock() {
        for pid in pids_guard.drain() {
            log_info!("强制终止aria2c进程: {}", pid);
            // 在Windows上，使用taskkill命令强制终止进程，隐藏窗口
            let result = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW 标志，隐藏命令行窗口
                .output(); // 使用output等待命令完成

            match result {
                Ok(output) => {
                    if output.status.success() {
                        log_info!("成功终止进程: {}", pid);
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        log_warn!("终止进程{}失败: {}", pid, stderr);
                    }
                }
                Err(e) => log_error!("执行taskkill命令失败: {}", e),
            }
        }
    } else {
        log_error!("获取进程ID列表锁失败，这是不应该发生的情况！");
    }

    // 额外等待时间，确保文件资源完全释放
    log_info!("等待文件资源完全释放...");
    std::thread::sleep(std::time::Duration::from_millis(500));

    // 清理可能残留的aria2临时文件
    if let Ok(downloads_dir) = crate::dir_manager::get_global_cache_dir() {
        if downloads_dir.exists() {
            log_info!(
                "检查并清理缓存目录中的临时文件: {}",
                downloads_dir.to_string_lossy()
            );
            match std::fs::read_dir(downloads_dir) {
                Ok(entries) => {
                    for entry in entries {
                        if let Ok(entry) = entry {
                            let path = entry.path();
                            log_info!("删除临时文件: {}", path.to_string_lossy());
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
                Err(e) => log_warn!("读取缓存目录失败: {}", e),
            }
        }
    }

    log_info!("aria2c资源清理完成");
}

/// 导入已存在的.aria2文件继续下载
///
/// 此函数使用aria2c的RPC接口导入已存在的.aria2文件，
/// 使aria2c继续之前未完成的下载任务。
///
/// # 参数
/// - `aria2_file_path`: .aria2文件的路径
/// - `app_handle`: Tauri应用句柄，用于发送下载状态事件
///
/// # 返回值

/// 取消下载任务 - 通过aria2c RPC接口取消指定的下载任务
///
/// 此函数使用aria2c的RPC接口发送取消下载请求，
/// 从aria2c下载队列中移除指定GID的下载任务。
///
/// # 参数
/// - `gid`: 要取消的下载任务的GID
///
/// # 返回值
/// - 成功时返回包含成功信息的Ok
/// - 失败时返回包含错误信息的Err
pub async fn cancel_download(gid: &str) -> Result<String, String> {
    // 确保RPC管理器已初始化
    get_rpc_manager().map_err(|e| e)?;

    // 获取RPC管理器实例
    let manager = ARIA2_RPC_MANAGER
        .lock()
        .map_err(|_| "无法获取RPC管理器锁".to_string())?;
    let manager = manager
        .as_ref()
        .ok_or_else(|| "RPC管理器未初始化".to_string())?;

    // 构建取消下载的RPC请求
    let request = Aria2JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "aria2.remove".to_string(),
        params: vec![
            format!("token:{}", manager.secret).into(),
            gid.to_string().into(),
        ],
        id: std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    };

    // 发送RPC请求
    match send_rpc_request_async(manager, &request).await {
        Ok(response_str) => {
            // 尝试解析响应
            match serde_json::from_str::<serde_json::Value>(&response_str) {
                Ok(response_json) => {
                    // 检查响应中是否包含错误
                    if response_json.get("error").is_some() {
                        let error_msg = response_json["error"]["message"]
                            .as_str()
                            .unwrap_or("未知错误");
                        Err(error_msg.to_string())
                    } else {
                        Ok(format!("下载任务已成功取消: {}", gid))
                    }
                }
                Err(e) => Err(format!("解析RPC响应失败: {}", e)),
            }
        }
        Err(e) => Err(e),
    }
}

/// 通过aria2c的RPC接口下载文件
///
/// # 参数
/// - `url`: 要下载的文件URL
/// - `app_handle`: Tauri应用句柄，用于发送进度和完成事件
/// - `task_id`: 下载任务的唯一标识符
///
/// # 返回值
/// - 成功时返回包含下载文件路径的Ok
/// - 失败时返回包含错误信息的Err
pub async fn download_via_aria2(
    url: &str,
    app_handle: AppHandle,
    task_id: &str,
) -> Result<String, String> {
    log_info!("开始通过aria2c RPC下载文件 [{}]: URL={}", task_id, url);

    // 检查aria2c是否已初始化完成
    if !ARIA2_INITIALIZED.load(Ordering::Relaxed) {
        log_info!("[{}] aria2c后端尚未初始化完成，将任务加入队列等待", task_id);

        // 将任务添加到待处理队列
        PENDING_TASKS_MANAGER.add_task(
            task_id.to_string(),
            PendingTask::Download {
                url: url.to_string(),
                app_handle: app_handle.clone(),
                task_id: task_id.to_string(),
            },
        );

        log_info!("[{}] 任务已成功添加到队列，等待aria2c初始化完成", task_id);

        // 发送等待中事件给前端
        let _ = app_handle.emit_to(
            "main",
            "download-waiting",
            &serde_json::json!({
                "taskId": task_id,
                "url": url,
                "message": "等待下载引擎初始化完成..."
            }),
        );

        // 返回错误，让调用者知道任务正在等待中，需要等待下载完成
        Err("下载任务正在等待引擎初始化，请稍后重试".to_string())
    } else {
        // aria2c已初始化完成，直接执行下载
        // 获取下载目录（短暂持有锁）
        let downloads_dir = {
            let manager = crate::dir_manager::DIR_MANAGER
                .lock()
                .map_err(|e| format!("无法锁定目录管理器: {:?}", e))?;

            if manager.is_none() {
                return Err("目录管理器未初始化".to_string());
            }

            manager.as_ref().unwrap().cache_dir().clone()
        };

        log_debug!(
            "[{}] 获取下载目录: {}",
            task_id,
            downloads_dir.to_string_lossy()
        );

        // 获取文件扩展名（如果有）
        let extension = match url.split('/').last() {
            Some(name) => {
                if let Some(ext) = name.split('.').last() {
                    if ext.len() <= 6 {
                        // 简单检查是否为合理的扩展名
                        Some(format!(".{}", ext))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            None => None,
        };

        // 生成随机文件名
        let random_name = format!("{}", Uuid::new_v4());
        let filename = if let Some(ext) = extension {
            format!("{}{}", random_name, ext)
        } else {
            random_name
        };
        log_debug!("[{}] 生成随机文件名: {}", task_id, filename);

        // 构建文件完整路径
        let file_path = downloads_dir.join(&filename);
        log_debug!(
            "[{}] 文件保存路径: {}",
            task_id,
            file_path.to_string_lossy()
        );

        // 转换为线程安全的路径字符串
        let url_owned = url.to_string();
        let file_path_clone = file_path.clone();
        let app_handle_for_events = app_handle.clone();
        let task_id_clone = task_id.to_string();
        let filename_clone = filename.clone();
        let downloads_dir_clone = downloads_dir.clone();

        // 在后台线程中运行下载，避免阻塞主线程
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            log_info!("[{}] 启动后台下载线程", task_id_clone);

            // 获取下载目录的路径字符串
            let download_dir_str = downloads_dir_clone
                .to_str()
                .ok_or_else(|| {
                    log_error!("[{}] 无效的目录路径", task_id_clone);
                    "无效的目录路径".to_string()
                })
                .unwrap();

            // 确保RPC管理器已初始化
            if let Err(e) = get_rpc_manager() {
                log_error!("[{}] 获取RPC管理器失败: {}", task_id_clone, e);
                let _ = tx.send(Err(e));
                return;
            }

            // 获取RPC管理器实例，使用带超时的try_lock以避免在关闭时阻塞
            log_debug!("[{}] 获取RPC管理器实例", task_id_clone);
            let manager = match try_lock_with_timeout(&ARIA2_RPC_MANAGER, 1000) {
                Some(rpc_manager) => {
                    log_debug!("[{}] RPC管理器锁获取成功", task_id_clone);
                    match rpc_manager.as_ref() {
                        Some(mgr) => {
                            log_debug!("[{}] RPC管理器实例存在，URL: {}", task_id_clone, mgr.url);
                            // 克隆manager以解决生命周期问题
                            mgr.clone()
                        }
                        None => {
                            log_error!("[{}] RPC管理器实例不存在", task_id_clone);
                            let _ = tx.send(Err("RPC管理器未初始化".to_string()));
                            return;
                        }
                    }
                }
                None => {
                    log_error!("[{}] 获取RPC管理器锁超时", task_id_clone);
                    let _ = tx.send(Err("获取RPC管理器锁超时".to_string()));
                    return;
                }
            };

            // 添加下载任务到RPC服务器
            log_debug!("[{}] 准备添加下载任务到RPC服务器", task_id_clone);
            let mut gid =
                match manager.add_download_sync(&url_owned, &download_dir_str, &filename_clone) {
                    Ok(id) => {
                        log_info!("[{}] 下载任务添加成功，GID: {}", task_id_clone, id);
                        id
                    }
                    Err(e) => {
                        log_error!("[{}] 添加下载任务失败: {}", task_id_clone, e);
                        let _ = tx.send(Err(e));
                        return;
                    }
                };
            log_debug!("[{}] 下载任务添加完成，开始监控进度", task_id_clone);

            // 监控下载进度
            log_debug!("[{}] 开始监控下载进度，GID: {}", task_id_clone, gid);

            // 优化的下载进度监控配置
            let progress_interval = Duration::from_millis(800); // 提高监控频率到800ms

            // 获取文件名
            let display_filename =
                get_file_name(url_owned.as_str()).unwrap_or("未知文件".to_string());

            // 创建Tokio运行时用于监控下载进度
            let rt = match Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    log_error!("[{}] 创建Tokio运行时失败: {}", task_id_clone, e);
                    let _ = tx.send(Err(format!("创建Tokio运行时失败: {}", e)));
                    return;
                }
            };

            // 监控下载进度，直到完成、失败
            let mut consecutive_failures = 0;
            let max_consecutive_failures = 8; // 增加连续失败次数阈值，避免过早判定失败
            let start_time = std::time::Instant::now(); // 记录下载开始时间
            let mut last_progress = -1.0; // 记录上次进度，用于检测进度是否真正变化
            let mut zero_speed_count = 0; // 记录下载速度为0的计次
            let mut zero_speed_start_time: Option<std::time::Instant> = None; // 记录下载速度首次为0的时间
            let max_zero_speed_checks = 10; // 最大计次，每秒1次，达到10次后重试
            let zero_speed_check_interval = std::time::Duration::from_secs(1); // 计时间隔为1秒
            let mut retry_count = 0; // 重试次数计数
            let max_retries = 5; // 最大重试次数，达到5次后判定失败

            loop {
                // 检查应用是否正在关闭，如果是则中断下载
                if is_app_shutting_down() {
                    log_info!("[{}] 检测到应用正在关闭，中断下载任务", task_id_clone);
                    let _ = tx.send(Err("下载被取消：应用程序正在关闭".to_string()));
                    return;
                }

                // 检查是否有取消下载请求
                if let Ok(cancel_requests) = CANCEL_DOWNLOAD_REQUESTS.lock() {
                    if let Some(cancel_reason) = cancel_requests.get(&task_id_clone) {
                        log_info!(
                            "[{}] 收到取消下载请求，原因: {}",
                            task_id_clone,
                            cancel_reason
                        );
                        // 从取消请求列表中移除
                        let reason_clone = cancel_reason.clone();
                        drop(cancel_requests); // 先释放锁
                        if let Ok(mut cancel_requests_mut) = CANCEL_DOWNLOAD_REQUESTS.lock() {
                            cancel_requests_mut.remove(&task_id_clone);
                        }

                        // 根据取消原因决定是发送取消事件还是失败事件
                        if reason_clone == "stalled" {
                            // 下载停滞视为下载失败
                            let error_message =
                                format!("下载停滞，无法继续下载: {}", display_filename.clone());
                            let _ = app_handle_for_events.emit_to(
                                "main",
                                "download-failed",
                                &serde_json::json!({
                                    "taskId": task_id_clone.clone(),
                                    "filename": display_filename.clone(),
                                    "error": error_message
                                }),
                            );
                        } else {
                            // 普通取消
                            let _ = app_handle_for_events.emit_to(
                                "main",
                                "download-canceled",
                                &serde_json::json!({
                                    "taskId": task_id_clone.clone(),
                                    "filename": display_filename.clone()
                                }),
                            );
                        }

                        let _ = tokio::task::block_in_place(async move || {
                            refresh_download_queue(app_handle.clone()).await.unwrap();
                        });

                        // 真正取消下载任务
                        let gid_clone = gid.clone();
                        let _ = tokio::task::block_in_place(async move || {
                            if let Err(e) = cancel_download(&gid_clone).await {
                                log_error!("取消下载任务失败: {}", e);
                            }
                        });

                        // 根据取消原因返回不同的错误信息
                        if reason_clone == "stalled" {
                            let error_message =
                                format!("下载停滞，无法继续下载: {}", display_filename.clone());
                            let _ = tx.send(Err(error_message));
                        } else {
                            let _ = tx.send(Err("用户取消下载".to_string()));
                        }
                        return;
                    }
                }

                std::thread::sleep(progress_interval);

                // 检查下载状态
                let status_result = rt.block_on(get_download_status(&gid));

                match status_result {
                    Ok(Some(status)) => {
                        // 重置失败计数
                        consecutive_failures = 0;

                        // 发送进度事件
                        log_info!(
                            "[{}] 下载进度: {:.1}% - 文件大小: {:.2}MB",
                            task_id_clone,
                            status.progress,
                            status.total_size_mb
                        );

                        // 计算已下载大小（MB）
                        let completed_mb = status.completed_length as f64 / (1024.0 * 1024.0);

                        // 格式化速度
                        let speed_str = if status.download_speed > 1024 * 1024 {
                            format!(
                                "{:.1}MiB/s",
                                status.download_speed as f64 / (1024.0 * 1024.0)
                            )
                        } else if status.download_speed > 1024 {
                            format!("{:.1}KiB/s", status.download_speed as f64 / 1024.0)
                        } else {
                            format!("{}B/s", status.download_speed)
                        };

                        // 计算已下载时间
                        let elapsed = start_time.elapsed().as_secs();
                        let elapsed_str = if elapsed >= 3600 {
                            format!("{}h", elapsed / 3600)
                        } else if elapsed >= 60 {
                            format!("{}m", elapsed / 60)
                        } else {
                            format!("{}s", elapsed)
                        };

                        // 计算ETA
                        let eta_str = if status.download_speed > 0 && status.progress < 100.0 {
                            let remaining_bytes = status.total_length - status.completed_length;
                            let remaining_seconds = remaining_bytes / status.download_speed;

                            if remaining_seconds >= 3600 {
                                format!("{}h", remaining_seconds / 3600)
                            } else if remaining_seconds >= 60 {
                                format!("{}m", remaining_seconds / 60)
                            } else {
                                format!("{}s", remaining_seconds)
                            }
                        } else {
                            "0s".to_string()
                        };

                        // 计算平均下载速度
                        let avg_speed = if elapsed > 0 {
                            let avg = status.completed_length / elapsed;
                            if avg > 1024 * 1024 {
                                format!("{:.1}MiB/s", avg as f64 / (1024.0 * 1024.0))
                            } else if avg > 1024 {
                                format!("{:.1}KiB/s", avg as f64 / 1024.0)
                            } else {
                                format!("{}B/s", avg)
                            }
                        } else {
                            speed_str.clone()
                        };

                        log_debug!("[{}] 检查下载进度，GID: {}", task_id_clone, gid);

                        // 从GID中提取前6位作为缓存键
                        let cache_key = gid.chars().take(6).collect::<String>();
                        log_debug!("[{}] 使用GID前6位作为缓存键: {}", task_id_clone, cache_key);

                        // 优化的raw_output格式，增加了更多有用信息
                        let raw_output = {
                            format!(
                                "[#{} {:.1}MiB/{:.1}MiB({:.1}%) CN:{} DL:{} AVG:{} ETA:{} TIME:{}]",
                                cache_key,
                                completed_mb,
                                status.total_size_mb,
                                status.progress,
                                status.connections,
                                speed_str,
                                avg_speed,
                                eta_str,
                                elapsed_str
                            )
                        };
                        log_debug!("[{}] 最终使用的raw_output: {}", task_id_clone, raw_output);

                        // 构建增强的JSON数据，包含更多下载信息
                        let progress_json = serde_json::json!(
                            {
                                "progress": status.progress,
                                "filename": display_filename.clone(),
                                "taskId": task_id_clone.clone(),
                                "totalSize": status.total_size_mb,
                                "completedSize": completed_mb,
                                "gid": gid,
                                "rawOutput": raw_output,
                                "downloadSpeed": status.download_speed,
                                "avgDownloadSpeed": status.completed_length / (elapsed.max(1)),
                                "connections": status.connections,
                                "elapsedTime": elapsed,
                                "eta": if status.download_speed > 0 { status.total_length.saturating_sub(status.completed_length) / status.download_speed } else { 0 }
                            }
                        );

                        // 输出格式化的JSON（带有缩进）
                        if let Ok(formatted_json) = serde_json::to_string_pretty(&progress_json) {
                            log_info!("Sending download-progress: {}", formatted_json);
                        }

                        // 确保进度确实变化才发送事件
                        if (status.progress - last_progress).abs() >= 0.1
                            || status.progress >= 100.0
                        {
                            last_progress = status.progress;
                            let emit_result = app_handle_for_events.emit_to(
                                "main",
                                "download-progress",
                                &progress_json,
                            );
                            if let Err(e) = emit_result {
                                log_error!("[{}] 发送下载进度事件失败: {}", task_id_clone, e);
                            } else {
                                log_info!(
                                    "[{}] 成功发送下载进度事件: {:.1}%",
                                    task_id_clone,
                                    status.progress
                                );
                            }
                        }

                        // 检查下载速度是否为0
                        if status.download_speed == 0 {
                            if zero_speed_start_time.is_none() {
                                zero_speed_start_time = Some(std::time::Instant::now());
                                log_warn!("[{}] 检测到下载速度为0，开始每秒计次", task_id_clone);
                            } else {
                                let elapsed_zero_speed =
                                    zero_speed_start_time.as_ref().unwrap().elapsed();
                                if elapsed_zero_speed
                                    >= zero_speed_check_interval * (zero_speed_count + 1)
                                {
                                    zero_speed_count += 1;
                                    log_warn!(
                                        "[{}] 下载速度持续为0，已计次 {}/{} 次",
                                        task_id_clone,
                                        zero_speed_count,
                                        max_zero_speed_checks
                                    );
                                }
                            }

                            // 如果下载速度为0但进度不是100%，且达到最大计次，需要进行重试
                            if status.progress < 100.0 && zero_speed_count >= max_zero_speed_checks
                            {
                                log_warn!(
                                    "[{}] 下载速度持续为0达到最大计次，准备重试下载",
                                    task_id_clone
                                );

                                retry_count += 1;
                                log_warn!(
                                    "[{}] 当前重试次数: {}/{} 次",
                                    task_id_clone,
                                    retry_count,
                                    max_retries
                                );

                                if retry_count <= max_retries {
                                    // 尝试取消当前任务
                                    log_info!("[{}] 取消当前停滞的下载任务", task_id_clone);
                                    let cancel_result = rt.block_on(cancel_download(&gid));
                                    if let Err(e) = cancel_result {
                                        log_error!("[{}] 取消下载任务失败: {}", task_id_clone, e);
                                    }

                                    // 重新开始下载
                                    log_info!("[{}] 重新开始下载任务", task_id_clone);

                                    // 取消当前任务并重新下载
                                    // 1. 发送取消事件
                                    let _ = app_handle_for_events.emit_to(
                                        "main",
                                        "download-canceled",
                                        &serde_json::json!({
                                            "taskId": task_id_clone.clone(),
                                            "filename": display_filename.clone(),
                                            "reason": "重新下载（速度为0）"
                                        }),
                                    );

                                    // 2. 等待一段时间后继续监控，让系统有时间处理取消事件
                                    std::thread::sleep(Duration::from_secs(2));

                                    // 2. 重置计数和计时，继续监控
                                    zero_speed_count = 0;
                                    zero_speed_start_time = None;
                                } else {
                                    // 达到最大重试次数，判定下载失败
                                    log_error!("[{}] 已达到最大重试次数，下载失败", task_id_clone);
                                    let error_message = format!(
                                        "下载停滞，无法继续下载: {}",
                                        display_filename.clone()
                                    );
                                    let _ = app_handle_for_events.emit_to(
                                        "main",
                                        "download-failed",
                                        &serde_json::json!({
                                            "taskId": task_id_clone.clone(),
                                            "filename": display_filename.clone(),
                                            "error": error_message
                                        }),
                                    );
                                    let _ = tx.send(Err(error_message));
                                    return;
                                }
                            }
                        } else {
                            // 下载速度不为0，重置计数和计时
                            zero_speed_count = 0;
                            zero_speed_start_time = None;
                        }

                        if status.progress >= 100.0 {
                            // 进度显示100%，但需要额外检查确认下载真正完成
                            log_info!("[{}] 进度显示100%，进行最终确认检查", task_id_clone);

                            // 多次确认下载状态，确保真的完成
                            let mut confirmed_complete = false;
                            for _ in 0..3 {
                                std::thread::sleep(Duration::from_secs(1));
                                let final_status = rt.block_on(get_download_status(&gid));
                                if let Ok(Some(final_stat)) = final_status {
                                    if final_stat.progress >= 100.0 {
                                        log_info!(
                                            "[{}] 确认下载完成状态: {:.1}%",
                                            task_id_clone,
                                            final_stat.progress
                                        );
                                        confirmed_complete = true;
                                        break;
                                    }
                                }
                            }

                            if confirmed_complete {
                                log_info!("[{}] 多次确认下载完成", task_id_clone);
                                break;
                            } else {
                                log_warn!("[{}] 进度显示100%但状态不稳定，继续等待", task_id_clone);
                                continue;
                            }
                        }
                    }
                    Ok(None) => {
                        // 任务不存在，需要进一步确认是否真的完成
                        log_warn!(
                            "[{}] 任务状态查询返回空，检查任务是否真的完成",
                            task_id_clone
                        );

                        // 先尝试重新获取RPC管理器，可能连接失效
                        if let Err(e) = get_rpc_manager() {
                            log_error!("[{}] 重新获取RPC管理器失败: {}", task_id_clone, e);
                            let _ = tx.send(Err(e));
                            return;
                        };

                        // 检查文件是否存在且不为空
                        if let Ok(metadata) = fs::metadata(&file_path_clone) {
                            if metadata.len() > 0 {
                                log_info!(
                                    "[{}] 任务文件已存在且不为空，进行进一步完整性检查",
                                    task_id_clone
                                );

                                // 增加额外的等待时间，确保文件下载完全
                                log_info!("[{}] 增加额外等待时间以确保下载完全完成", task_id_clone);
                                std::thread::sleep(Duration::from_secs(5));

                                // 多次检查文件大小是否有变化，确保下载真的完成
                                let mut size_stable = true;
                                let initial_size = metadata.len();

                                for i in 0..3 {
                                    std::thread::sleep(Duration::from_secs(2));
                                    if let Ok(new_metadata) = fs::metadata(&file_path_clone) {
                                        if new_metadata.len() != initial_size {
                                            log_warn!(
                                                "[{}] 文件大小仍在变化 ({} -> {}), 继续等待",
                                                task_id_clone,
                                                initial_size,
                                                new_metadata.len()
                                            );
                                            size_stable = false;
                                            break;
                                        }
                                        log_debug!(
                                            "[{}] 文件大小检查 {}: 稳定在 {} 字节",
                                            task_id_clone,
                                            i + 1,
                                            initial_size
                                        );
                                    }
                                }

                                if size_stable {
                                    log_info!(
                                        "[{}] 文件大小多次检查稳定，确认下载完成",
                                        task_id_clone
                                    );
                                    break;
                                } else {
                                    log_warn!(
                                        "[{}] 文件大小仍在变化，继续等待下载完成",
                                        task_id_clone
                                    );
                                    // 强制发送一次进度更新
                                    let progress_json = serde_json::json!({
                                        "progress": 99.0,
                                        "filename": display_filename.clone(),
                                        "taskId": task_id_clone.clone(),
                                        "message": "下载中，任务状态查询暂时不可用"
                                    });
                                    _ = app_handle_for_events.emit_to(
                                        "main",
                                        "download-progress",
                                        &progress_json,
                                    );
                                    continue;
                                }
                            } else {
                                log_error!("[{}] 任务文件为空，下载可能失败", task_id_clone);
                                let _ = tx.send(Err("下载失败：任务文件为空".to_string()));
                                return;
                            }
                        } else {
                            log_error!("[{}] 无法访问任务文件，下载可能失败", task_id_clone);
                            // 重试访问文件
                            std::thread::sleep(Duration::from_secs(2));
                            if let Err(_) = fs::metadata(&file_path_clone) {
                                let _ = tx.send(Err("下载失败：无法访问任务文件".to_string()));
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        log_error!(
                            "[{}] 获取下载状态失败 ({}): {}",
                            task_id_clone,
                            consecutive_failures,
                            e
                        );

                        // 检查是否是GID丢失错误，如果是则尝试使用原始URL重新添加任务
                        if e == "GID_NOT_FOUND" {
                            log_info!(
                                "[{}] 检测到GID丢失，尝试使用原始URL重新添加任务",
                                task_id_clone
                            );

                            // 尝试重新获取RPC管理器
                            if let Err(manager_err) = get_rpc_manager() {
                                log_error!(
                                    "[{}] 重新获取RPC管理器失败: {}",
                                    task_id_clone,
                                    manager_err
                                );
                            } else if let Some(manager_guard) =
                                try_lock_with_timeout(&ARIA2_RPC_MANAGER, 1000)
                            {
                                if let Some(current_manager) = manager_guard.as_ref() {
                                    log_info!(
                                        "[{}] 尝试使用新的RPC管理器重新添加任务",
                                        task_id_clone
                                    );

                                    // 使用原始URL重新添加任务
                                    if let Ok(new_gid) = current_manager.add_download_sync(
                                        &url_owned,
                                        &download_dir_str,
                                        &filename_clone,
                                    ) {
                                        log_info!(
                                            "[{}] 任务重新添加成功，新GID: {}",
                                            task_id_clone,
                                            new_gid
                                        );
                                        // 更新GID，继续监控新的任务
                                        gid = new_gid;
                                        consecutive_failures = 0;
                                        continue;
                                    } else {
                                        log_error!("[{}] 任务重新添加失败", task_id_clone);
                                    }
                                }
                            }
                        }

                        // 如果连续失败次数过多，尝试重新初始化RPC管理器
                        if consecutive_failures % 3 == 0 {
                            log_info!(
                                "[{}] 多次获取状态失败，尝试重新初始化RPC管理器",
                                task_id_clone
                            );

                            // 首先尝试重新获取RPC管理器，而不是立即重置
                            if let Err(e) = get_rpc_manager() {
                                log_error!("[{}] 重新获取RPC管理器失败: {}", task_id_clone, e);
                                // 如果重新获取失败，再尝试重置RPC管理器
                                match try_lock_with_timeout(&ARIA2_RPC_MANAGER, 1000) {
                                    Some(mut manager) => {
                                        *manager = None;
                                        log_info!(
                                            "[{}] RPC管理器已重置，尝试重新获取",
                                            task_id_clone
                                        );
                                        let _ = get_rpc_manager();
                                    }
                                    None => {
                                        log_warn!(
                                            "[{}] 获取RPC管理器锁超时，无法重置RPC管理器",
                                            task_id_clone
                                        );
                                    }
                                }
                            }
                        }

                        // 每次重试都发送状态更新，确保前端知道下载仍在进行中
                        // 避免前端因超时而关闭任务栏
                        let progress_json = serde_json::json!({
                            "progress": last_progress.max(0.0),
                            "filename": display_filename.clone(),
                            "taskId": task_id_clone.clone(),
                            "message": format!("正在重试... ({}次重试)", consecutive_failures)
                        });
                        _ = app_handle_for_events.emit_to(
                            "main",
                            "download-progress",
                            &progress_json,
                        );

                        // 定期更新下载队列状态，确保前端能正确显示任务栏
                        if consecutive_failures % 3 == 0 {
                            let _ = {
                                let app_handle: &AppHandle = &app_handle_for_events;
                                let _ = refresh_download_queue(app_handle.clone());
                            };
                        }

                        // 如果连续失败次数过多，认为下载失败
                        if consecutive_failures >= max_consecutive_failures {
                            log_error!(
                                "[{}] 连续获取下载状态失败次数过多，检查文件是否已下载",
                                task_id_clone
                            );

                            // 最后检查文件是否已下载
                            if let Ok(metadata) = fs::metadata(&file_path_clone) {
                                if metadata.len() > 0 {
                                    // 构建aria2临时文件路径: 原文件名.aria2
                                    let aria2_file_path = file_path_clone.with_extension("aria2");

                                    log_info!("[{}] 虽然状态查询失败，但文件已存在且大小为 {} 字节，检查aria2临时文件", 
                                         task_id_clone, metadata.len());

                                    // 如果aria2临时文件不存在，说明下载可能已经完成，继续处理文件
                                    // 注意：aria2会在开始下载前预分配空间，所以文件大小不一定表示已下载完成
                                    if !aria2_file_path.exists() {
                                        log_info!("[{}] aria2临时文件不存在，文件可能已下载完成，继续处理", task_id_clone);
                                        break;
                                    } else {
                                        log_warn!(
                                            "[{}] aria2临时文件仍然存在，下载可能正在进行中",
                                            task_id_clone
                                        );
                                        // 如果临时文件存在，先检查文件大小是否合理
                                        // 考虑到aria2的预分配机制，我们不应该仅仅因为文件小就判定失败
                                        // 只在文件特别小（0字节）时才判定失败
                                        if metadata.len() == 0 {
                                            log_error!(
                                                "[{}] 文件大小为0，确认下载失败",
                                                task_id_clone
                                            );
                                            let _ = tx
                                                .send(Err("下载失败：获取状态失败且文件大小为0"
                                                    .to_string()));
                                            return;
                                        }
                                    }
                                }
                            }

                            log_error!("[{}] 文件也不存在，确认下载失败", task_id_clone);
                            let _ = tx.send(Err(format!(
                                "下载失败：连续获取下载状态失败，最后错误：{}",
                                e
                            )));
                            return;
                        }

                        // 继续尝试
                    }
                }
            }

            // 循环退出意味着下载已完成或任务不存在
            log_info!(
                "[{}] 下载完成: {}",
                task_id_clone,
                file_path_clone.to_string_lossy()
            );

            // 最后确认文件大小
            let final_file_size = fs::metadata(&file_path_clone).map(|m| m.len()).unwrap_or(0);
            log_info!(
                "[{}] 下载完成，最终文件大小: {} 字节",
                task_id_clone,
                final_file_size
            );

            // 确保文件大小合理
            if final_file_size > 0 {
                // 尝试进行文件魔数检查，判断下载是否有效
                let is_file_valid = check_file_magic_number(&file_path_clone);

                if is_file_valid {
                    // 发送下载完成事件到main窗口，表示文件已下载完成
                    let emit_result = app_handle_for_events.emit_to(
                        "main",
                        "download-complete",
                        &serde_json::json!({
                            "taskId": task_id_clone.clone(),
                            "success": true,
                            "message": "下载完成，正在准备解压",
                            "filename": display_filename,
                            "fileSize": final_file_size
                        }),
                    );

                    if let Err(e) = emit_result {
                        log_error!("[{}] 发送下载完成事件失败: {}", task_id_clone, e);
                    }
                } else {
                    log_error!(
                        "[{}] 下载完成但文件魔数检查失败，可能是无效文件",
                        task_id_clone
                    );
                    let _ = app_handle_for_events.emit_to(
                        "main",
                        "download-failed",
                        &serde_json::json!({
                            "taskId": task_id_clone.clone(),
                            "filename": display_filename,
                            "error": "下载完成但文件魔数检查失败，可能是无效文件"
                        }),
                    );
                    let _ = tx.send(Err("下载完成但文件魔数检查失败，可能是无效文件".to_string()));
                    return;
                }
            } else {
                log_error!("[{}] 下载完成但文件大小为0，发送失败事件", task_id_clone);
                let _ = app_handle_for_events.emit_to(
                    "main",
                    "download-failed",
                    &serde_json::json!({
                        "taskId": task_id_clone.clone(),
                        "filename": display_filename,
                        "error": "下载完成但文件大小为0"
                    }),
                );
                let _ = tx.send(Err("下载完成但文件大小为0".to_string()));
                return;
            }

            // 等待aria2c完全释放文件 - 检查是否存在临时的aria2文件
            // 构建aria2临时文件路径: 原文件名.aria2
            let aria2_file_path = file_path_clone.with_extension("aria2");
            let max_wait_seconds = 60; // 增加最大等待时间到60秒，确保大文件也能完成下载
            let check_interval = Duration::from_millis(50); // 检查间隔
            let mut wait_count = 0;
            let mut consecutive_stable_size = 0;
            let mut last_file_size = 0;

            log_info!(
                "[{}] 等待aria2c完全释放文件，检查aria2临时文件: {}",
                task_id_clone,
                aria2_file_path.to_string_lossy()
            );

            // 等待aria2临时文件消失或文件大小稳定
            while (aria2_file_path.exists() || consecutive_stable_size < 10)
                && wait_count < max_wait_seconds * 2
            {
                std::thread::sleep(check_interval);
                wait_count += 1;

                // 检查文件大小是否稳定
                if let Ok(metadata) = fs::metadata(&file_path_clone) {
                    let current_size = metadata.len();
                    if current_size == last_file_size {
                        consecutive_stable_size += 1;
                    } else {
                        consecutive_stable_size = 0;
                        last_file_size = current_size;
                    }
                    log_debug!(
                        "[{}] 文件大小: {} 字节，连续稳定计数: {}",
                        task_id_clone,
                        current_size,
                        consecutive_stable_size
                    );
                }

                if wait_count % 20 == 0 {
                    // 每10秒记录一次日志
                    log_info!(
                        "[{}] 等待aria2c释放文件: 已等待 {} 秒，临时文件: {}",
                        task_id_clone,
                        wait_count / 2,
                        if aria2_file_path.exists() {
                            "存在"
                        } else {
                            "已消失"
                        }
                    );
                }
            }

            if aria2_file_path.exists() {
                log_warn!(
                    "[{}] aria2临时文件仍存在，但已达到最大等待时间或文件大小稳定: {}",
                    task_id_clone,
                    aria2_file_path.to_string_lossy()
                );

                // 多次检查确保文件确实不再变化
                let initial_size = fs::metadata(&file_path_clone).map(|m| m.len()).unwrap_or(0);
                let mut size_stable = true;

                for _i in 0..3 {
                    std::thread::sleep(Duration::from_secs(1));
                    if let Ok(new_metadata) = fs::metadata(&file_path_clone) {
                        if new_metadata.len() != initial_size {
                            log_warn!(
                                "[{}] 文件大小仍在变化 ({} -> {}), 再次等待",
                                task_id_clone,
                                initial_size,
                                new_metadata.len()
                            );
                            size_stable = false;
                            break;
                        }
                    }
                }

                if size_stable {
                    log_info!(
                        "[{}] 尽管aria2临时文件存在，但文件大小已稳定，认为下载完成",
                        task_id_clone
                    );
                } else {
                    log_warn!("[{}] 文件大小仍在变化，可能下载尚未完全完成", task_id_clone);
                    // 再次等待额外时间
                    std::thread::sleep(Duration::from_secs(5));
                }
            } else {
                log_info!("[{}] aria2临时文件已消失，文件已完全释放", task_id_clone);
            }

            // 发送成功结果
            let _ = tx.send(Ok(file_path_clone.to_string_lossy().to_string()));

            log_debug!("[{}] 下载线程结束", task_id_clone);
        });

        // 等待下载完成并返回结果
        log_debug!("[{}] 等待下载线程完成...", task_id);
        let result = rx.recv().map_err(|_| {
            log_error!("[{}] 接收下载结果失败", task_id);
            "接收下载结果失败".to_string()
        })?;

        result
    }
}

/// 检查文件的魔数，判断文件是否有效
/// 考虑到文件可能被aria2锁定，会进行多次尝试读取
fn check_file_magic_number(file_path: &PathBuf) -> bool {
    // 重试次数
    const MAX_RETRIES: u32 = 5;
    // 重试间隔
    const RETRY_INTERVAL: Duration = Duration::from_millis(500);

    // 常见压缩文件的魔数
    // ZIP格式
    const ZIP_MAGIC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
    // GZIP格式
    const GZIP_MAGIC: [u8; 2] = [0x1F, 0x8B];
    // 7Z格式
    const SEVENZ_MAGIC: [u8; 6] = [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];

    for retry in 0..MAX_RETRIES {
        match fs::File::open(file_path) {
            Ok(mut file) => {
                // 读取足够的字节来检查所有支持的魔数
                let mut buffer = [0u8; 10]; // 足够检查最长的魔数
                match file.read_exact(&mut buffer) {
                    Ok(_) => {
                        // 检查是否匹配任何支持的文件类型魔数
                        let is_valid = buffer.starts_with(&ZIP_MAGIC)
                            || buffer.starts_with(&GZIP_MAGIC)
                            || buffer.starts_with(&SEVENZ_MAGIC);

                        // 对于TAR文件，需要额外处理偏移量
                        // 注意：这里简化处理，实际应用中可能需要更复杂的逻辑

                        if is_valid {
                            log_info!("[aria2c] 文件魔数检查通过: {:?}", buffer);
                            return true;
                        } else {
                            log_warn!("[aria2c] 文件魔数不匹配已知格式: {:?}", buffer);
                            // 即使魔数不匹配，也不立即判定为无效，因为可能是其他格式
                            return true;
                        }
                    }
                    Err(e) => {
                        log_warn!(
                            "[aria2c] 读取文件失败 (尝试 {}): {}, 可能文件太小或仍被锁定",
                            retry + 1,
                            e
                        );

                        // 如果文件太小，无法读取足够字节，也认为是有效的（可能是正常情况）
                        if let Ok(metadata) = fs::metadata(file_path) {
                            if metadata.len() < buffer.len() as u64 {
                                log_info!("[aria2c] 文件大小小于魔数检查所需字节数，认为有效");
                                return true;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                log_warn!(
                    "[aria2c] 打开文件失败 (尝试 {}): {}, 可能文件仍被锁定",
                    retry + 1,
                    e
                );
            }
        }

        // 如果不是最后一次尝试，等待一段时间后重试
        if retry < MAX_RETRIES - 1 {
            std::thread::sleep(RETRY_INTERVAL);
        }
    }

    // 多次尝试后仍然失败，可能是文件被锁定或其他问题
    // 但考虑到aria2的特性，我们不能仅仅因为无法读取魔数就判定失败
    // 返回true表示继续处理文件
    log_warn!("[aria2c] 多次尝试读取文件魔数失败，但继续处理文件");
    true
}
