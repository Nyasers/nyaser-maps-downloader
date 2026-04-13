const getAssets = (asset) =>
  decodeURIComponent(window.__TAURI__.core.convertFileSrc(asset, "asset"));

/**
 * 获取服务器图标占位符
 * @param {string} icon - 服务器图标（字符串）
 * @returns {string} 图标占位符
 */
function getIconPlaceholder(icon) {
  return icon || "🌐";
}

/**
 * 渲染服务器列表
 * @param {Array} servers - 服务器列表数据
 */
function renderServerList(servers) {
  const serverList = document.getElementById("serverList");
  const template = document.getElementById("serverItemTemplate");

  if (servers.length === 0) {
    serverList.innerHTML = '<p class="loading">暂无服务器</p>';
    return;
  }

  serverList.innerHTML = "";

  servers.forEach((server) => {
    const clone = template.content.cloneNode(true);
    const serverItem = clone.querySelector(".server-item");

    serverItem.setAttribute("data-url", server.url);

    // 尝试加载服务器图标
    const serverIcon = serverItem.querySelector(".server-icon");
    serverIcon.textContent = getIconPlaceholder(server.icon);

    // 检查是否强制使用占位符
    if (!server.iconOffline) {
      try {
        const hostname = new URL(server.url).hostname;

        // 从Bitwarden图标服务获取图标
        const iconUrl = `https://icons.bitwarden.net/${hostname}/icon.png`;

        // 创建img元素来加载图标
        const img = document.createElement("img");
        img.src = iconUrl;
        img.alt = server.name;
        img.style.width = "100%";
        img.style.height = "100%";

        // 图标加载成功时替换文本
        img.onload = () => {
          serverIcon.textContent = "";
          serverIcon.appendChild(img);
        };
      } catch (e) {}
    }

    serverItem.querySelector(".server-name").textContent = server.name;
    serverItem.querySelector(".server-url").textContent = server.url;

    const openBtn = serverItem.querySelector(".open-btn");
    openBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      openServerWindow(server.url, server.name, server.icon);
    });

    serverItem.addEventListener("click", () => {
      openServerWindow(server.url, server.name, server.icon);
    });

    serverList.appendChild(clone);
  });
}

/**
 * 打开服务器窗口
 * @param {string} url - 服务器URL
 * @param {string} name - 服务器名称
 * @param {object|string} icon - 服务器图标（对象或字符串）
 */
async function openServerWindow(url, name, icon) {
  try {
    const {
      core: { invoke },
      webviewWindow: { WebviewWindow, getCurrentWebviewWindow },
    } = window.__TAURI__;

    const windowLabel = `server_${new URL(url).hostname.replaceAll(".", "_")}`;

    const parentWindow = await getCurrentWebviewWindow();

    // 处理图标格式，获取占位符
    const iconPlaceholder = getIconPlaceholder(icon);

    // 获取父窗口的位置和大小
    let windowOptions = {
      url: url,
      title: `${name} ${iconPlaceholder}`,
      width: 1024,
      height: 768,
      parent: "serverlist",
      minimizable: false,
    };

    try {
      const position = await parentWindow.outerPosition();
      const size = await parentWindow.innerSize();
      const maximized = await parentWindow.isMaximized();
      Object.assign(windowOptions, {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized: maximized,
      });
    } catch (err) {
      console.warn("获取窗口信息失败:", err);
    }

    const webview = new WebviewWindow(windowLabel, windowOptions);

    webview.once("tauri://created", function () {
      console.log("窗口创建成功");
      parentWindow?.hide();
    });

    webview.once("tauri://error", function (e) {
      console.error("创建窗口失败:", e);
      alert(`创建窗口失败: ${e.payload}`);
    });

    webview.once("tauri://destroyed", function () {
      console.log("窗口销毁成功");
      invoke("open_serverlist_window");
    });
  } catch (error) {
    console.error("打开窗口失败:", error);
    alert(`打开窗口失败: ${error}`);
  }
}

async function main() {
  return import(getAssets("serverlist/list.json"), {
    with: { type: "json" },
  }).then(({ default: servers }) => renderServerList(servers));
}

main();
