// 目录管理器模块 - 负责管理临时目录和解压目录，以及 Steam 和游戏目录的查找

// 标准库导入
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

// 第三方库导入
extern crate lazy_static;
use lazy_static::lazy_static;
use regex::Regex;
use winreg::{enums::*, RegKey};

// 内部模块导入
use crate::{log_error, log_info, log_warn};

// 全局目录管理器实例，使用 Arc<Mutex<>> 确保线程安全
lazy_static! {
    pub static ref DIR_MANAGER: Arc<Mutex<Option<DirManager>>> = Arc::new(Mutex::new(None));
}

// 目录管理器，负责创建和管理临时目录和 L4D2 addons 目录
// 实现 Drop trait，在程序退出时自动清理临时目录
pub struct DirManager {
    addons_dir: Option<PathBuf>,
    downloads_dir: PathBuf,
    bin_dir: PathBuf,
    maps_dir: PathBuf,
}

impl DirManager {
    /// 创建一个新的目录管理器实例
    ///
    /// 返回包含临时目录的 DirManager 实例
    ///
    /// 临时目录以 "nmd_" 开头并附加随机 UUID
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            addons_dir: None,
            downloads_dir: PathBuf::new(),
            bin_dir: PathBuf::new(),
            maps_dir: PathBuf::new(),
        })
    }

    /// 使用指定的 nmd_data 目录创建目录管理器实例
    ///
    /// # 参数
    /// - `nmd_data_dir`: nmd_data 目录路径
    ///
    /// # 返回值
    /// - 成功时返回 DirManager 实例
    /// - 失败时返回包含错误信息的 Err
    pub fn with_nmd_data_dir(nmd_data_dir: PathBuf) -> Result<Self, String> {
        // 确保 nmd_data 目录存在
        std::fs::create_dir_all(&nmd_data_dir)
            .map_err(|e| format!("无法创建 nmd_data 目录: {:?}", e))?;

        // 创建 nmd_data/bin 目录
        let bin_dir = nmd_data_dir.join("bin");
        std::fs::create_dir_all(&bin_dir).map_err(|e| format!("无法创建 bin 目录: {:?}", e))?;

        // 创建 nmd_data/bin/cache 目录（作为下载目录）
        let downloads_dir = bin_dir.join("cache");
        std::fs::create_dir_all(&downloads_dir)
            .map_err(|e| format!("无法创建 bin/cache 目录: {:?}", e))?;

        // 创建 nmd_data/maps 目录
        let maps_dir = nmd_data_dir.join("maps");
        std::fs::create_dir_all(&maps_dir).map_err(|e| format!("无法创建 maps 目录: {:?}", e))?;

        Ok(Self {
            addons_dir: None,
            downloads_dir,
            bin_dir,
            maps_dir,
        })
    }

    /// 获取二进制文件目录路径
    pub fn bin_dir(&self) -> PathBuf {
        self.bin_dir.to_path_buf()
    }

    /// 获取下载目录路径
    pub fn downloads_dir(&self) -> PathBuf {
        self.downloads_dir.to_path_buf()
    }

    /// 设置 L4D2 addons 目录
    pub fn set_addons_dir(&mut self, addons_dir: PathBuf) {
        self.addons_dir = Some(addons_dir);
    }

    /// 获取 L4D2 addons 目录路径（如果已设置）
    pub fn addons_dir(&self) -> Option<&PathBuf> {
        self.addons_dir.as_ref()
    }

    /// 获取 maps 目录路径
    pub fn maps_dir(&self) -> PathBuf {
        self.maps_dir.to_path_buf()
    }
}

/// 获取全局下载目录路径
///
/// 如果全局目录管理器尚未初始化，则会自动初始化
pub fn get_global_downloads_dir() -> Result<PathBuf, String> {
    let mut manager = DIR_MANAGER
        .lock()
        .map_err(|e| format!("无法锁定目录管理器: {:?}", e))?;

    // 如果还没有初始化目录管理器，先初始化
    if manager.is_none() {
        *manager = Some(DirManager::new()?);
    }

    // 返回下载目录路径的副本
    Ok(manager.as_ref().unwrap().downloads_dir().to_path_buf())
}

/// 获取全局二进制目录路径
///
/// 如果全局目录管理器尚未初始化，则会自动初始化
pub fn get_global_bin_dir() -> Result<PathBuf, String> {
    let mut manager = DIR_MANAGER
        .lock()
        .map_err(|e| format!("无法锁定目录管理器: {:?}", e))?;

    // 如果还没有初始化目录管理器，先初始化
    if manager.is_none() {
        *manager = Some(DirManager::new()?);
    }

    // 返回二进制目录路径的副本
    Ok(manager.as_ref().unwrap().bin_dir().to_path_buf())
}

/// 设置全局 L4D2 addons 目录
///
/// 如果全局目录管理器尚未初始化，则会自动初始化
pub fn set_global_addons_dir(addons_dir: &str) -> Result<(), String> {
    let mut manager = DIR_MANAGER
        .lock()
        .map_err(|e| format!("无法锁定目录管理器: {:?}", e))?;

    // 如果还没有初始化目录管理器，先初始化
    if manager.is_none() {
        *manager = Some(DirManager::new()?);
    }

    // 设置 L4D2 addons 目录
    manager
        .as_mut()
        .unwrap()
        .set_addons_dir(PathBuf::from(addons_dir));

    Ok(())
}

// ========== Steam相关功能 ==========

/// 从 Windows 注册表获取 Steam 安装路径
pub fn get_steam_install_path() -> Result<String, String> {
    // 打开注册表项
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let steam_key = hklm
        .open_subkey("SOFTWARE\\WOW6432Node\\Valve\\Steam")
        .map_err(|e| format!("无法打开 Steam 注册表项, 请检查 Steam 是否已安装:\n{:?}", e))?;

    // 读取 InstallPath 值
    let install_path: String = steam_key.get_value("InstallPath").map_err(|e| {
        format!(
            "无法读取 InstallPath 值, 请检查 Steam 是否正确安装:\n{:?}",
            e
        )
    })?;

    Ok(install_path)
}

/// 解析 libraryfolders.vdf 文件获取所有 Steam 库路径
///
/// 返回包含所有 Steam 库路径的向量，包括主 Steam 目录
pub fn parse_library_folders(steam_path: &str) -> Result<Vec<String>, String> {
    let vdf_path = PathBuf::from(steam_path)
        .join("steamapps")
        .join("libraryfolders.vdf");

    if !vdf_path.exists() {
        return Err(format!(
            "libraryfolders.vdf 文件不存在:\n{}",
            vdf_path.display()
        ));
    }

    let vdf_content = fs::read_to_string(&vdf_path)
        .map_err(|e| format!("无法读取 libraryfolders.vdf 文件:\n{:?}", e))?;

    let mut library_paths = Vec::new();
    library_paths.push(steam_path.to_string()); // 添加主Steam目录

    // 解析vdf文件，查找所有path键值对
    for line in vdf_content.lines() {
        let line_trimmed = line.trim();
        if line_trimmed.starts_with('"') && line_trimmed.contains("\"path\"") {
            // 提取path值
            if let Some(start) = line_trimmed.find("\"path\"") {
                let remaining = &line_trimmed[start + 6..]; // 跳过 "path"
                if let Some(value_start) = remaining.find('"') {
                    let value_part = &remaining[value_start + 1..];
                    if let Some(value_end) = value_part.find('"') {
                        let path = value_part[..value_end].to_string();
                        // 处理路径中的转义字符
                        let path = path.replace("\\\\", "\\");
                        library_paths.push(path);
                    }
                }
            }
        }
    }

    Ok(library_paths)
}

/// 解析appmanifest文件获取游戏安装目录
pub fn parse_appmanifest(manifest_path: &PathBuf) -> Result<String, String> {
    if !manifest_path.exists() {
        return Err(format!(
            "appmanifest 文件不存在:\n{}",
            manifest_path.display()
        ));
    }

    let manifest_content = fs::read_to_string(manifest_path)
        .map_err(|e| format!("无法读取 appmanifest 文件:\n{:?}", e))?;

    // 首先尝试使用正则表达式查找installdir值
    let re = Regex::new(r#"installdir"\s+"([^"]+)"#).unwrap();
    if let Some(captures) = re.captures(&manifest_content) {
        let installdir = captures.get(1).unwrap().as_str().to_string();
        return Ok(installdir);
    }

    // 如果正则表达式方法失败，尝试简单的字符串搜索方法
    for line in manifest_content.lines() {
        let line_lower = line.to_lowercase();
        if line_lower.contains("installdir") {
            // 尝试提取引号中的内容
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() >= 3 {
                let value = parts[2].trim();
                if !value.is_empty() {
                    return Ok(value.to_string());
                }
            }
        }
    }

    Err("在 appmanifest 文件中未找到 installdir 值".to_string())
}

/// 获取 Left 4 Dead 2 的 addons 目录路径
///
/// 该函数通过以下步骤查找游戏目录：
/// 1. 从注册表获取 Steam 安装路径
/// 2. 解析 libraryfolders.vdf 获取所有 Steam 库路径
/// 3. 遍历所有库路径，查找 Left 4 Dead 2 的 appmanifest 文件
/// 4. 解析 appmanifest 获取游戏安装目录
/// 5. 构建并验证 addons 目录路径
pub fn get_l4d2_addons_dir() -> Result<String, String> {
    log_info!("开始查找 Left 4 Dead 2 游戏目录...");

    // 从注册表获取Steam安装路径
    log_info!("从注册表获取 Steam 安装路径...");
    let steam_path = get_steam_install_path()?;
    log_info!("Steam 安装路径: {}", steam_path);

    // 解析libraryfolders.vdf获取所有Steam库路径
    log_info!("解析 libraryfolders.vdf 获取所有 Steam 库路径...");
    let library_paths = parse_library_folders(&steam_path)?;
    log_info!("找到 {} 个 Steam 库路径", library_paths.len());

    // 游戏ID 550是Left 4 Dead 2
    const L4D2_APP_ID: &str = "550";

    // 遍历所有库路径，查找appmanifest_550.acf文件
    for (index, path) in library_paths.iter().enumerate() {
        log_info!("检查第 {} 个库路径: {}", index + 1, path);
        let manifest_path = PathBuf::from(&path)
            .join("steamapps")
            .join(format!("appmanifest_{}.acf", L4D2_APP_ID));

        log_info!("检查 manifest 文件: {}", manifest_path.display());
        if manifest_path.exists() {
            log_info!("找到 appmanifest_550.acf，开始解析...");
            // 解析appmanifest文件获取游戏安装目录
            if let Ok(installdir) = parse_appmanifest(&manifest_path) {
                log_info!("游戏安装目录: {}", installdir);
                // 构建addons目录路径
                let addons_dir = PathBuf::from(&path)
                    .join("steamapps")
                    .join("common")
                    .join(&installdir)
                    .join("left4dead2")
                    .join("addons");

                log_info!("检查 addons 目录: {}", addons_dir.display());
                if addons_dir.exists() {
                    log_info!("找到 Left 4 Dead 2 addons 目录: {}", addons_dir.display());
                    return Ok(addons_dir.to_string_lossy().to_string());
                } else {
                    log_warn!("addons 目录不存在: {}", addons_dir.display());
                }
            } else {
                log_warn!("解析 appmanifest 文件失败");
            }
            // 解析失败时继续检查下一个库路径
        } else {
            log_info!("appmanifest_550.acf 不存在");
        }
    }

    let error_msg =
        "未找到 Left 4 Dead 2 游戏目录，请确认你已经在 Steam 中安装了 Left 4 Dead 2 游戏"
            .to_string();
    log_error!("{}", error_msg);
    Err(error_msg)
}
