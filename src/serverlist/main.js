const getAssets = (asset) =>
  decodeURIComponent(window.__TAURI__.core.convertFileSrc(asset, "asset"));

async function loadServerList() {
  try {
    const listUrl = getAssets("serverlist/list.json");
    const response = await fetch(listUrl);
    const servers = await response.json();
    return servers;
  } catch (error) {
    console.error("加载服务器列表失败:", error);
    return [];
  }
}

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
    serverItem.querySelector(".server-icon").textContent = server.icon || "🌐";
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

async function openServerWindow(url, name, icon) {
  try {
    const {
      core: { invoke },
      webviewWindow: { WebviewWindow, getCurrentWebviewWindow },
    } = window.__TAURI__;

    const windowLabel = `server_${new URL(url).hostname.replaceAll(".", "_")}`;

    const parentWindow = await getCurrentWebviewWindow();

    const webview = new WebviewWindow(windowLabel, {
      url: url,
      title: `${name} ${icon || ""}`,
      width: 1024,
      height: 768,
      parent: "serverlist",
      minimizable: false,
      center: true,
    });

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
  const servers = await loadServerList();
  renderServerList(servers);
}

main();
