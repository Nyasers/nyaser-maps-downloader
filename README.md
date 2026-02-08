# Nyaser Maps Downloader

Nyaser Maps Downloader 是一个专为从 [maps.nyase.ru](https://maps.nyase.ru) 网站下载和管理地图文件而设计的桌面应用程序。该应用基于 Tauri 框架构建，提供高效、可靠的地图下载体验。

## 项目简介

Nyaser Maps Downloader 简化了从 maps.nyase.ru 网站获取地图资源的过程，通过智能拦截下载请求和高效的下载队列管理，为用户提供流畅的地图获取体验。

## 主要功能

- **一键下载**：通过 HTML 注入技术，自动拦截并处理地图下载请求，简化下载流程
- **高效下载管理**：使用 aria2c 作为下载后端，支持队列管理和并行下载，确保最佳下载速度
- **下载队列持久化**：支持下载队列的保存与恢复，应用重启后自动恢复未完成的下载任务
- **下载任务控制**：支持单个下载任务取消、批量取消、刷新队列等操作，提供实时下载状态显示
- **自动解压**：下载完成后自动解压地图文件到指定目录，支持多种压缩格式
- **解压队列管理**：支持解压任务的队列管理，确保解压任务按顺序执行
- **拖拽解压**：支持将压缩包文件拖拽到应用中进行解压操作
- **子文件夹创建**：解压时自动创建以压缩包名称命名的子文件夹，保持文件组织结构
- **解压后自动挂载**：解压完成后自动将地图文件挂载到游戏目录
- **文件管理器**：内置文件管理器，支持分组显示、批量操作、挂载/卸载、删除等功能
- **文件挂载系统**：支持将下载的地图文件挂载到游戏目录，方便游戏识别
- **服务器列表窗口**：集成服务器列表访问功能，方便用户快速查找和加入游戏服务器
- **配置管理**：支持用户自定义配置，灵活管理应用设置和数据存储目录
- **符号链接管理**：支持扫描和管理文件符号链接，方便管理游戏地图文件
- **清理无效链接**：支持清理无效的符号链接，保持文件系统整洁
- **文件分类功能**：支持对地图文件进行分类管理，优化文件组织
- **分类和挂载状态筛选**：支持按分类和挂载状态筛选文件，提高文件查找效率
- **分组搜索功能**：支持搜索文件分组，快速定位特定分组
- **模糊搜索功能**：集成 Fuse.js 实现智能模糊搜索，支持按文件名进行模糊匹配
- **依赖复制工具**：支持复制应用依赖文件，方便应用部署和迁移
- **Deep Link 支持**：支持 nmd:// 协议，可通过外部链接直接启动应用并执行特定操作
- **自动更新**：集成 Tauri updater 插件，支持应用自动更新和版本检查，更新前自动清理资源
- **智能文件名提取**：支持从百度 PCS 链接等特殊格式中提取文件名
- **百度网盘支持**：支持百度网盘下载链接的本地代理处理
- **多窗口支持**：支持主窗口、文件管理器窗口和服务器列表窗口的多窗口管理
- **子窗口继承**：子窗口自动继承主窗口位置和大小，提供一致的用户体验
- **资源协议处理**：使用自定义 asset:// 协议加载应用资源，优化资源管理
- **确认对话框**：重要操作前提供确认对话框，防止误操作
- **UI 遮罩层**：在关键操作时显示遮罩层，防止用户误触

## 系统要求

- **操作系统**：Windows 7 或更高版本
- **内存**：至少 2GB RAM
- **存储空间**：至少 100MB 可用空间（不包括下载的地图文件）

## 安装说明

1. 从 [发布页面](https://github.com/Nyasers/nyaser-maps-downloader/releases) 下载最新版本的安装程序
2. 运行安装程序并按照提示完成安装
3. 启动应用程序，开始使用地图下载功能

## 使用方法

1. 打开 Nyaser Maps Downloader 应用程序
2. 应用将自动加载 [maps.nyase.ru](https://maps.nyase.ru) 网站
3. 在网站上浏览并选择您想要下载的地图
4. 点击下载按钮，应用将自动处理下载和解压过程
5. 下载完成后，地图文件将被保存在指定目录中

## 开发指南

如果您想从源代码构建此应用，请按照以下步骤操作：

### 前置要求

- [Node.js](https://nodejs.org/)
- [npm](https://www.npmjs.com/)
- [Rust](https://www.rust-lang.org/)
- [Tauri](https://tauri.app/)

### 构建步骤

1. 克隆仓库：
   ```bash
   git clone https://github.com/Nyasers/nyaser-maps-downloader.git
   cd nyaser-maps-downloader
   ```

2. 安装依赖：
   ```bash
   npm install
   ```

3. 开发模式运行：
   ```bash
   npm run tauri dev
   ```

4. 构建发布版本：
   ```bash
   npm run tauri build
   ```

## 项目结构

```
nyaser-maps-downloader/
├── src/                # 前端源代码目录
│   ├── filemanager/    # 文件管理器模块
│   │   ├── main.css
│   │   ├── main.html
│   │   └── main.js
│   ├── plugin/         # 插件注入模块
│   │   ├── main.css
│   │   ├── main.html
│   │   └── main.js
│   └── serverlist/      # 服务器列表模块
│       └── main.js
├── src-tauri/           # Rust 后端代码
│   ├── bin/             # 二进制文件和依赖
│   │   ├── Lang/        # 语言文件
│   │   ├── 7z.dll
│   │   ├── 7z.exe
│   │   ├── 7zG.exe
│   │   └── aria2c.exe
│   ├── capabilities/   # Tauri 2.x 权限配置
│   │   ├── default.json
│   │   └── desktop.json
│   ├── icons/           # 应用图标
│   ├── src/             # 主源代码目录
│   │   ├── aria2c.rs          # aria2c 下载引擎集成与管理
│   │   ├── commands.rs        # Tauri 命令定义和前端交互接口
│   │   ├── config_manager.rs  # 配置管理模块
│   │   ├── dialog_manager.rs  # 对话框管理模块
│   │   ├── dir_manager.rs     # 目录管理模块
│   │   ├── download_manager.rs # 下载队列和任务管理
│   │   ├── extract_manager.rs # 解压管理器，处理下载文件的自动解压
│   │   ├── init.rs            # 应用初始化逻辑
│   │   ├── lib.rs             # 库入口文件
│   │   ├── log_utils.rs       # 日志工具函数
│   │   ├── main.rs            # 应用入口文件
│   │   ├── queue_manager.rs   # 通用队列管理功能
│   │   ├── symlink_manager.rs # 符号链接管理模块
│   │   └── utils.rs           # 工具函数集合
│   ├── build.rs         # 构建脚本
│   ├── tauri.conf.json  # Tauri 应用配置
│   └── Cargo.toml       # Rust 依赖配置
├── .github/             # GitHub Actions 工作流
│   └── workflows/
│       └── release.yml  # 自动发布配置
├── .vscode/             # VS Code 配置
├── minify.js            # HTML 压缩脚本
├── minify-options.js    # 压缩选项配置
├── package.json         # 前端依赖配置
├── version.js           # 版本管理脚本
├── version-sync.js      # 版本同步脚本
└── README.md            # 项目文档（当前文件）
```

## 技术栈

- **前端**：HTML, CSS, JavaScript
- **后端**：Rust
- **框架**：[Tauri 2](https://tauri.app/)
- **下载引擎**：aria2c
- **解压工具**：7-Zip (7z.exe)
- **构建工具**：npm, Cargo
- **核心依赖**：
  - 前端：@tauri-apps/cli, cssnano, html-minifier-terser, terser, dotenv-cli
  - 后端：
    - Tauri 2.x (protocol-asset feature)
    - tauri-plugin-dialog, tauri-plugin-deep-link, tauri-plugin-single-instance, tauri-plugin-updater
    - serde, serde_json (序列化与反序列化)
    - tokio (异步运行时)
    - uuid (唯一标识符生成)
    - chrono (日期时间处理)
    - winapi, windows-sys (Windows API 绑定)
    - winreg (Windows 注册表操作)
    - urlencoding (URL 编码解码)
    - regex (正则表达式)
    - lazy_static (静态变量延迟初始化)
    - reqwest (HTTP 客户端，用于 aria2c RPC 请求)
  - 构建工具：tauri-build

## 许可证

[MIT License](LICENSE)

## 注意事项

- 该应用仅支持 Windows 操作系统
- 请确保您有足够的存储空间用于下载和存储地图文件
- 应用会在临时目录中存储下载的文件，请定期清理以释放空间
- 首次运行时需要配置数据存储目录，请选择合适的存储位置
- 应用支持自动更新功能，建议保持网络连接以获取最新版本
- 如需使用 Deep Link 功能，请确保已正确关联 nmd:// 协议
- 应用关闭时会自动清理临时资源，确保系统资源得到释放

