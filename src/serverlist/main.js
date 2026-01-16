// Nyaser Maps Downloader - Server List Button Click Logic

// 查找指定按钮的函数
function findAndClickButton() {
  const button = document.querySelector(
    "#app-container > div > section > main > div > form > div > div > button"
  );
  if (button) {
    console.log("Nyaser Maps Downloader: 找到指定按钮，触发点击事件");
    // 创建并触发点击事件
    const event = new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
      view: window,
    });
    button.dispatchEvent(event);
    return true;
  }
  return false;
}

function main() {
  // 先尝试直接查找并点击按钮
  if (findAndClickButton()) {
    console.log("Nyaser Maps Downloader: 按钮点击成功完成");
    return;
  }

  console.log("Nyaser Maps Downloader: 未找到指定按钮，添加DOM加载完成监听");

  // 如果按钮不存在，添加DOMContentLoaded事件监听
  if (document.readyState === "loading") {
    // 文档仍在加载中，监听DOMContentLoaded事件
    document.addEventListener("DOMContentLoaded", function () {
      console.log("Nyaser Maps Downloader: DOMContentLoaded事件触发");
      if (!findAndClickButton()) {
        console.log(
          "Nyaser Maps Downloader: DOMContentLoaded后仍未找到按钮，添加1秒延迟后重试"
        );
        setTimeout(findAndClickButton, 1000);
      }
    });
  } else {
    // 文档已加载完成但按钮仍不存在，可能是动态生成的，添加MutationObserver
    console.log(
      "Nyaser Maps Downloader: 文档已加载但按钮不存在，添加MutationObserver监听DOM变化"
    );
    const observer = new MutationObserver(function (mutations) {
      if (findAndClickButton()) {
        console.log(
          "Nyaser Maps Downloader: 按钮通过MutationObserver找到并点击"
        );
        observer.disconnect();
      }
    });

    // 配置观察选项
    const config = {
      childList: true,
      subtree: true,
    };

    // 开始观察文档体
    observer.observe(document.body, config);

    // 5秒后如果还没找到，停止观察
    setTimeout(function () {
      observer.disconnect();
      console.log("Nyaser Maps Downloader: 观察超时，停止监听DOM变化");
    }, 5000);
  }
}

main();
