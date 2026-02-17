// 在 debug 模式下使用 console 子系统，方便调试
#![cfg_attr(debug_assertions, windows_subsystem = "console")]
// 在 release 模式下使用 windows 子系统，不显示控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    env, fs,
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
    process,
};

// Windows API绑定
#[cfg(windows)]
mod windows {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::shellapi::ShellExecuteW;
    use winapi::um::winuser::SW_NORMAL;

    /// 以管理员权限重启当前进程
    pub fn run_as_admin(args: &[String]) -> Result<(), String> {
        let executable =
            std::env::current_exe().map_err(|e| format!("获取当前可执行文件路径失败: {:?}", e))?;
        let executable_str = executable.to_string_lossy().to_string();

        // 为每个参数添加引号，处理带空格的参数
        let quoted_args: Vec<String> = args
            .iter()
            .map(|arg| {
                if arg.contains(' ') {
                    format!("{}", arg)
                } else {
                    arg.to_string()
                }
            })
            .collect();
        let args_str = quoted_args.join(" ");

        println!("以管理员权限重启: {} {}", executable_str, args_str);

        let executable_w = OsStr::new(&executable_str)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();

        let command_line_w = OsStr::new(&args_str)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();

        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                OsStr::new("runas")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect::<Vec<_>>()
                    .as_ptr(),
                executable_w.as_ptr(),
                command_line_w.as_ptr(),
                std::ptr::null_mut(),
                SW_NORMAL,
            )
        };

        if result as usize > 32 {
            Ok(())
        } else {
            Err(format!("请求管理员权限失败，错误码: {:?}", result))
        }
    }
}

/// 符号链接操作类型

/// 消息结构
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SymlinkMessage {
    pub create: Option<CreateArgs>,
}

/// 创建命令参数结构
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CreateArgs {
    pub target: String,
    pub dir: String,
    pub name: String,
}

/// 响应结构
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SymlinkResponse {
    pub success: bool,
    pub message: String,
}

/// 创建文件符号链接
fn create_file_symlink(
    target_path: &str,
    link_path: &str,
    link_name: &str,
) -> Result<String, String> {
    let target = Path::new(target_path);

    if !target.exists() {
        return Err(format!("目标文件不存在: {}", target_path));
    }

    if !target.is_file() {
        return Err(format!("目标路径不是文件: {}", target_path));
    }

    let link_dir_path = Path::new(link_path);

    if !link_dir_path.exists() {
        return Err(format!("链接目录不存在: {}", link_path));
    }

    if !link_dir_path.is_dir() {
        return Err(format!("链接路径不是目录: {}", link_path));
    }

    let link_path = link_dir_path.join(link_name);

    if link_path.exists() {
        return Err(format!("链接路径已存在: {}", link_path.display()));
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::symlink_file;
        symlink_file(target_path, &link_path).map_err(|e| format!("创建符号链接失败: {:?}", e))?;
    }

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::symlink;
        symlink(target_path, &link_path).map_err(|e| format!("创建符号链接失败: {:?}", e))?;
    }

    println!("文件符号链接创建成功: {}", link_path.display());
    Ok(format!("符号链接创建成功: {}", link_path.display()))
}

/// 处理单个客户端连接
fn handle_client(mut stream: std::net::TcpStream, server_token: &str) {
    // 不使用split，直接使用同一个流
    loop {
        // 读取一行消息
        let mut message = String::new();
        {
            let mut reader = BufReader::new(&mut stream);
            match reader.read_line(&mut message) {
                Ok(0) => {
                    // 连接关闭
                    println!("客户端连接关闭");
                    // 退出当前线程，不退出整个进程
                    return;
                }
                Ok(_) => {
                    // 解析消息
                    message = message.trim().to_string();
                    println!("收到消息: {}", message);
                }
                Err(e) => {
                    println!("读取消息失败: {:?}", e);
                    // 连接错误，退出当前线程
                    return;
                }
            }
        }

        // Token验证
        if !server_token.is_empty() {
            // 尝试解析消息为JSON
            match serde_json::from_str::<serde_json::Value>(&message) {
                Ok(msg) => {
                    // 提取token
                    match msg.get("token").and_then(|t| t.as_str()) {
                        Some(token) => {
                            // 验证token
                            if token != server_token {
                                println!("Token验证失败");
                                let response = SymlinkResponse {
                                    success: false,
                                    message: "Token验证失败".to_string(),
                                };
                                let json_response = serde_json::to_string(&response).unwrap();
                                let mut writer = BufWriter::new(&mut stream);
                                writeln!(writer, "{}", json_response).unwrap();
                                writer.flush().unwrap();
                                continue;
                            }
                        }
                        None => {
                            println!("缺少Token");
                            let response = SymlinkResponse {
                                success: false,
                                message: "缺少Token".to_string(),
                            };
                            let json_response = serde_json::to_string(&response).unwrap();
                            let mut writer = BufWriter::new(&mut stream);
                            writeln!(writer, "{}", json_response).unwrap();
                            writer.flush().unwrap();
                            continue;
                        }
                    }
                }
                Err(_) => {
                    println!("消息格式错误，无法解析Token");
                    let response = SymlinkResponse {
                        success: false,
                        message: "消息格式错误，无法解析Token".to_string(),
                    };
                    let json_response = serde_json::to_string(&response).unwrap();
                    let mut writer = BufWriter::new(&mut stream);
                    writeln!(writer, "{}", json_response).unwrap();
                    writer.flush().unwrap();
                    continue;
                }
            }
        }

        let response = match serde_json::from_str::<serde_json::Value>(&message) {
            Ok(msg) => {
                if let Some(cmd) = msg.get("cmd").and_then(|c| c.as_str()) {
                    match cmd {
                        "create" => {
                            // create命令需要args参数
                            if let Some(args) = msg.get("args").and_then(|a| a.as_object()) {
                                if let (Some(target), Some(path), Some(name)) = (
                                    args.get("target").and_then(|t| t.as_str()),
                                    args.get("path").and_then(|p| p.as_str()),
                                    args.get("name").and_then(|n| n.as_str()),
                                ) {
                                    match create_file_symlink(target, path, name) {
                                        Ok(message) => SymlinkResponse {
                                            success: true,
                                            message,
                                        },
                                        Err(error) => SymlinkResponse {
                                            success: false,
                                            message: error,
                                        },
                                    }
                                } else {
                                    SymlinkResponse {
                                        success: false,
                                        message: "缺少必要参数".to_string(),
                                    }
                                }
                            } else {
                                SymlinkResponse {
                                    success: false,
                                    message: "缺少args参数".to_string(),
                                }
                            }
                        }
                        // 可以在这里添加其他不需要args的命令
                        _ => SymlinkResponse {
                            success: false,
                            message: "未知命令".to_string(),
                        },
                    }
                } else {
                    SymlinkResponse {
                        success: false,
                        message: "缺少cmd参数".to_string(),
                    }
                }
            }
            Err(e) => SymlinkResponse {
                success: false,
                message: format!("解析消息失败: {:?}", e),
            },
        };

        // 发送响应
        {
            let mut writer = BufWriter::new(&mut stream);
            let json_response = serde_json::to_string(&response).unwrap();
            writeln!(writer, "{}", json_response).unwrap();
            writer.flush().unwrap();
        }
    }
}

/// 启动服务器模式
fn start_server() {
    println!("启动符号链接服务器...");

    // 从命令行参数中读取端口号和token，支持 --port/-p 和 --token/-t 参数
    let mut port = 0; // 默认随机端口
    let mut token = String::new(); // 默认空token
    let args: Vec<String> = std::env::args().collect();

    for i in 1..args.len() {
        if (args[i] == "--port" || args[i] == "-p") && i + 1 < args.len() {
            if let Ok(p) = args[i + 1].parse::<u16>() {
                port = p;
            }
        } else if (args[i] == "--token" || args[i] == "-t") && i + 1 < args.len() {
            token = args[i + 1].clone();
        }
    }

    // 创建TCP服务器
    let listener = match std::net::TcpListener::bind(format!("127.0.0.1:{}", port)) {
        Ok(listener) => listener,
        Err(e) => {
            println!("启动服务器失败: {:?}", e);
            return;
        }
    };

    println!("服务器已启动，监听端口 {}", port);
    if !token.is_empty() {
        println!("Token验证已启用");
    }

    // 跟踪连接数
    let connection_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let shutdown_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_token = std::sync::Arc::new(token);

    // 启动监控线程
    {
        let connection_count = connection_count.clone();
        let shutdown_flag = shutdown_flag.clone();
        std::thread::spawn(move || {
            loop {
                // 检查是否有连接
                if connection_count.load(std::sync::atomic::Ordering::Relaxed) == 0 {
                    // 如果没有连接，等待一段时间后退出
                    println!("没有客户端连接，准备退出...");
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if connection_count.load(std::sync::atomic::Ordering::Relaxed) == 0 {
                        println!("确认没有客户端连接，退出服务器");
                        shutdown_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                        std::process::exit(0);
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        });
    }

    // 接受客户端连接
    for stream in listener.incoming() {
        if shutdown_flag.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        match stream {
            Ok(stream) => {
                // 增加连接数
                let count = connection_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                println!("接受新连接，当前连接数: {}", count);

                // 为客户端创建一个线程
                let connection_count = connection_count.clone();
                let server_token = server_token.clone();
                std::thread::spawn(move || {
                    handle_client(stream, &server_token);
                    // 连接关闭时减少连接数
                    let count = connection_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    println!("客户端连接已关闭，当前连接数: {}", count - 1);
                });
            }
            Err(e) => {
                println!("接受连接失败: {:?}", e);
            }
        }
    }
}

/// 测试符号链接权限
fn test_symlink_permission() -> Result<String, String> {
    // 创建临时测试文件
    let test_dir = env::temp_dir();
    let test_file = test_dir.join("nmd_test_SeCreateSymbolicLinkPrivilege.tmp");
    let link_path = test_dir.join("nmd_link_SeCreateSymbolicLinkPrivilege.tmp");

    // 写入测试内容
    fs::write(&test_file, "SeCreateSymbolicLinkPrivilege")
        .map_err(|e| format!("创建测试文件失败: {:?}", e))?;

    // 尝试创建符号链接
    #[cfg(windows)]
    let result = {
        use std::os::windows::fs::symlink_file;
        symlink_file(&test_file, &link_path)
    };

    #[cfg(not(windows))]
    let result = {
        use std::os::unix::fs::symlink;
        symlink(&test_file, &link_path)
    };

    // 清理测试文件
    let _ = fs::remove_file(&test_file);
    let _ = fs::remove_file(&link_path);

    result.map_err(|e| format!("创建测试符号链接失败: {:?}", e))?;
    Ok("符号链接权限测试成功".to_string())
}

fn main() {
    // 启动时自动测试符号链接权限
    println!("测试符号链接权限...");
    match test_symlink_permission() {
        Err(error) => {
            println!("权限测试失败: {}", error);
            // 尝试以管理员权限重启自身
            println!("尝试以管理员权限重启...");

            // 获取当前命令行参数
            let args: Vec<String> = std::env::args().collect();
            // 跳过第一个参数（可执行文件路径），只传递后续参数
            let restart_args: Vec<String> = args.iter().skip(1).cloned().collect();

            #[cfg(windows)]
            {
                if let Err(e) = windows::run_as_admin(&restart_args) {
                    println!("以管理员权限重启失败: {}", e);
                    process::exit(5);
                } else {
                    // 重启请求已发送，退出当前进程
                    println!("已请求管理员权限重启，退出当前进程");
                    process::exit(0);
                }
            }

            #[cfg(not(windows))]
            {
                println!("非Windows平台，无法自动提权重启");
                process::exit(5);
            }
        }
        Ok(message) => {
            println!("权限测试成功: {}", message);
        }
    }

    // 直接启动服务器模式
    start_server();
}
