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
    const { core: { invoke } } = window.__TAURI__;
    const iconPlaceholder = getIconPlaceholder(icon);
    await invoke("open_server_window", {
      url: url,
      name: name,
      icon: iconPlaceholder,
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
