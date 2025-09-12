// 目录管理器模块 - 负责管理临时目录和解压目录，以及Steam和游戏目录的查找

// 标准库导入
use std::{
    env::temp_dir,
    fs,
    ops::Drop,
    path::PathBuf,
    sync::{Arc, Mutex},
};

// 第三方库导入
extern crate lazy_static;
use lazy_static::lazy_static;
use regex::Regex;
use uuid::Uuid;
use winreg::{enums::*, RegKey};

// 全局目录管理器实例，使用Arc<Mutex<>>确保线程安全
lazy_static! {
    pub static ref DIR_MANAGER: Arc<Mutex<Option<DirManager>>> = Arc::new(Mutex::new(None));
}

// 目录管理器，负责创建和管理临时目录和解压目录
// 实现Drop trait在程序退出时自动清理临时目录
pub struct DirManager {
    temp_dir: PathBuf,
    extract_dir: Option<PathBuf>,
}

impl DirManager {
    /// 创建一个新的目录管理器实例
    ///
    /// 返回包含临时目录的DirManager实例，临时目录以"nmd_"开头并附加随机UUID
    pub fn new() -> Result<Self, String> {
        // 获取系统临时目录
        let sys_temp_dir = temp_dir();

        // 创建以"nmd"开头的临时目录（Nyaser Maps Downloader）
        let temp_dir = sys_temp_dir.join(format!("nmd_{}", Uuid::new_v4().simple()));

        // 确保临时目录存在，创建失败时返回错误
        std::fs::create_dir_all(&temp_dir).map_err(|e| format!("无法创建临时目录: {:?}", e))?;

        Ok(Self {
            temp_dir,
            extract_dir: None,
        })
    }

    /// 获取临时目录路径
    pub fn temp_dir(&self) -> &PathBuf {
        &self.temp_dir
    }

    /// 设置解压目录
    pub fn set_extract_dir(&mut self, extract_dir: PathBuf) {
        self.extract_dir = Some(extract_dir);
    }

    /// 获取解压目录路径（如果已设置）
    pub fn extract_dir(&self) -> Option<&PathBuf> {
        self.extract_dir.as_ref()
    }
}

// 实现Drop trait，在结构体被销毁时自动清理临时目录
impl Drop for DirManager {
    fn drop(&mut self) {
        // 尝试删除临时目录及其所有内容
        if let Err(e) = std::fs::remove_dir_all(&self.temp_dir) {
            eprintln!("无法删除临时目录 {}: {:?}", self.temp_dir.display(), e);
        }
    }
}

/// 获取全局临时目录路径
///
/// 如果全局目录管理器尚未初始化，则会自动初始化
pub fn get_global_temp_dir() -> Result<PathBuf, String> {
    let mut manager = DIR_MANAGER
        .lock()
        .map_err(|e| format!("无法锁定目录管理器: {:?}", e))?;

    // 如果还没有初始化目录管理器，先初始化
    if manager.is_none() {
        *manager = Some(DirManager::new()?);
    }

    // 返回临时目录路径的副本
    Ok(manager.as_ref().unwrap().temp_dir().to_path_buf())
}

/// 设置全局解压目录
///
/// 如果全局目录管理器尚未初始化，则会自动初始化
pub fn set_global_extract_dir(extract_dir: &str) -> Result<(), String> {
    let mut manager = DIR_MANAGER
        .lock()
        .map_err(|e| format!("无法锁定目录管理器: {:?}", e))?;

    // 如果还没有初始化目录管理器，先初始化
    if manager.is_none() {
        *manager = Some(DirManager::new()?);
    }

    // 设置解压目录
    manager
        .as_mut()
        .unwrap()
        .set_extract_dir(PathBuf::from(extract_dir));

    Ok(())
}

/// 显式清理全局临时目录
///
/// 重置目录管理器，触发Drop trait的执行以清理临时目录
pub fn cleanup_temp_dir() {
    if let Ok(mut manager) = DIR_MANAGER.lock() {
        // 重置管理器，触发Drop trait的执行
        *manager = None;
    }
}

// ========== Steam相关功能 ==========

/// 从Windows注册表获取Steam安装路径
pub fn get_steam_install_path() -> Result<String, String> {
    // 打开注册表项
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let steam_key = hklm
        .open_subkey("SOFTWARE\\WOW6432Node\\Valve\\Steam")
        .map_err(|e| format!("无法打开Steam注册表项: {:?}", e))?;

    // 读取InstallPath值
    let install_path: String = steam_key
        .get_value("InstallPath")
        .map_err(|e| format!("无法读取InstallPath值: {:?}", e))?;

    Ok(install_path)
}

/// 解析libraryfolders.vdf文件获取所有Steam库路径
///
/// 返回包含所有Steam库路径的向量，包括主Steam目录
pub fn parse_library_folders(steam_path: &str) -> Result<Vec<String>, String> {
    let vdf_path = PathBuf::from(steam_path)
        .join("steamapps")
        .join("libraryfolders.vdf");

    if !vdf_path.exists() {
        return Err(format!(
            "libraryfolders.vdf文件不存在: {}",
            vdf_path.display()
        ));
    }

    let vdf_content = fs::read_to_string(&vdf_path)
        .map_err(|e| format!("无法读取libraryfolders.vdf文件: {:?}", e))?;

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
            "appmanifest文件不存在: {}",
            manifest_path.display()
        ));
    }

    let manifest_content = fs::read_to_string(manifest_path)
        .map_err(|e| format!("无法读取appmanifest文件: {:?}", e))?;

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

    Err("在appmanifest文件中未找到installdir值".to_string())
}

/// 获取Left 4 Dead 2的addons目录路径
///
/// 该函数通过以下步骤查找游戏目录：
/// 1. 从注册表获取Steam安装路径
/// 2. 解析libraryfolders.vdf获取所有Steam库路径
/// 3. 遍历所有库路径，查找Left 4 Dead 2的appmanifest文件
/// 4. 解析appmanifest获取游戏安装目录
/// 5. 构建并验证addons目录路径
pub fn get_l4d2_addons_dir() -> Result<String, String> {
    // 从注册表获取Steam安装路径
    let steam_path = get_steam_install_path()?;

    // 解析libraryfolders.vdf获取所有Steam库路径
    let library_paths = parse_library_folders(&steam_path)?;

    // 游戏ID 550是Left 4 Dead 2
    const L4D2_APP_ID: &str = "550";

    // 遍历所有库路径，查找appmanifest_550.acf文件
    for path in library_paths {
        let manifest_path = PathBuf::from(&path)
            .join("steamapps")
            .join(format!("appmanifest_{}.acf", L4D2_APP_ID));

        if manifest_path.exists() {
            // 解析appmanifest文件获取游戏安装目录
            if let Ok(installdir) = parse_appmanifest(&manifest_path) {
                // 构建addons目录路径
                let addons_dir = PathBuf::from(&path)
                    .join("steamapps")
                    .join("common")
                    .join(&installdir)
                    .join("left4dead2")
                    .join("addons");

                if addons_dir.exists() {
                    return Ok(addons_dir.to_string_lossy().to_string());
                }
            }
            // 解析失败时继续检查下一个库路径
        }
    }

    Err(
        "未找到 Left 4 Dead 2 游戏目录，请确认你已经在 Steam 中安装了 Left 4 Dead 2 游戏"
            .to_string(),
    )
}
