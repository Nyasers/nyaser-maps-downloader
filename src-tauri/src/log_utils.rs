// 日志工具模块
// 此模块包含所有日志相关的辅助函数和宏定义
#[cfg(debug_assertions)]
use chrono::DateTime;
#[cfg(debug_assertions)]
use std::{
    io::BufRead,
    time::{Duration, SystemTime},
};

/// 辅助函数：获取当前时间的格式化字符串
#[cfg(debug_assertions)]
pub fn get_current_time() -> String {
    let now = SystemTime::now();
    let since_epoch = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0));
    let time = DateTime::from_timestamp(since_epoch.as_secs() as i64, 0)
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());
    time.format("%Y-%m-%d %H:%M:%S.%3f").to_string()
}

/// 记录日志的辅助函数
pub fn log_message(level: &str, message: &str) {
    #[cfg(debug_assertions)]
    {
        let timestamp = get_current_time();
        println!("[{}] [{}] {}", timestamp, level, message);
    }
    #[cfg(not(debug_assertions))]
    {
        _ = (level, message);
    }
}

/// 日志宏定义 - 信息级别
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::log_utils::log_message("INFO", &format!($($arg)*)); };
}

/// 日志宏定义 - 警告级别
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::log_utils::log_message("WARN", &format!($($arg)*)) };
}

/// 日志宏定义 - 错误级别
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { $crate::log_utils::log_message("ERROR", &format!($($arg)*)) };
}

/// 日志宏定义 - 调试级别
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => { $crate::log_utils::log_message("DEBUG", &format!($($arg)*)) };
}

/// 重定向子进程的标准输出和标准错误到日志系统
///
/// # 参数
/// - `stdout`: 子进程的标准输出流
/// - `stderr`: 子进程的标准错误流
/// - `prefix`: 用于日志输出的前缀字符串，通常包含进程标识或任务ID
///
/// # 功能说明
/// 该函数为stdout和stderr分别创建独立的线程，持续读取输出并通过相应级别的日志函数记录。
/// 这样可以确保子进程的输出被正确捕获并格式化显示，而不会阻塞主程序的执行。
pub fn redirect_process_output(
    stdout: std::process::ChildStdout,
    stderr: std::process::ChildStderr,
    prefix: String,
) {
    // 创建一个副本用于stdout线程
    let prefix_stdout = prefix.clone();

    // 为stdout创建一个线程，使用log_info级别记录输出
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        loop {
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF，流已关闭
                Ok(_) => {
                    if !line.trim().is_empty() {
                        log_info!("{} [STDOUT]: {}", prefix_stdout, line.trim());
                    }
                    line.clear();
                }
                Err(e) => {
                    log_error!("读取stdout失败: {}", e);
                    break;
                }
            }
        }
    });

    // 为stderr创建一个线程，使用log_error级别记录输出
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stderr);
        let mut line = String::new();
        loop {
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF，流已关闭
                Ok(_) => {
                    if !line.trim().is_empty() {
                        log_error!("{} [STDERR]: {}", prefix, line.trim());
                    }
                    line.clear();
                }
                Err(e) => {
                    log_error!("读取stderr失败: {}", e);
                    break;
                }
            }
        }
    });
}
