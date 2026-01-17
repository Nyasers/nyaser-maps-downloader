!(function () {
  try {
    // 数据存储目录配置检查
    async function checkDataDirConfig() {
      try {
        // 尝试获取数据存储目录
        const { invoke } = window.__TAURI__.core;
        const config = await invoke("read_config", {
          configName: "config.json",
        });

        // 检查是否已配置数据存储目录
        if (!config || !config.nmd_data) {
          console.log(
            "Nyaser Maps Downloader: 未配置数据存储目录，正在弹出选择对话框...",
          );
          showOverlay();
          try {
            // 弹出目录选择对话框
            const selectedDir = await invoke("show_directory_dialog");

            // 保存选择的目录
            await invoke("write_config", {
              configName: "config.json",
              config: { ...config, nmd_data: selectedDir },
            });

            console.log(
              "Nyaser Maps Downloader: 已配置数据存储目录:",
              selectedDir,
            );
          } finally {
            hideOverlay();
          }
        } else {
          console.log(
            "Nyaser Maps Downloader: 数据存储目录已配置:",
            config.nmd_data,
          );
        }
      } catch (error) {
        console.error("Nyaser Maps Downloader: 配置数据存储目录失败:", error);
        const errorMsg = error.message || JSON.stringify(error);
        const dialog = window.__TAURI__.dialog;
        const shouldRetry = await dialog.confirm(
          `配置数据存储目录失败: ${errorMsg}\n\n程序无法进行初始化，功能无法将正常使用，是否重试？`,
          {
            title: "初始化失败",
            okLabel: "重试",
            cancelLabel: "取消",
          },
        );
        if (shouldRetry) {
          await checkDataDirConfig();
        }
      }
    }

    // 创建下载任务容器和警告通知元素
    const downloadsContainer = document.createElement("div");
    downloadsContainer.className = "nmd-container";
    document.body.appendChild(downloadsContainer);

    const warningDisplay = document.createElement("div");
    warningDisplay.className = "nmd-warning";
    document.body.appendChild(warningDisplay);

    // 创建遮罩层
    const overlay = document.createElement("div");
    overlay.className = "nmd-overlay";
    overlay.style.display = "none";
    overlay.innerHTML = `<div class="nmd-overlay-content"><div class="nmd-spinner"></div><div class="nmd-overlay-text">等待用户操作...</div></div>`;
    document.body.appendChild(overlay);

    // 显示遮罩层
    function showOverlay() {
      overlay.style.display = "flex";
    }

    // 隐藏遮罩层
    function hideOverlay() {
      overlay.remove();
    }

    // 存储当前活动的下载任务
    const activeTasks = new Map();

    // 创建排队任务容器
    const queueContainer = document.createElement("div");
    queueContainer.className = "nmd-queue-container";
    queueContainer.style.display = "none"; // 默认隐藏
    downloadsContainer.appendChild(queueContainer);

    // 创建排队任务标题
    const queueTitle = document.createElement("div");
    queueTitle.className = "nmd-queue-title";
    queueContainer.appendChild(queueTitle);

    // 创建排队任务列表
    const queueList = document.createElement("div");
    queueList.className = "nmd-queue-list";
    queueContainer.appendChild(queueList);

    // 创建队列为空时的提示
    const queueEmpty = document.createElement("div");
    queueEmpty.className = "nmd-queue-empty";
    queueEmpty.textContent = "队列为空";
    queueList.appendChild(queueEmpty);

    // 创建下载任务元素的函数
    function createTaskElement(
      taskId,
      filename,
      initialStatus = "准备下载...",
    ) {
      // 创建任务容器
      const taskElement = document.createElement("div");
      taskElement.className = "nmd-task";
      taskElement.dataset.taskId = taskId;

      // 创建任务头部
      const taskHeader = document.createElement("div");
      taskHeader.className = "nmd-task-header";

      // 创建文件名显示
      const filenameElement = document.createElement("div");
      filenameElement.className = "nmd-task-filename";
      filenameElement.textContent = filename;

      // 创建状态显示
      const statusElement = document.createElement("div");
      statusElement.className = "nmd-task-status";
      statusElement.textContent = initialStatus;

      // 创建取消按钮
      const cancelButton = document.createElement("button");
      cancelButton.className = "nmd-cancel-button";
      cancelButton.textContent = "取消";
      cancelButton.title = "取消下载";
      cancelButton.addEventListener("click", async () => {
        try {
          // 调用后端取消下载命令
          await window.__TAURI__.core.invoke("cancel_download", {
            taskId: taskId,
          });
          console.log("Nyaser Maps Downloader: 取消下载任务:", taskId);
        } catch (error) {
          console.error("Nyaser Maps Downloader: 取消下载失败:", error);
          warningDisplay.textContent = "错误: 取消下载失败 - " + error.message;
          warningDisplay.style.display = "block";
          warningDisplay.style.background = "rgba(244, 67, 54, 0.9)";

          setTimeout(() => {
            warningDisplay.style.display = "none";
            warningDisplay.style.background = "rgba(255, 152, 0, 0.9)";
          }, 5000);
        }
      });

      // 创建进度条容器
      const progressContainer = document.createElement("div");
      progressContainer.className = "nmd-progress";

      // 创建进度条
      const progressBar = document.createElement("div");
      progressBar.className = "nmd-progress-bar";

      // 创建进度百分比
      const progressText = document.createElement("div");
      progressText.className = "nmd-progress-text";
      progressText.textContent = "0%";

      // 创建进度信息容器
      const progressInfo = document.createElement("div");
      progressInfo.className = "nmd-progress-info";
      progressInfo.appendChild(progressText);

      // 创建原始aria2c输出显示
      const rawOutputElement = document.createElement("div");
      rawOutputElement.className = "nmd-raw-output";
      rawOutputElement.textContent = ""; // 初始为空

      // 组装元素
      taskHeader.appendChild(filenameElement);
      taskHeader.appendChild(statusElement);
      taskHeader.appendChild(cancelButton);
      progressContainer.appendChild(progressBar);
      taskElement.appendChild(taskHeader);
      taskElement.appendChild(progressContainer);
      taskElement.appendChild(progressInfo);
      taskElement.appendChild(rawOutputElement);

      // 添加到容器
      downloadsContainer.appendChild(taskElement);

      // 返回创建的元素引用
      return {
        element: taskElement,
        filename: filenameElement,
        status: statusElement,
        progress: progressBar,
        progressText: progressText,
        rawOutput: rawOutputElement,
        cancelButton: cancelButton,
      };
    }

    // 移除下载任务元素的函数
    function removeTaskElement(taskId) {
      const task = activeTasks.get(taskId);
      if (task) {
        activeTasks.delete(taskId);
        // 同时从lastUpdateTimes中删除
        if (typeof lastUpdateTimes !== "undefined") {
          lastUpdateTimes.delete(taskId);
        }
        // 添加淡出动画
        task.element.style.opacity = "0";
        setTimeout(() => {
          if (task.element.parentNode) {
            task.element.parentNode.removeChild(task.element);
          }

          // 如果没有活动任务，隐藏容器
          if (activeTasks.size === 0) {
            downloadsContainer.style.display = "none";
          }

          refreshDownloadQueue().catch((error) => {
            console.error("Nyaser Maps Downloader: 刷新队列失败:", error);
          });
        }, 300);
      }
    }

    // 刷新下载队列
    async function refreshDownloadQueue() {
      return window.__TAURI__.core.invoke("refresh_download_queue");
    }

    // 将下载链接传递给后端处理
    async function handleDownloadLink(url, savepath = "", saveonly = false) {
      try {
        let result = await window.__TAURI__.core.invoke("install", {
          url: url,
          savepath: savepath,
          saveonly: saveonly,
        });
        return true;
      } catch (error) {
        console.error("Nyaser Maps Downloader: 处理下载链接失败:", error);
        // 为依赖错误提供更详细的帮助信息
        let errorMessage = error.message || "未知错误";

        // 显示错误警告信息
        warningDisplay.textContent = "错误: 下载失败 - " + errorMessage;
        warningDisplay.style.display = "block";
        warningDisplay.style.background = "rgba(244, 67, 54, 0.9)"; // 红色背景表示错误

        // 10秒后自动隐藏错误警告
        setTimeout(() => {
          warningDisplay.style.display = "none";
          warningDisplay.style.background = "rgba(255, 152, 0, 0.9)"; // 恢复橙色背景
        }, 10000);

        return false;
      }
    }

    function isNormalLink(url) {
      return url.match(/http(s?):\/\/.+\.nyase\.ru\/(d|p)\/.+/);
    }

    function getFilenameFromNormalLink(url) {
      let re = /\/([^\/?]+)(\?.*)?$/;
      let match = url.match(re);
      if (match && match[1]) {
        return match[1];
      } else {
        return null;
      }
    }

    function isBaidupcsLink(url) {
      return url.match(/http(s?):\/\/.+\.baidupcs\.com\/file\/.+/);
    }

    function getFilenameFromBaidupcsLink(url) {
      let re = /&fin=([^&]+)/;
      let match = url.match(re);
      if (match && match[1]) {
        let encoded_name = match[1];
        let fixed_encoded = encoded_name.replaceAll("+", "%20");
        if (fixed_encoded) {
          return decodeURIComponent(fixed_encoded);
        } else {
          return decodeURIComponent(encoded_name);
        }
      } else {
        return null;
      }
    }

    // 检测是否为下载链接
    function isDownloadLink(url) {
      return url && (isNormalLink(url) || isBaidupcsLink(url));
    }

    function getFilename(url) {
      if (isNormalLink(url)) {
        return getFilenameFromNormalLink(url);
      } else if (isBaidupcsLink(url)) {
        return getFilenameFromBaidupcsLink(url);
      } else {
        return null;
      }
    }

    // 等待全局TAURI对象可用的函数
    function waitForTauri() {
      return new Promise((resolve) => {
        let attempts = 0;
        const maxAttempts = 10;
        const checkInterval = setInterval(() => {
          attempts++;
          if (
            window.__TAURI__?.core?.invoke &&
            window.__TAURI__?.event?.listen
          ) {
            clearInterval(checkInterval);
            resolve(true);
          } else if (attempts >= maxAttempts) {
            clearInterval(checkInterval);
            resolve(false);
          }
        }, 100);
      });
    }

    // 检测到Tauri环境，静默启动

    function createObserver(callback) {
      const observer = new MutationObserver(callback);
      return function () {
        observer.observe(document.body, { childList: true, subtree: true });
        callback([], observer);
      };
    }

    const setupShortermObserver = createObserver((mutations, observer) => {
      let loginButtonRemoved = false;

      for (const mutation of mutations) {
        if (mutation.type === "childList") {
          // 尝试移除登录按钮
          try {
            const loginButton = document.querySelector(
              "#root > div.footer.hope-stack > div > a.hope-anchor.inactive",
            );
            if (loginButton) {
              loginButton.remove();
              loginButtonRemoved = true;

              // 同样处理另一个元素
              const bar = document.querySelector(
                "#root > div.footer.hope-stack > div > span",
              );
              if (bar) {
                bar.remove();
              }

              // 更新下载队列
              refreshDownloadQueue();
            }
          } catch {}
        }
      }

      // 如果登录按钮已移除，则停止主观察器以优化性能
      if (loginButtonRemoved) {
        observer.disconnect();
      }
    });

    const setupLongtermObserver = createObserver((mutations, observer) => {
      try {
        if (!document.querySelector("#steam-launch-button")) {
          // 尝试找到left-toolbar-in元素
          const leftToolbar = document.querySelector("div.left-toolbar-in");
          if (leftToolbar) {
            const settings = document.querySelector(
              "div.left-toolbar-in > svg:nth-child(3)",
            );
            if (settings) {
              // 创建工具栏按钮的函数
              function createToolbarButton(buttonId, svgContent, clickHandler) {
                const button = settings.parentNode.appendChild(
                  settings.cloneNode(),
                );
                button.id = buttonId;
                button.innerHTML = svgContent;

                // 添加点击事件处理
                button.addEventListener("click", (event) => {
                  event.stopPropagation(); // 阻止事件冒泡
                  clickHandler(event);
                });
              }

              // 创建Steam启动按钮
              createToolbarButton(
                "steam-launch-button",
                `<path d="M424.8064 0l60.943515 36.615758-61.967515 103.051636h218.329212L673.359127 192.201697 575.706764 372.363636h160.923151c84.743758 0 164.615758 33.978182 224.907637 95.635394A327.059394 327.059394 0 0 1 1055.031855 697.995636v0.496485a327.059394 327.059394 0 0 1-93.494303 229.996606C901.245673 990.145939 821.280582 1024 736.505794 1024H318.403491c-84.743758 0-164.615758-33.978182-224.907636-95.635394A326.997333 326.997333 0 0 1 0.001552 698.492121v-0.496485a327.059394 327.059394 0 0 1 93.494303-229.996606C153.787733 406.341818 233.659733 372.363636 318.403491 372.363636h176.469333l87.505455-161.512727h-221.525334l-30.409697-53.992727L424.83743 0zM736.660945 455.959273H318.372461c-130.451394 0-236.668121 108.606061-236.668122 242.036363v0.496485c0 133.430303 106.216727 242.036364 236.668122 242.036364H736.660945c130.451394 0 236.668121-108.606061 236.668122-242.036364v-0.496485c0-133.430303-106.216727-242.036364-236.668122-242.036363z m-51.386181 138.457212A90.608485 90.608485 0 0 1 775.759127 685.08703 90.608485 90.608485 0 0 1 685.243733 775.757576a90.701576 90.701576 0 0 1 0-181.341091z m-405.566061 9.18497l62.681212 0.155151L342.172703 651.636364H403.395491v93.090909h-61.377939l-0.062061 21.938424L279.274279 766.510545 279.336339 744.727273H248.243976v-93.090909h31.278545l0.124121-48.034909z m405.566061 43.442424c-20.976485 0-38.105212 17.159758-38.105212 38.167273 0 21.007515 17.128727 38.167273 38.105212 38.167272a38.167273 38.167273 0 1 0 0-76.334545z" fill="#1890ff" p-id="1755"></path>`,
                (event) => {
                  location.href = "steam://rungameid/550";
                },
              );

              // 创建服务器列表按钮
              createToolbarButton(
                "server-list-button",
                `<path d="M864 138.666667v768h-704v-768h704z m-64 533.333333h-576v170.666667h576v-170.666667zM704 725.333333v64h-128v-64h128z m96-288h-576v170.666667h576v-170.666667zM704 490.666667v64h-128v-64h128z m96-288h-576v170.666666h576v-170.666666zM704 256v64h-128v-64h128z" fill="#1890ff" p-id="4714"></path>`,
                async (event) => {
                  try {
                    console.log(
                      "Nyaser Maps Downloader: 调用后端open_server_list_window命令",
                    );
                    await window.__TAURI__.core.invoke(
                      "open_server_list_window",
                    );
                  } catch (e) {
                    console.error(
                      "Nyaser Maps Downloader: 打开服务器列表窗口时出错:",
                      e,
                    );
                  }
                },
              );

              // 创建文件管理器按钮
              createToolbarButton(
                "file-manager-button",
                `<path d="M98 351c-17.7 0-32-14.3-32-32V192.8c0-52.9 43.1-96 96-96h221c52.9 0 96 43.1 96 96V255c0 17.7-14.3 32-32 32s-32-14.3-32-32v-62.2c0-17.6-14.4-32-32-32H162c-17.6 0-32 14.4-32 32V319c0 17.6-14.3 32-32 32z" fill="#1890ff" p-id="4791"></path><path d="M864 926.6H383c-17.7 0-32-14.3-32-32s14.3-32 32-32h481c17.6 0 32-14.4 32-32V319c0-17.6-14.4-32-32-32H447c-17.7 0-32-14.3-32-32s14.3-32 32-32h417c52.9 0 96 43.1 96 96v511.7c0 52.9-43.1 95.9-96 95.9z" fill="#1890ff" p-id="4792"></path><path d="M383 926.6H162c-52.9 0-96-43.1-96-96V319c0-17.7 14.3-32 32-32s32 14.3 32 32v511.7c0 17.6 14.4 32 32 32h221c17.7 0 32 14.3 32 32 0 17.6-14.3 31.9-32 31.9zM768.1 511.2H256c-17.7 0-32-14.3-32-32s14.3-32 32-32h512.1c17.7 0 32 14.3 32 32s-14.3 32-32 32z" fill="#1890ff" p-id="4793"></path><path d="M768.1 703H256c-17.7 0-32-14.3-32-32s14.3-32 32-32h512.1c17.7 0 32 14.3 32 32s-14.3 32-32 32z" fill="#1890ff" p-id="4794"></path>`,
                async (event) => {
                  try {
                    console.log(
                      "Nyaser Maps Downloader: 调用后端open_file_manager_window命令",
                    );
                    await window.__TAURI__.core.invoke(
                      "open_file_manager_window",
                    );
                  } catch (e) {
                    console.error(
                      "Nyaser Maps Downloader: 打开文件管理器窗口时出错:",
                      e,
                    );
                  }
                },
              );
            }
          }
        }
      } catch (e) {
        console.error("Nyaser Maps Downloader: 添加按钮时出错:", e);
      }
      try {
        // 1. 尝试找到第一个目标按钮 - 改为"安装"
        const installButton = document.querySelector(
          "div.fileinfo > div:nth-child(3) > div > a",
        );
        if (installButton && installButton.textContent !== "安装") {
          // 将按钮文本改为"安装"
          installButton.textContent = "安装";
          console.log('Nyaser Maps Downloader: 成功修改按钮文本为"安装"');
        }

        // 2. 尝试找到第二个目标按钮 - 复制链接按钮改为"下载并安装"
        const copyLinkButton = document.querySelector(
          "div.fileinfo > div:nth-child(3) > div > button",
        );
        if (copyLinkButton && copyLinkButton.textContent !== "下载") {
          // 将按钮文本改为"下载"
          copyLinkButton.textContent = "下载";

          // 移除原有的点击事件监听器并创建新按钮
          const newButton = copyLinkButton.cloneNode(true);
          copyLinkButton.parentNode.replaceChild(newButton, copyLinkButton);

          // 添加新的点击事件处理器，使用捕获阶段确保优先处理
          newButton.addEventListener(
            "click",
            async function (event) {
              // 尝试获取链接
              const linkElement = document.querySelector(
                "div.fileinfo > div:nth-child(3) > div > a",
              );
              if (linkElement && linkElement.href) {
                try {
                  console.log(
                    "Nyaser Maps Downloader: 点击下载按钮，链接地址:",
                    linkElement.href,
                  );
                  const filename = getFilename(linkElement.href);
                  if (filename) {
                    console.log(
                      "Nyaser Maps Downloader: 提取到文件名:",
                      filename,
                    );
                  } else {
                    throw new Error(
                      "Nyaser Maps Downloader: 从链接中提取文件名失败",
                    );
                  }
                  const dialog = window.__TAURI__.dialog;
                  const savepath = await dialog.save({
                    title: "选择保存位置",
                    defaultPath: filename,
                    filters: [
                      {
                        name: "Archive Files",
                        extensions: [
                          "7z",
                          "zip",
                          "rar",
                          "tar",
                          "gz",
                          "bz2",
                          "xz",
                          "arj",
                          "cab",
                          "chm",
                          "cpio",
                          "deb",
                          "dmg",
                          "iso",
                          "lzh",
                          "lzma",
                          "msi",
                          "nsis",
                          "rpm",
                          "udf",
                          "wim",
                          "xar",
                          "z",
                        ],
                      },
                    ],
                  });
                  if (savepath) {
                    const saveonly = await dialog.confirm(
                      `保存位置:\n${savepath}`,
                      {
                        title: "选择模式",
                        okLabel: "仅保存",
                        cancelLabel: "保存并安装",
                      },
                    );
                    await handleDownloadLink(
                      linkElement.href,
                      savepath,
                      saveonly,
                    );
                  }
                } catch (e) {
                  console.error(
                    "Nyaser Maps Downloader: 处理点击事件时出错:",
                    e.message,
                    e.stack,
                  );
                }
              } else {
                console.error(
                  "Nyaser Maps Downloader: 未找到链接元素或链接地址",
                );
              }
            },
            true,
          ); // 使用true参数在捕获阶段执行

          console.log('Nyaser Maps Downloader: 成功修改复制链接按钮为"下载"');
        }
      } catch (e) {
        console.error("Nyaser Maps Downloader: 修改按钮时出错:", e);
      }
      document.querySelector("div.markdown")?.parentElement.remove();
      const dl = document.querySelector("a[href='/dl-nmd']");
      if (dl) {
        dl.parentElement.remove();
        const cnt = document.querySelector("div.list > p");
        if (cnt) {
          const match = cnt.textContent.match(/.+有 (.+) 个.+/);
          if (match) {
            const count = parseInt(match[1]);
            cnt.textContent = match[0].replace(match[1], count - 1);
          }
        }
      }
    });

    const observers = [setupShortermObserver, setupLongtermObserver];

    waitForTauri().then((tauriReady) => {
      if (!tauriReady) {
        console.error("Nyaser Maps Downloader: Tauri API未能完全初始化");
        return;
      }

      // 加载并注入中间件CSS
      const link = document.createElement("link");
      link.setAttribute("rel", "stylesheet");
      link.setAttribute(
        "href",
        decodeURIComponent(
          window.__TAURI__.core.convertFileSrc("plugin/main.css", "asset"),
        ),
      );
      document.head.appendChild(link);

      // 检查数据存储目录配置
      checkDataDirConfig();

      // 初始化完成后再启动观察器
      observers.map((f) => f());

      // 准备注册事件监听器

      // 存储任务最后更新时间的映射
      const lastUpdateTimes = new Map();

      // 检查并移除长时间无响应的下载任务
      function checkStalledDownloads() {
        const currentTime = Date.now();
        const stallThreshold = 3e4; // 30秒无更新则视为停滞

        activeTasks.forEach((task, taskId) => {
          const lastUpdateTime = lastUpdateTimes.get(taskId);
          if (lastUpdateTime && currentTime - lastUpdateTime > stallThreshold) {
            // 更新状态为下载中断
            task.status.textContent = "下载中断";
            task.progress.style.background = "#ff9800"; // 橙色背景表示警告

            // 向后端发送特殊的取消下载命令，表示这是因为下载停滞而取消的
            try {
              window.__TAURI__.core.invoke("cancel_download", {
                taskId: taskId,
                reason: "stalled", // 添加reason参数标记这是由于下载停滞而取消的
              });
              console.log(
                "Nyaser Maps Downloader: 检测到下载停滞，已取消下载任务:",
                taskId,
              );
            } catch (error) {
              console.error("Nyaser Maps Downloader: 取消停滞下载失败:", error);
            }

            // 再显示5秒后移除
            setTimeout(() => {
              removeTaskElement(taskId);
              lastUpdateTimes.delete(taskId);
            }, 5000);
          }
        });
      }

      // 每5秒检查一次停滞的下载任务
      setInterval(checkStalledDownloads, 5000);

      // 提取event API
      const { listen } = window.__TAURI__.event;

      // 监听深层链接打开事件
      const deepLinkUnlisten = listen("deep-link-open", (event) => {
        let url = event.payload;
        if (!url.startsWith("/")) url = "/" + url;
        console.log("Open Link Received:", url);
        location.href = url;
      });

      // 监听下载进度事件
      const progressUnlisten = listen("download-progress", (event) => {
        // 接收到进度事件
        const { progress, filename, taskId, rawOutput } = event.payload;
        // 对文件名进行URL解码
        const decodedFilename = filename
          ? decodeURIComponent(filename)
          : "未知文件";

        // 确保容器可见
        downloadsContainer.style.display = "flex";

        // 获取或创建任务元素
        let task;
        if (activeTasks.has(taskId)) {
          task = activeTasks.get(taskId);
        } else {
          task = createTaskElement(taskId, decodedFilename, "正在下载...");
          activeTasks.set(taskId, task);
        }

        // 更新最后活动时间
        lastUpdateTimes.set(taskId, Date.now());

        // 更新进度信息
        const progressPercent = progress.toFixed(1); // 保留一位小数
        task.progress.style.width = `${progressPercent}%`;
        task.progressText.textContent = `${progressPercent}%`;
        task.status.textContent = "正在下载...";

        // 更新原始aria2c输出信息
        if (rawOutput && task.rawOutput) {
          task.rawOutput.textContent = rawOutput;
        }
      });

      // 进度事件监听器注册成功

      // 监听下载完成事件（仅表示下载完成，解压还未开始）
      const downloadedUnlisten = listen("download-complete", (event) => {
        // 接收到下载完成事件
        const { filename, success, message, taskId, saveonly } =
          event.payload || {};
        if (!taskId) return; // 如果没有taskId，忽略此事件

        // 对文件名进行URL解码
        const decodedFilename = filename
          ? decodeURIComponent(filename)
          : "未知文件";

        // 获取任务元素
        const element = activeTasks.get(taskId);
        if (element) {
          if (success) {
            // 更新任务状态为下载完成，准备解压
            element.status.textContent = "下载完成";
            element.progress.style.width = "100%";
            element.progressText.textContent = "100%";

            // 任务完成后移除取消按钮
            if (element.cancelButton && element.cancelButton.parentNode) {
              element.cancelButton.parentNode.removeChild(element.cancelButton);
              console.log(
                "Nyaser Maps Downloader: 下载完成，已移除取消按钮:",
                taskId,
              );
            }

            // 在控制台输出下载完成信息
            console.log("Nyaser Maps Downloader: 文件下载完成:", message);
          }

          if (saveonly) {
            // 延迟5秒后移除任务显示
            setTimeout(() => {
              removeTaskElement(taskId);
            }, 5000);
          }
        }
      });

      // 监听解压开始事件
      const extractStartUnlisten = listen("extract-start", (event) => {
        // 接收到解压开始事件
        const { filename, taskId, extractDir } = event.payload || {};
        if (!taskId) return; // 如果没有taskId，忽略此事件

        // 对文件名进行URL解码
        const decodedFilename = filename
          ? decodeURIComponent(filename)
          : "未知文件";

        // 获取任务元素
        const task = activeTasks.get(taskId);
        if (task) {
          // 更新任务状态为解压中
          task.status.textContent = "解压中";
          // 在控制台输出解压开始信息
          console.log(
            "Nyaser Maps Downloader: 开始解压文件:",
            decodedFilename,
            "到目录:",
            extractDir,
          );
        }
      });

      // 监听解压完成事件
      const extractCompleteUnlisten = listen("extract-complete", (event) => {
        // 接收到解压完成事件
        const { filename, success, message, taskId } = event.payload || {};
        if (!taskId) return; // 如果没有taskId，忽略此事件

        // 对文件名进行URL解码
        const decodedFilename = filename
          ? decodeURIComponent(filename)
          : "未知文件";

        // 获取任务元素
        const task = activeTasks.get(taskId);
        if (task) {
          // 更新任务状态
          if (success) {
            task.status.textContent = "解压完成";
            // 在控制台输出解压路径
            console.log(
              "Nyaser Maps Downloader: 文件解压完成，解压路径:",
              message,
            );
          } else {
            task.status.textContent = "解压失败";
            task.progress.style.background = "#f44336";
            // 在控制台输出错误信息
            console.error("Nyaser Maps Downloader: 解压失败:", message);
          }

          // 延迟5秒后移除任务显示
          setTimeout(() => {
            removeTaskElement(taskId);
          }, 5e3);
        }
      });

      // 监听下载任务开始事件
      const taskStartUnlisten = listen("download-task-start", (event) => {
        // 接收到下载任务开始事件
        const { taskId, filename, url } = event.payload || {};
        if (!taskId) return; // 如果没有taskId，忽略此事件

        // 对文件名进行URL解码
        const decodedFilename = filename
          ? decodeURIComponent(filename)
          : "未知文件";

        // 确保容器可见
        downloadsContainer.style.display = "flex";

        // 创建新的任务元素
        const task = createTaskElement(taskId, decodedFilename, "准备下载");
        activeTasks.set(taskId, task);

        // 在控制台输出任务开始信息
        console.log(
          "Nyaser Maps Downloader: 开始下载任务:",
          taskId,
          decodedFilename,
        );

        // 刷新下载队列
        refreshDownloadQueue().catch((error) => {
          console.error("Nyaser Maps Downloader: 刷新队列失败:", error);
        });
      });

      // 监听下载任务添加事件
      const taskAddUnlisten = listen("download-task-add", (event) => {
        // 接收到下载任务添加事件
        const { taskId, filename, url } = event.payload || {};
        if (!taskId) return; // 如果没有taskId，忽略此事件

        // 对文件名进行URL解码
        const decodedFilename = filename
          ? decodeURIComponent(filename)
          : "未知文件";

        // 在控制台输出任务添加信息
        console.log(
          "Nyaser Maps Downloader: 任务添加到队列:",
          taskId,
          decodedFilename,
        );

        // 刷新下载队列
        refreshDownloadQueue().catch((error) => {
          console.error("Nyaser Maps Downloader: 刷新队列失败:", error);
        });
      });

      // 监听下载队列更新事件
      const queueUpdateUnlisten = listen("download-queue-update", (event) => {
        // 接收到下载队列更新事件
        const { queue } = event.payload || {};
        if (!queue) return; // 如果没有queue信息，忽略此事件

        const { total_tasks, waiting_tasks, active_tasks } = queue;

        // 在控制台输出队列更新信息
        console.log(
          "Nyaser Maps Downloader: 队列更新 - 总任务数:",
          total_tasks,
          "等待任务:",
          waiting_tasks,
          "活跃任务:",
          active_tasks,
        );

        // 更新排队任务UI
        updateQueueDisplay(total_tasks, waiting_tasks, active_tasks);
      });

      // 监听目录更改事件
      const dirChangedUnlisten = listen("extract-dir-changed", (event) => {
        // 接收到目录更改事件
        const { newDir, success } = event.payload;

        if (newDir) {
          // 在控制台输出解压路径
          console.log("Nyaser Maps Downloader: 解压路径:", newDir);
        }
      });

      // 更新排队任务显示的函数
      function updateQueueDisplay(totalTasks, waitingTasks, activeTask) {
        // 清空队列列表
        queueList.innerHTML = "";

        // 更新标题
        queueTitle.innerHTML = `排队任务 (${waitingTasks.length})`;

        // 创建按钮容器
        const actionsContainer = document.createElement("div");
        actionsContainer.className = "nmd-queue-actions";

        // 添加刷新按钮
        const refreshButton = document.createElement("button");
        refreshButton.className = "nmd-queue-action-button refresh";
        refreshButton.textContent = "刷新";
        refreshButton.title = "刷新队列状态";
        refreshButton.addEventListener("click", async () => {
          try {
            console.log("Nyaser Maps Downloader: 刷新队列状态");
            // 请求后端刷新队列
            await refreshDownloadQueue();
          } catch (error) {
            console.error("Nyaser Maps Downloader: 刷新队列失败:", error);
            warningDisplay.textContent =
              "错误: 刷新队列失败 - " + error.message;
            warningDisplay.style.display = "block";
            warningDisplay.style.background = "rgba(244, 67, 54, 0.9)";

            setTimeout(() => {
              warningDisplay.style.display = "none";
              warningDisplay.style.background = "rgba(255, 152, 0, 0.9)";
            }, 5000);
          }
        });
        actionsContainer.appendChild(refreshButton);

        // 添加全部取消按钮
        const cancelAllButton = document.createElement("button");
        cancelAllButton.className = "nmd-queue-action-button cancel-all";
        cancelAllButton.textContent = "全部取消";
        cancelAllButton.title = "取消所有排队任务";
        cancelAllButton.addEventListener("click", async () => {
          if (waitingTasks && waitingTasks.length > 0) {
            try {
              console.log("Nyaser Maps Downloader: 取消所有排队任务");
              // 请求后端取消所有排队任务
              await window.__TAURI__.core.invoke("cancel_all_downloads");
            } catch (error) {
              console.error(
                "Nyaser Maps Downloader: 取消所有排队任务失败:",
                error,
              );
              warningDisplay.textContent =
                "错误: 取消所有排队任务失败 - " + error.message;
              warningDisplay.style.display = "block";
              warningDisplay.style.background = "rgba(244, 67, 54, 0.9)";

              setTimeout(() => {
                warningDisplay.style.display = "none";
                warningDisplay.style.background = "rgba(255, 152, 0, 0.9)";
              }, 5000);
            }
          }
        });
        actionsContainer.appendChild(cancelAllButton);

        // 将按钮容器添加到标题
        queueTitle.appendChild(actionsContainer);

        // 如果有等待任务，显示队列容器
        if (waitingTasks && waitingTasks.length > 0) {
          queueContainer.style.display = "block";

          // 检查是否有活跃任务
          const hasActiveTasks = activeTask.length > 0;

          // 添加每个排队任务
          waitingTasks.forEach((task, index) => {
            const queueTaskElement = document.createElement("div");
            queueTaskElement.className = "nmd-queue-task";

            // 创建文件名元素
            const filenameElement = document.createElement("div");
            filenameElement.className = "nmd-queue-task-filename";
            // 解码文件名
            const decodedFilename = task.filename
              ? decodeURIComponent(task.filename)
              : "未知文件";
            filenameElement.textContent = decodedFilename;

            // 创建位置元素
            const positionElement = document.createElement("div");
            positionElement.className = "nmd-queue-task-position";
            // 如果有活跃任务，位置为 index + 1；否则位置为 index（因为即将开始下载）
            const position = hasActiveTasks ? index + 1 : 0;
            positionElement.textContent =
              position > 0 ? `#${position}` : "即将开始";

            // 组装元素
            queueTaskElement.appendChild(filenameElement);
            queueTaskElement.appendChild(positionElement);
            queueList.appendChild(queueTaskElement);

            // 创建取消按钮
            const cancelButton = document.createElement("button");
            cancelButton.className = "nmd-queue-task-cancel";
            cancelButton.textContent = "取消";
            cancelButton.title = "取消此排队任务";

            // 添加取消按钮到任务元素
            queueTaskElement.appendChild(cancelButton);

            // 添加按钮点击事件，允许取消排队任务
            cancelButton.addEventListener("click", async (e) => {
              e.stopPropagation(); // 阻止事件冒泡

              if (task.id) {
                try {
                  // 调用后端取消下载命令
                  await window.__TAURI__.core.invoke("cancel_download", {
                    taskId: task.id,
                  });
                  console.log("Nyaser Maps Downloader: 取消排队任务:", task.id);
                } catch (error) {
                  console.error(
                    "Nyaser Maps Downloader: 取消排队任务失败:",
                    error,
                  );
                  warningDisplay.textContent =
                    "错误: 取消排队任务失败 - " + error.message;
                  warningDisplay.style.display = "block";
                  warningDisplay.style.background = "rgba(244, 67, 54, 0.9)";

                  setTimeout(() => {
                    warningDisplay.style.display = "none";
                    warningDisplay.style.background = "rgba(255, 152, 0, 0.9)";
                  }, 5000);
                }
              }
            });
          });
        } else {
          // 如果没有等待任务，隐藏队列容器
          queueContainer.style.display = "none";

          // 添加队列为空的提示
          const emptyElement = document.createElement("div");
          emptyElement.className = "nmd-queue-empty";
          emptyElement.textContent = "队列为空";
          queueList.appendChild(emptyElement);
        }

        activeTask.forEach((task, index) => {
          console.log(task);
          try {
            // 接收到进度事件
            const { filename, id } = task;
            // 对文件名进行URL解码
            const decodedFilename = filename
              ? decodeURIComponent(filename)
              : "未知文件";

            // 确保容器可见
            downloadsContainer.style.display = "flex";

            // 获取或创建任务元素
            let element;
            if (activeTasks.has(id)) {
              element = activeTasks.get(id);
            } else {
              element = createTaskElement(id, decodedFilename, "正在下载...");
              activeTasks.set(id, element);
            }

            // 更新最后活动时间
            lastUpdateTimes.set(id, Date.now());
          } catch (error) {
            console.error(error);
          }
        });
      }

      // 监听游戏目录警告事件
      const gameDirWarningUnlisten = listen("game-dir-warning", (event) => {
        // 游戏目录警告
        const { message } = event.payload;

        // 显示警告信息
        warningDisplay.textContent = "警告: " + message;
        warningDisplay.style.display = "block";

        // 8秒后自动隐藏警告，让用户有足够时间阅读警告信息
        setTimeout(() => {
          warningDisplay.style.display = "none";
        }, 8000);
      });

      // 监听下载失败事件
      const downloadFailedUnlisten = listen("download-failed", (event) => {
        // 接收到下载失败事件
        const { taskId, filename, error } = event.payload || {};
        if (!taskId) return; // 如果没有taskId，忽略此事件

        // 对文件名进行URL解码
        const decodedFilename = filename
          ? decodeURIComponent(filename)
          : "未知文件";

        // 获取任务元素
        const task = activeTasks.get(taskId);
        if (task) {
          // 更新任务状态为下载失败
          task.status.textContent = "下载失败";
          task.progress.style.background = "#f44336"; // 红色背景表示错误

          // 在控制台输出错误信息
          console.error(
            "Nyaser Maps Downloader: 下载失败:",
            decodedFilename,
            "错误:",
            error,
          );

          // 10秒后自动隐藏错误提示
          setTimeout(() => {
            removeTaskElement(taskId);
          }, 10000);
        } else {
          // 如果没有找到任务元素，创建一个新的错误提示
          warningDisplay.textContent =
            "下载失败: " + decodedFilename + " - " + error;
          warningDisplay.style.display = "block";
          warningDisplay.style.background = "#f44336"; // 红色背景表示错误

          // 10秒后自动隐藏警告
          setTimeout(() => {
            warningDisplay.style.display = "none";
            warningDisplay.style.background = "#ff9800"; // 恢复橙色背景
          }, 10000);
        }

        // 刷新下载队列
        refreshDownloadQueue().catch((error) => {
          console.error("Nyaser Maps Downloader: 刷新队列失败:", error);
        });
      });

      // 监听下载取消事件
      const cancelDownloadUnlisten = listen("download-canceled", (event) => {
        const { taskId, filename } = event.payload || {};
        if (!taskId) return;

        const task = activeTasks.get(taskId);
        if (task) {
          task.status.textContent = "已取消";
          task.progress.style.background = "#9e9e9e"; // 灰色背景表示取消
          if (task.cancelButton) {
            task.cancelButton.disabled = true;
            task.cancelButton.textContent = "已取消";
          }
          console.log(
            "Nyaser Maps Downloader: 下载已取消:",
            taskId,
            filename || "",
          );

          // 5秒后移除任务显示
          setTimeout(() => {
            removeTaskElement(taskId);
          }, 5000);
        }

        // 刷新下载队列
        refreshDownloadQueue().catch((error) => {
          console.error("Nyaser Maps Downloader: 刷新队列失败:", error);
        });
      });

      // 完成事件监听器注册成功

      // 添加窗口关闭时清理监听器的逻辑
      [
        progressUnlisten,
        downloadedUnlisten,
        extractStartUnlisten,
        extractCompleteUnlisten,
        taskStartUnlisten,
        downloadFailedUnlisten,
        taskAddUnlisten,
        queueUpdateUnlisten,
        dirChangedUnlisten,
        gameDirWarningUnlisten,
        cancelDownloadUnlisten,
      ].forEach((fn) => window.addEventListener("beforeunload", fn));

      // 设置链接拦截
      function setupLinkInterceptor() {
        // 正在设置链接拦截器

        // 1. 拦截全局点击事件，只检查A标签
        document.addEventListener(
          "click",
          async (event) => {
            // 检查A标签
            let target = event.target;
            while (target && target.tagName !== "A") {
              target = target.parentElement;
            }

            if (target && target.tagName === "A") {
              const href = target.getAttribute("href");
              if (href && isDownloadLink(href)) {
                // 阻止默认行为
                event.preventDefault();
                event.stopPropagation();

                // 拦截到下载链接(点击A标签)

                // 直接传递给后端处理
                await handleDownloadLink(href);
              }
            }
          },
          true,
        );

        // 2. 拦截window.open调用
        const originalOpen = window.open;
        window.open = function (url, target, features) {
          if (url && isDownloadLink(url)) {
            // 拦截到下载链接(window.open)
            // 使用立即执行的方式避免阻塞
            (async () => {
              await handleDownloadLink(url);
            })();
            return null; // 阻止原始窗口打开
          }
          return originalOpen.apply(this, arguments);
        };

        // 下载链接拦截器已设置完成
      }

      // 设置拖拽事件监听器
      async function setupDragAndDrop() {
        try {
          const { getCurrentWebview } = window.__TAURI__.webview;
          const webview = getCurrentWebview();

          // 创建拖拽提示元素
          const dragOverlay = document.createElement("div");
          dragOverlay.id = "nmd-drag-overlay";
          dragOverlay.style.cssText = `
            position: fixed;
            top: 0;
            left: 0;
            width: 100%;
            height: 100%;
            background: rgba(76, 175, 80, 0.1);
            border: 4px dashed rgba(76, 175, 80, 0.8);
            display: none;
            justify-content: center;
            align-items: center;
            z-index: 999999;
            pointer-events: none;
            transition: all 0.2s ease;
          `;
          dragOverlay.innerHTML = `
            <div style="
              background: rgba(76, 175, 80, 0.95);
              color: white;
              padding: 20px 40px;
              border-radius: 8px;
              font-size: 18px;
              font-weight: bold;
              box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
            ">
              📦 拖拽压缩包到此处安装
            </div>
          `;
          document.body.appendChild(dragOverlay);

          const unlisten = await webview.onDragDropEvent((event) => {
            if (event.payload.type === "over") {
              console.log("Nyaser Maps Downloader: 用户正在拖拽文件");
              dragOverlay.style.display = "flex";
            } else if (event.payload.type === "drop") {
              console.log(
                "Nyaser Maps Downloader: 用户拖拽了文件:",
                event.payload.paths,
              );

              dragOverlay.style.display = "none";

              const paths = event.payload.paths;
              if (paths && paths.length > 0) {
                const filePath = paths[0];
                handleDroppedFile(filePath);
              }
            } else {
              console.log("Nyaser Maps Downloader: 文件拖拽已取消");
              dragOverlay.style.display = "none";
            }
          });

          console.log("Nyaser Maps Downloader: 拖拽事件监听器已设置");
        } catch (error) {
          console.error(
            "Nyaser Maps Downloader: 设置拖拽事件监听器失败:",
            error,
          );
        }
      }

      // 处理拖拽的文件
      async function handleDroppedFile(filePath) {
        try {
          console.log("Nyaser Maps Downloader: 处理拖拽的文件:", filePath);

          // 提取文件名
          const path = filePath;
          const fileName = path.split(/[\\/]/).pop();

          // 检查文件扩展名，判断是否为压缩包
          const validExtensions = [
            ".7z",
            ".zip",
            ".rar",
            ".tar",
            ".gz",
            ".bz2",
            ".xz",
            ".arj",
            ".cab",
            ".chm",
            ".cpio",
            ".deb",
            ".dmg",
            ".iso",
            ".lzh",
            ".lzma",
            ".msi",
            ".nsis",
            ".rpm",
            ".udf",
            ".wim",
            ".xar",
            ".z",
          ];

          const isArchive = validExtensions.some((ext) =>
            fileName.toLowerCase().endsWith(ext),
          );

          if (!isArchive) {
            warningDisplay.textContent =
              "错误: 不支持的文件格式，请拖拽压缩包文件";
            warningDisplay.style.display = "block";
            warningDisplay.style.background = "rgba(244, 67, 54, 0.9)";

            setTimeout(() => {
              warningDisplay.style.display = "none";
              warningDisplay.style.background = "rgba(255, 152, 0, 0.9)";
            }, 5000);
            return;
          }

          // 监听解压开始事件
          const extractStartUnlisten = listen("extract-start", (event) => {
            const { filename } = event.payload || {};
            if (filename === fileName) {
              warningDisplay.textContent = "正在解压: " + fileName;
              warningDisplay.style.display = "block";
              warningDisplay.style.background = "rgba(76, 175, 80, 0.9)";
            }
          });

          // 监听解压完成事件
          const extractCompleteUnlisten = listen(
            "extract-complete",
            (event) => {
              const { filename, success, message } = event.payload || {};
              if (filename === fileName) {
                if (success) {
                  warningDisplay.textContent = "解压完成: " + fileName;
                  warningDisplay.style.background = "rgba(76, 175, 80, 0.9)";

                  setTimeout(() => {
                    warningDisplay.style.display = "none";
                    warningDisplay.style.background = "rgba(255, 152, 0, 0.9)";
                  }, 3000);
                } else {
                  warningDisplay.textContent = "解压失败: " + message;
                  warningDisplay.style.background = "rgba(244, 67, 54, 0.9)";

                  setTimeout(() => {
                    warningDisplay.style.display = "none";
                    warningDisplay.style.background = "rgba(255, 152, 0, 0.9)";
                  }, 10000);
                }

                // 取消监听器
                extractStartUnlisten();
                extractCompleteUnlisten();
              }
            },
          );

          // 调用后端解压命令
          const result = await window.__TAURI__.core.invoke(
            "extract_dropped_file",
            {
              filePath: filePath,
            },
          );

          console.log("Nyaser Maps Downloader: 解压命令已发送:", result);
        } catch (error) {
          console.error("Nyaser Maps Downloader: 解压文件失败:", error);
          const errorMsg = error.message || JSON.stringify(error);
          warningDisplay.textContent = "错误: 解压失败 - " + errorMsg;
          warningDisplay.style.display = "block";
          warningDisplay.style.background = "rgba(244, 67, 54, 0.9)";

          setTimeout(() => {
            warningDisplay.style.display = "none";
            warningDisplay.style.background = "rgba(255, 152, 0, 0.9)";
          }, 10000);
        }
      }

      // 初始化拦截器
      setupLinkInterceptor();

      // 初始化拖拽功能
      setupDragAndDrop();

      // 导出公共API
      // window.NyaserMapsDownloader = {
      //   handleDownloadLink,
      //   isDownloadLink,
      //   setupLinkInterceptor
      // };

      // 通知后端页面已加载完成
      async function notifyLoaded() {
        try {
          if (window.__TAURI__) {
            await window.__TAURI__.core.invoke("frontend_loaded");
            await window.__TAURI__.core.invoke("deep_link_ready");
            console.log("Nyaser Maps Downloader: 已通知后端前端加载完成");
          }
        } catch (error) {
          console.error(
            "Nyaser Maps Downloader: 通知后端前端加载完成失败:",
            error,
          );
        }
      }

      // 添加加载完成指示器
      function addLoadedIndicator() {
        // 创建加载完成指示器元素
        const loadedIndicator = document.createElement("div");
        loadedIndicator.id = "nmd-loaded-indicator";
        loadedIndicator.style.position = "fixed";
        loadedIndicator.style.bottom = "20px";
        loadedIndicator.style.right = "20px";
        loadedIndicator.style.padding = "8px 16px";
        loadedIndicator.style.background = "rgba(76, 175, 80, 0.9)";
        loadedIndicator.style.color = "white";
        loadedIndicator.style.borderRadius = "4px";
        loadedIndicator.style.boxShadow = "0 2px 8px rgba(0, 0, 0, 0.2)";
        loadedIndicator.style.fontSize = "14px";
        loadedIndicator.style.zIndex = "9999";
        loadedIndicator.style.opacity = "0";
        loadedIndicator.style.transition = "opacity 0.3s ease";
        loadedIndicator.textContent = "插件加载完成";
        loadedIndicator.title = "点击可隐藏";
        loadedIndicator.style.cursor = "pointer";
        loadedIndicator.style.display = "none";

        // 添加点击隐藏事件
        loadedIndicator.addEventListener("click", async () => {
          loadedIndicator.style.opacity = "0";
          setTimeout(() => {
            loadedIndicator.style.display = "none";
          }, 300);
        });

        // 添加到页面
        document.body.appendChild(loadedIndicator);

        // 显示指示器
        setTimeout(() => {
          loadedIndicator.style.display = "block";
          setTimeout(() => {
            loadedIndicator.style.opacity = "1";
            // 5秒后自动隐藏
            setTimeout(() => {
              loadedIndicator.style.opacity = "0";
              setTimeout(() => {
                loadedIndicator.style.display = "none";
              }, 300);
            }, 5000);
          }, 100);
        }, 100);
      }

      // 当所有资源加载完成后执行
      function onAllResourcesLoaded() {
        notifyLoaded();
        addLoadedIndicator();
      }

      // 检测文档加载状态
      if (document.readyState === "complete") {
        onAllResourcesLoaded();
      } else {
        window.addEventListener("load", onAllResourcesLoaded);
      }
    });
  } catch (error) {
    if (typeof console !== "undefined") {
      console.error("Nyaser Maps Downloader: 初始化失败:", error);
    }
  }
})();
