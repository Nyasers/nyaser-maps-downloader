# Nyaser Maps Downloader

Nyaser Maps Downloader 是一个专为从 [maps.nyase.ru](https://maps.nyase.ru) 网站下载和管理地图文件而设计的桌面应用程序。该应用基于 Tauri 框架构建，提供高效、可靠的地图下载体验。

## 项目简介

Nyaser Maps Downloader 简化了从 maps.nyase.ru 网站获取地图资源的过程，通过智能拦截下载请求和高效的下载队列管理，为用户提供流畅的地图获取体验。

## 主要功能

- **一键下载**：通过 HTML 注入技术，自动拦截并处理地图下载请求，简化下载流程
- **高效下载管理**：使用 aria2c 作为下载后端，支持队列管理和并行下载，确保最佳下载速度
- **自动解压**：下载完成后自动解压地图文件到指定目录，支持多种压缩格式
- **用户友好界面**：基于 Web 技术的现代化界面，与 maps.nyase.ru 网站无缝集成
- **文件管理器**：内置文件管理器，方便用户查看、管理和删除已下载的地图文件
- **服务器列表窗口**：集成服务器列表访问功能，方便用户快速查找和加入游戏服务器
- **配置管理**：支持用户自定义配置，灵活管理应用设置和数据存储目录
- **符号链接管理**：支持扫描和管理文件符号链接，方便管理游戏地图文件
- **Deep Link 支持**：支持 nmd:// 协议，可通过外部链接直接启动应用并执行特定操作
- **自动更新**：集成 Tauri updater 插件，支持应用自动更新和版本检查
- **智能文件名提取**：支持从百度 PCS 链接等特殊格式中提取文件名
- **多窗口支持**：支持主窗口、文件管理器窗口和服务器列表窗口的多窗口管理

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
- **构建工具**：npm, Cargo
- **核心依赖**：
  - 前端：@tauri-apps/cli, @tauri-apps/plugin-dialog, @tauri-apps/plugin-updater, @tauri-apps/plugin-deep-link, @tauri-apps/plugin-single-instance
  - 后端：serde, serde_json, tokio, uuid, regex, winreg, chrono, winapi, windows-sys, urlencoding, lazy_static, tauri-plugin-deep-link, tauri-plugin-updater, tauri-plugin-single-instance
  - 构建工具：html-minifier-terser, terser, cssnano, dotenv-cli

## 许可证

[MIT License](LICENSE)

## 注意事项

- 该应用仅支持 Windows 操作系统
- 请确保您有足够的存储空间用于下载和存储地图文件
- 应用会在临时目录中存储下载的文件，请定期清理以释放空间
- 首次运行时需要配置数据存储目录，请选择合适的存储位置
- 应用支持自动更新功能，建议保持网络连接以获取最新版本
- 如需使用 Deep Link 功能，请确保已正确关联 nmd:// 协议

