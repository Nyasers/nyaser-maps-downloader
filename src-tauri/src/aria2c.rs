// aria2c 模块 - 负责通过aria2c工具下载文件，提供文件下载功能

// 标准库导入
use std::{
    collections::HashSet,
    env, fs,
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
use serde::{Deserialize, Serialize};
use serde_json;
use tauri::{AppHandle, Emitter};
use tokio::{runtime::Runtime, sync::oneshot};
use uuid::Uuid;

// 内部模块导入
use crate::{
    init::is_app_shutting_down, log_debug, log_error, log_info, log_utils::redirect_process_output,
    log_warn, queue_manager::QueueManager,
};

// 定义下载任务队列项结构
#[derive(Debug)]
enum PendingTask {
    Download {
        url: String,
        app_handle: AppHandle,
        task_id: String,
        responder: oneshot::Sender<Result<String, String>>,
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
    // RPC服务器管理单例
    static ref ARIA2_RPC_MANAGER: Mutex<Option<Aria2RpcManager>> = Mutex::new(None);
    // 保存上次成功启动的aria2c信息（端口、密钥、PID）
    static ref LAST_ARIA2_INFO: Mutex<Option<(u16, String, u32)>> = Mutex::new(None);
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
                if true {
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

            let pid = self.pid;
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
                        if let Ok(mut manager) = ARIA2_RPC_MANAGER.lock() {
                            if let Some(current_manager) = manager.as_ref() {
                                if current_manager.pid == pid {
                                    log_info!("准备重启aria2c服务");

                                    // 从上次保存的信息中获取端口和密钥
                                    if let Some((last_port, last_secret, _)) =
                                        { (*LAST_ARIA2_INFO.lock().unwrap()).clone() }
                                    {
                                        log_info!(
                                            "尝试使用上次的配置重启aria2c: 端口={}",
                                            last_port
                                        );

                                        match start_aria2c_rpc_server(last_port, &last_secret) {
                                            Ok(process) => {
                                                let new_pid = process.id();
                                                log_info!("aria2c服务重启成功，新PID: {}", new_pid);

                                                // 更新管理器信息
                                                *manager = Some(Aria2RpcManager {
                                                    url: format!(
                                                        "http://localhost:{}/jsonrpc",
                                                        last_port
                                                    ),
                                                    secret: last_secret.clone(),
                                                    process: Some(process),
                                                    pid: new_pid,
                                                    is_monitored: AtomicBool::new(false),
                                                });

                                                // 更新上次成功启动的信息
                                                *LAST_ARIA2_INFO.lock().unwrap() =
                                                    Some((last_port, last_secret, new_pid));

                                                // 重启监控
                                                if let Some(new_manager) = manager.as_ref() {
                                                    new_manager.start_process_monitoring();
                                                }
                                            }
                                            Err(e) => {
                                                log_error!("重启aria2c服务失败: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // 结束当前监控线程
                        break;
                    }
                }
            });
        }
    }

    // 添加下载任务到RPC服务器
    pub fn add_download(
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
            "console-log-level": "notice"
        });
        params.push(options);

        // 构建JSON-RPC请求
        let request = Aria2JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "aria2.addUri".to_string(),
            params,
            id: 1,
        };

        // 发送请求并获取响应
        let response = send_rpc_request(self, &request)?;

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

    // 获取aria2c可执行文件路径
    let aria2c_path = get_aria2c_path();
    log_debug!("aria2c路径: {:?}", aria2c_path);

    // 构建命令行参数
    let mut command = Command::new(aria2c_path);
    command
        .arg("--enable-rpc")
        .arg(format!("--rpc-listen-port={}", port))
        // 设置为true允许监听所有接口，但我们的连接代码会明确使用localhost
        .arg("--rpc-listen-all=false")
        .arg(format!("--rpc-secret={}", secret))
        .arg("--rpc-allow-origin-all")
        .arg("--continue=true")
        .arg("--max-concurrent-downloads=5")
        .arg("--max-connection-per-server=16")
        .arg("--min-split-size=1M")
        .arg("--split=16")
        .arg("--console-log-level=info") // 设置更详细的日志级别以便捕获更多信息
        .stdout(Stdio::piped()) // 捕获stdout输出
        .stderr(Stdio::piped()) // 捕获stderr输出
        .stdin(Stdio::null());

    // 在Windows上，隐藏窗口运行
    command.creation_flags(0x08000000);

    // 启动进程
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动aria2c RPC服务器失败: {}", e))?;

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

// 使用PowerShell发送RPC请求
fn send_rpc_request_via_powershell(
    manager: Aria2RpcManager,
    request: Aria2JsonRpcRequest,
) -> Result<String, String> {
    log_debug!("使用PowerShell发送RPC请求到: {}", manager.url);
    log_debug!("请求内容: {:?}", request);

    // 将请求序列化为JSON字符串
    let request_json =
        serde_json::to_string(&request).map_err(|e| format!("序列化请求失败: {}", e))?;

    // 转义JSON字符串中的引号，使其能在PowerShell命令中使用
    // 在PowerShell中，单引号内的字符串需要转义单引号，而不是双引号
    let escaped_json = request_json.replace("'", "''");

    // 构建PowerShell命令
    // 使用Invoke-WebRequest代替Invoke-RestMethod以获取原始JSON响应
    // 添加-UseBasicParsing以在没有Internet Explorer的环境中也能工作
    // 使用.Content获取原始响应内容
    let powershell_command = format!(
        "(Invoke-WebRequest -Uri '{}' -Method Post -ContentType 'application/json' -Body '{}' -UseBasicParsing).Content",
        manager.url,
        escaped_json
    );

    log_debug!("PowerShell命令: {}", powershell_command);

    // 定义最大重试次数和初始重试间隔
    const MAX_RETRIES: u8 = 3;
    let mut retry_interval = Duration::from_millis(500);
    let mut last_error = "未知错误".to_string();

    // 执行请求，带重试机制
    for attempt in 0..=MAX_RETRIES {
        // 创建PowerShell进程
        let mut command = Command::new("powershell.exe");
        command
            .arg("-Command")
            .arg(&powershell_command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // 隐藏窗口运行
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW 标志

        // 执行命令
        match command.output() {
            Ok(output) => {
                // 检查命令是否成功执行
                if output.status.success() {
                    // 读取响应内容
                    let mut response_text = String::from_utf8_lossy(&output.stdout).to_string();

                    // 检查响应是否为空
                    if !response_text.is_empty() {
                        // 处理可能的ASCII码序列（例如从Invoke-WebRequest返回的响应）
                        if response_text
                            .lines()
                            .all(|line| line.trim().parse::<u8>().is_ok())
                        {
                            log_debug!("检测到ASCII码序列响应，尝试转换为原始JSON");
                            let mut decoded_json = String::new();

                            for line in response_text.lines() {
                                if let Ok(ascii_code) = line.trim().parse::<u8>() {
                                    decoded_json.push(ascii_code as char);
                                }
                            }

                            response_text = decoded_json;
                        }

                        log_debug!("RPC请求成功，响应长度: {}", response_text.len());
                        // 记录完整响应内容用于调试
                        log_debug!("响应内容: {}", response_text);
                        // 检查响应是否以'{'开始，确保是有效的JSON
                        let trimmed_response = response_text.trim();
                        if trimmed_response.starts_with('{') {
                            // 已经是有效的JSON格式
                            return Ok(response_text);
                        } else {
                            log_warn!("响应不是有效的JSON格式，开始尝试解析");

                            // 检查是否包含PowerShell对象表示法(@{...})
                            if trimmed_response.contains("@{") {
                                log_debug!("检测到PowerShell对象表示法");

                                // 对于复杂命令(tellStatus等)，尝试从PowerShell对象转换
                                if request.method == "aria2.tellStatus" {
                                    // 提取PowerShell对象部分
                                    if let Some(start) = trimmed_response.find("@{") {
                                        if let Some(end) = trimmed_response.rfind("}") {
                                            let ps_object = &trimmed_response[start..=end];
                                            log_debug!("提取的PowerShell对象: {}", ps_object);

                                            // 简单的PowerShell对象到JSON的转换
                                            // 注意：这是一个简化版本，可能需要根据实际情况进行调整
                                            let mut json_obj = String::from("{\n");
                                            let pairs: Vec<&str> = ps_object
                                                [2..ps_object.len() - 1]
                                                .split(';')
                                                .map(|s| s.trim())
                                                .collect();

                                            for (i, pair) in pairs.iter().enumerate() {
                                                if let Some(equal_pos) = pair.find('=') {
                                                    let key = pair[..equal_pos].trim();
                                                    let value = pair[equal_pos + 1..].trim();

                                                    // 处理不同类型的值
                                                    let json_value = if value.starts_with('"')
                                                        && value.ends_with('"')
                                                    {
                                                        // 字符串值
                                                        value.to_string()
                                                    } else if value.starts_with('[')
                                                        && value.ends_with(']')
                                                    {
                                                        // 数组值
                                                        value.to_string()
                                                    } else if value.starts_with('@') {
                                                        // 嵌套对象
                                                        // 这里简化处理，实际可能需要递归解析
                                                        format!("\"{}\"", value)
                                                    } else {
                                                        // 数字或布尔值
                                                        value.to_string()
                                                    };

                                                    json_obj.push_str(&format!(
                                                        "\"{}\": {}",
                                                        key, json_value
                                                    ));
                                                    if i < pairs.len() - 1 {
                                                        json_obj.push_str(",");
                                                    }
                                                    json_obj.push_str("\n");
                                                }
                                            }
                                            json_obj.push_str("}");
                                            log_debug!("转换后的JSON对象: {}", json_obj);

                                            // 构建完整的JSON-RPC响应
                                            let json_response = format!(
                                                "{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}}",
                                                json_obj
                                            );
                                            log_debug!("构建的JSON-RPC响应: {}", json_response);
                                            return Ok(json_response);
                                        }
                                    }
                                }
                            }

                            // 尝试清理响应，移除可能的PowerShell额外输出
                            if let Some(json_start) = response_text.find('{') {
                                if let Some(json_end) = response_text.rfind('}') {
                                    let cleaned_response = &response_text[json_start..=json_end];
                                    log_debug!("清理后的响应: {}", cleaned_response);
                                    return Ok(cleaned_response.to_string());
                                }
                            }

                            // 对于addUri等简单命令，尝试解析PowerShell表格格式输出
                            // 注意：这种解析只适用于返回简单字符串的命令，不适用于返回复杂对象的命令
                            if request.method == "aria2.addUri" {
                                let lines: Vec<&str> = trimmed_response.lines().collect();
                                if lines.len() >= 3 {
                                    // 检查是否包含表格标题行
                                    let header_line = lines[0].to_lowercase();
                                    if header_line.contains("id")
                                        && header_line.contains("jsonrpc")
                                        && header_line.contains("result")
                                    {
                                        // 表格格式确认，处理数据行
                                        for line in &lines[2..] {
                                            let parts: Vec<&str> =
                                                line.split_whitespace().collect();
                                            if parts.len() >= 3 {
                                                // 提取id, jsonrpc和result(gid)
                                                let gid = parts[2].to_string();
                                                log_info!("从表格格式响应中成功提取GID: {}", gid);

                                                // 构建预期的JSON响应格式
                                                let json_response = format!(
                                                    "{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"{}\"}}",
                                                    gid
                                                );
                                                log_debug!("构建的JSON响应: {}", json_response);
                                                return Ok(json_response);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        return Ok(response_text);
                    } else {
                        last_error = "RPC响应为空".to_string();
                    }
                } else {
                    // 命令执行失败
                    let error_output = String::from_utf8_lossy(&output.stderr).to_string();
                    last_error = format!("PowerShell命令执行失败: {}", error_output);
                    log_warn!(
                        "PowerShell执行失败 ({}), 尝试重试 ({} of {})...",
                        last_error,
                        attempt + 1,
                        MAX_RETRIES
                    );
                }
            }
            Err(e) => {
                // 进程执行错误
                last_error = format!("启动PowerShell进程失败: {}", e);
                log_warn!(
                    "PowerShell进程错误: {}, 尝试重试 ({} of {})...",
                    e,
                    attempt + 1,
                    MAX_RETRIES
                );
            }
        }

        // 如果不是最后一次尝试，等待重试间隔
        if attempt < MAX_RETRIES {
            std::thread::sleep(retry_interval);
            // 指数退避
            retry_interval = retry_interval.saturating_mul(2);
        }
    }

    // 所有重试都失败
    log_error!("RPC请求失败: {}", last_error);
    Err(last_error)
}

// 异步发送RPC请求（使用PowerShell）
async fn send_rpc_request_async(
    manager: &Aria2RpcManager,
    request: &Aria2JsonRpcRequest,
) -> Result<String, String> {
    // 在单独的线程中执行PowerShell请求，避免阻塞异步运行时
    let manager_clone = manager.clone();
    let request_clone = request.clone();

    // 使用tokio::task::spawn_blocking在线程池中执行阻塞操作
    tokio::task::spawn_blocking(move || {
        send_rpc_request_via_powershell(manager_clone, request_clone)
    })
    .await
    .map_err(|e| format!("异步任务执行失败: {}", e))?
}

// 同步包装器 - 用于在非异步环境中调用
fn send_rpc_request(
    manager: &Aria2RpcManager,
    request: &Aria2JsonRpcRequest,
) -> Result<String, String> {
    send_rpc_request_via_powershell(manager.clone(), request.clone())
}

// 获取下载任务状态
async fn get_download_status(
    manager: &Aria2RpcManager,
    gid: &str,
) -> Result<Option<DownloadStatus>, String> {
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
    let response = match send_rpc_request_async(manager, &request).await {
        Ok(response) => response,
        Err(e) => return Err(e),
    };

    // 解析响应
    let response: Aria2JsonRpcResponse<serde_json::Value> = match serde_json::from_str(&response) {
        Ok(response) => response,
        Err(e) => return Err(format!("解析RPC响应失败: {}", e)),
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

// 检查任务是否存在于aria2c中
async fn check_task_exists(manager: &Aria2RpcManager, gid: &str, _rt: &Runtime) -> bool {
    log_debug!("检查任务是否存在: {}", gid);

    let request = Aria2JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "aria2.tellStatus".to_string(),
        params: vec![
            serde_json::Value::String(format!("token:{}", manager.secret)),
            serde_json::Value::String(gid.to_string()),
        ],
        id: 1,
    };

    match send_rpc_request_async(manager, &request).await {
        Ok(response) => {
            // 检查响应是否包含错误
            if response.contains("error") {
                log_debug!("任务不存在或已完成: {}", gid);
                false
            } else {
                log_debug!("任务存在: {}", gid);
                true
            }
        }
        Err(e) => {
            log_error!("检查任务存在性失败: {}", e);
            false
        }
    }
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
pub fn get_rpc_manager() -> Result<&'static Mutex<Option<Aria2RpcManager>>, String> {
    log_debug!("尝试获取全局RPC管理器实例");

    // 检查是否已经初始化RPC管理器
    let mut manager = match ARIA2_RPC_MANAGER.lock() {
        Ok(guard) => {
            log_debug!("获取RPC管理器锁成功");
            guard
        }
        Err(poisoned) => {
            // 处理中毒的锁 - 尝试恢复
            log_warn!("RPC管理器锁被中毒，尝试恢复...");
            poisoned.into_inner()
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

    log_debug!("RPC管理器获取成功，返回实例");
    Ok(&ARIA2_RPC_MANAGER)
}

/// 处理队列中的单个待处理下载任务
fn process_pending_task(task: PendingTask) {
    match task {
        PendingTask::Download {
            url,
            app_handle,
            task_id,
            responder,
        } => {
            log_info!("处理待处理的下载任务: [{}] URL={}", task_id, url);

            // 在新的异步任务中执行下载
            tauri::async_runtime::spawn(async move {
                // 异步执行下载
                let download_result = download_via_aria2(&url, app_handle, &task_id).await;

                // 发送结果给等待的调用者
                let _ = responder.send(download_result);
            });
        }
    }
}

/// 获取任务ID的函数
fn get_pending_task_id(task: &PendingTask) -> String {
    match task {
        PendingTask::Download { task_id, .. } => task_id.clone(),
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
        get_pending_task_id,
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
/// 这个函数应该在应用关闭时调用，确保aria2c RPC服务器正确关闭
pub fn cleanup_aria2c_resources() {
    log_info!("应用关闭时清理aria2c资源...");

    // 关闭RPC管理器 - 使用try_lock避免在应用关闭时阻塞
    if let Ok(mut manager_guard) = ARIA2_RPC_MANAGER.try_lock() {
        if let Some(mut rpc_manager) = manager_guard.take() {
            rpc_manager.shutdown();
        }
    } else {
        log_warn!("无法获取RPC管理器锁，跳过关闭RPC管理器");
    }

    // 清理所有运行中的aria2c进程 - 使用try_lock避免在应用关闭时阻塞
    if let Ok(mut pids_guard) = RUNNING_ARIA2_PIDS.try_lock() {
        for pid in pids_guard.drain() {
            log_info!("尝试终止aria2c进程: {}", pid);
            // 在Windows上，使用taskkill命令强制终止进程，隐藏窗口，并且不等待命令完成
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW 标志，隐藏命令行窗口
                .spawn(); // 使用spawn而不是output，避免阻塞清理过程
        }
    } else {
        log_warn!("无法获取进程ID列表锁，跳过强制终止aria2c进程");
    }

    log_info!("aria2c资源清理完成");
}

// 从嵌入式资源中释放aria2c.exe到统一的临时目录
//
// 这是一个内部辅助函数，仅在需要时被调用
fn release_aria2c_from_resource() -> Option<PathBuf> {
    // 嵌入aria2c.exe作为资源
    const ARIA2C_BYTES: &[u8] = include_bytes!("../bin/aria2c.exe");

    // 获取统一的临时目录
    let temp_dir = match crate::dir_manager::get_global_temp_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("无法获取临时目录: {:?}", err);
            return None;
        }
    };

    // 释放资源到临时文件
    let temp_path = temp_dir.join("aria2c.exe");
    if let Err(err) = fs::write(&temp_path, ARIA2C_BYTES) {
        eprintln!("无法写入aria2c.exe到临时目录: {:?}", err);
        return None;
    }

    Some(temp_path)
}

/// 获取aria2c可执行文件的路径
///
/// 该函数按以下优先级查找aria2c可执行文件:
/// 1. 首先检查统一临时目录中是否存在aria2c.exe
/// 2. 如果不存在，尝试从嵌入式资源中释放aria2c.exe到临时目录
/// 3. 如果上述方法失败，尝试使用相对路径bin/aria2c.exe
/// 4. 作为最后的回退，仅使用程序名"aria2c"（依赖PATH环境变量）
pub fn get_aria2c_path() -> PathBuf {
    // 尝试使用统一的临时目录
    if let Ok(temp_dir) = crate::dir_manager::get_global_temp_dir() {
        let aria2c_temp_path = temp_dir.join("aria2c.exe");

        if aria2c_temp_path.exists() {
            return aria2c_temp_path;
        }

        // 如果临时目录中不存在，则尝试释放嵌入式资源
        if let Some(path) = release_aria2c_from_resource() {
            return path;
        }
    } else {
        eprintln!("获取临时目录失败，使用回退方案");
    }

    // 作为最后的回退，尝试使用相对路径
    let mut path = env::current_dir().unwrap_or(PathBuf::from("."));
    path.push("bin");
    path.push("aria2c.exe");

    // 如果相对路径不存在，则回退到只使用程序名（依赖PATH环境变量）
    if !path.exists() {
        return PathBuf::from("aria2c");
    }

    path
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

        // 创建一个oneshot通道用于接收下载结果
        let (sender, receiver) = oneshot::channel();

        // 将任务添加到待处理队列
        PENDING_TASKS_MANAGER.add_task(PendingTask::Download {
            url: url.to_string(),
            app_handle: app_handle.clone(),
            task_id: task_id.to_string(),
            responder: sender,
        });

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

        // 等待下载结果
        match receiver.await {
            Ok(result) => result,
            Err(_) => Err("下载任务被取消或超时".to_string()),
        }
    } else {
        // aria2c已初始化完成，直接执行下载
        // 获取统一的临时目录
        let temp_dir = crate::dir_manager::get_global_temp_dir()?;
        log_debug!("[{}] 获取临时目录: {}", task_id, temp_dir.to_string_lossy());

        // 创建下载子目录
        let download_dir = temp_dir.join("downloads");
        fs::create_dir_all(&download_dir).map_err(|e| {
            log_error!("[{}] 创建下载目录失败: {}", task_id, e);
            format!("创建下载目录失败: {}", e)
        })?;
        log_info!(
            "[{}] 下载目录准备就绪: {}",
            task_id,
            download_dir.to_string_lossy()
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
        let file_path = download_dir.join(&filename);
        log_debug!(
            "[{}] 文件保存路径: {}",
            task_id,
            file_path.to_string_lossy()
        );

        // 获取下载目录的路径字符串
        let download_dir_str = download_dir
            .to_str()
            .ok_or_else(|| {
                log_error!("[{}] 无效的目录路径", task_id);
                "无效的目录路径".to_string()
            })?
            .to_string();

        // 转换为线程安全的路径字符串
        let url_owned = url.to_string();
        let file_path_clone = file_path.clone();
        let app_handle_for_events = app_handle.clone();
        let task_id_clone = task_id.to_string();
        let filename_clone = filename.clone();

        // 在后台线程中运行下载，避免阻塞主线程
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            log_info!("[{}] 启动后台下载线程", task_id_clone);

            // 获取全局RPC管理器
            let rpc_manager_mutex = match get_rpc_manager() {
                Ok(manager) => manager,
                Err(e) => {
                    log_error!("[{}] 获取RPC管理器失败: {}", task_id_clone, e);
                    let _ = tx.send(Err(e));
                    return;
                }
            };

            // 获取RPC管理器实例
            log_debug!("[{}] 获取RPC管理器实例", task_id_clone);
            let rpc_manager = rpc_manager_mutex.lock().unwrap();
            log_debug!("[{}] RPC管理器锁获取成功", task_id_clone);

            let manager = match rpc_manager.as_ref() {
                Some(mgr) => {
                    log_debug!("[{}] RPC管理器实例存在，URL: {}", task_id_clone, mgr.url);
                    mgr
                }
                None => {
                    log_error!("[{}] RPC管理器实例不存在", task_id_clone);
                    let _ = tx.send(Err("RPC管理器未初始化".to_string()));
                    return;
                }
            };

            // 添加下载任务到RPC服务器
            log_debug!("[{}] 准备添加下载任务到RPC服务器", task_id_clone);
            let gid = match manager.add_download(&url_owned, &download_dir_str, &filename_clone) {
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

            // 获取文件名（从URL中提取）
            let display_filename = url_owned
                .split('/')
                .last()
                .unwrap_or("未知文件")
                .to_string();

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
            let mut zero_speed_count = 0; // 记录下载速度为0的次数

            loop {
                // 检查应用是否正在关闭，如果是则中断下载
                if is_app_shutting_down() {
                    log_info!("[{}] 检测到应用正在关闭，中断下载任务", task_id_clone);
                    let _ = tx.send(Err("下载被取消：应用程序正在关闭".to_string()));
                    return;
                }

                std::thread::sleep(progress_interval);

                // 检查下载状态
                let status_result = rt.block_on(get_download_status(&manager, &gid));

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
                            zero_speed_count += 1;
                            log_warn!(
                                "[{}] 下载速度为0，计数: {}",
                                task_id_clone,
                                zero_speed_count
                            );

                            // 如果下载速度为0但进度不是100%，需要特别处理
                            if status.progress < 100.0 && zero_speed_count > 10 {
                                log_warn!(
                                    "[{}] 下载速度长时间为0，重新检查任务状态",
                                    task_id_clone
                                );

                                // 尝试获取任务列表，确认任务是否存在
                                let task_exists =
                                    rt.block_on(check_task_exists(&manager, &gid, &rt));
                                if !task_exists {
                                    log_warn!(
                                        "[{}] 任务可能已不存在，检查文件完整性",
                                        task_id_clone
                                    );

                                    // 检查文件是否存在且不为空
                                    if let Ok(metadata) = fs::metadata(&file_path_clone) {
                                        if metadata.len() > 0 {
                                            log_info!(
                                                "[{}] 任务文件存在且不为空，进行完整性检查",
                                                task_id_clone
                                            );

                                            // 等待文件大小稳定
                                            std::thread::sleep(Duration::from_secs(3));

                                            // 再次检查文件大小
                                            if let Ok(new_metadata) = fs::metadata(&file_path_clone)
                                            {
                                                if new_metadata.len() == metadata.len() {
                                                    log_info!(
                                                        "[{}] 文件大小稳定，确认下载完成",
                                                        task_id_clone
                                                    );
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                                zero_speed_count = 0; // 重置计数
                            }
                        } else {
                            zero_speed_count = 0; // 重置计数
                        }

                        if status.progress >= 100.0 {
                            // 进度显示100%，但需要额外检查确认下载真正完成
                            log_info!("[{}] 进度显示100%，进行最终确认检查", task_id_clone);

                            // 多次确认下载状态，确保真的完成
                            let mut confirmed_complete = false;
                            for _ in 0..3 {
                                std::thread::sleep(Duration::from_secs(1));
                                let final_status = rt.block_on(get_download_status(&manager, &gid));
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
                        let _rpc_manager_mutex = match get_rpc_manager() {
                            Ok(manager) => manager,
                            Err(e) => {
                                log_error!("[{}] 重新获取RPC管理器失败: {}", task_id_clone, e);
                                let _ = tx.send(Err(e));
                                return;
                            }
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

                        // 如果连续失败次数过多，尝试重新初始化RPC管理器
                        if consecutive_failures % 3 == 0 {
                            log_info!(
                                "[{}] 多次获取状态失败，尝试重新初始化RPC管理器",
                                task_id_clone
                            );

                            // 强制重新初始化RPC管理器
                            if let Ok(mut manager) = ARIA2_RPC_MANAGER.lock() {
                                *manager = None;
                                log_info!("[{}] RPC管理器已重置，尝试重新获取", task_id_clone);
                            }
                        }

                        // 定期发送状态更新，告知前端下载仍在进行中
                        if consecutive_failures % 2 == 0 {
                            let progress_json = serde_json::json!({
                                "progress": last_progress.max(0.0),
                                "filename": display_filename.clone(),
                                "taskId": task_id_clone.clone(),
                                "message": format!("状态查询暂时不可用 ({}次重试)", consecutive_failures)
                            });
                            _ = app_handle_for_events.emit_to(
                                "main",
                                "download-progress",
                                &progress_json,
                            );
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
                                    log_info!("[{}] 虽然状态查询失败，但文件已存在且大小为 {} 字节，尝试继续处理", 
                                         task_id_clone, metadata.len());
                                    break;
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

                for _i in 0..5 {
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
