const { core, dialog } = window.__TAURI__;
const { invoke, listen } = core;

// 加载文件列表
async function loadFileList() {
  try {
    const fileListElement = document.getElementById("fileList");
    fileListElement.innerHTML = '<p class="no-files">正在加载文件列表...</p>';

    const groups = await invoke("get_maps");

    if (groups && groups.length > 0) {
      let html = "";

      // 渲染所有分组
      groups.forEach((group) => {
        const groupName = group.name;
        const files = group.files;

        if (files.length > 0) {
          // 计算分组总大小
          const totalSize = files.reduce((sum, file) => sum + file.size, 0);

          // 为分组生成唯一ID
          const groupId = `group-${encodeURIComponent(groupName).replace(
            /[^a-zA-Z0-9]/g,
            "-"
          )}`;

          // 使用模板渲染分组项
          const groupItemTemplate =
            document.getElementById("groupItemTemplate").innerHTML;
          const fileItemTemplate =
            document.getElementById("fileItemTemplate").innerHTML;

          // 替换模板中的变量
          let groupHtml = groupItemTemplate
            .replace(/\{\{groupKey\}\}/g, encodeURIComponent(groupName))
            .replace(/\{\{displayGroupName\}\}/g, groupName)
            .replace(/\{\{fileCount\}\}/g, files.length)
            .replace(/\{\{totalSize\}\}/g, formatFileSize(totalSize))
            .replace(/\{\{groupId\}\}/g, groupId);

          // 渲染分组内的每个文件
          let filesHtml = "";
          files.forEach((file) => {
            const fileHtml = fileItemTemplate
              .replace(/\{\{groupKey\}\}/g, encodeURIComponent(groupName))
              .replace(/\{\{displayName\}\}/g, file.name)
              .replace(/\{\{fileSize\}\}/g, formatFileSize(file.size));
            filesHtml += fileHtml;
          });

          // 将文件HTML插入到分组HTML中
          const tempDiv = document.createElement("div");
          tempDiv.innerHTML = groupHtml;
          const groupFilesElement = tempDiv.querySelector(".group-files");
          if (groupFilesElement) {
            groupFilesElement.innerHTML = filesHtml;
          }
          groupHtml = tempDiv.innerHTML;

          // 将分组HTML添加到总HTML中
          html += groupHtml;
        }
      });

      fileListElement.innerHTML = html;

      // 添加分组删除按钮事件监听
      document
        .querySelectorAll(".group-delete-btn[data-group]")
        .forEach((btn) => {
          btn.addEventListener("click", async (e) => {
            const groupKey = e.target.getAttribute("data-group");

            // 直接获取分组内的所有文件项
            const fileItems = document.querySelectorAll(
              `.file-item[data-group="${groupKey}"]`
            );

            // 确保获取了所有文件
            if (fileItems.length === 0) {
              console.error("未找到分组中的文件");
              return;
            }

            // 收集文件名
            const fileNames = [];

            // 遍历文件项来构建文件名列表
            fileItems.forEach((item) => {
              const fileName = item.querySelector(".file-name").textContent;
              fileNames.push(fileName);
            });

            // 获取分组的显示名称（用于对话框标题）
            const groupHeader = document.querySelector(
              `.group-header[data-group="${groupKey}"]`
            );
            const groupDisplayName = groupHeader
              ? groupHeader.querySelector(".group-info").textContent
              : "未命名组";

            // 显示确认对话框
            const confirmed = await dialog.confirm(
              fileNames.length > 1
                ? `确定要删除 ${groupDisplayName} 分组中的所有 ${fileNames.length} 个文件吗？`
                : `确定要删除 "${groupDisplayName}" 吗？`,
              {
                title: fileNames.length > 1 ? "确认删除分组" : "确认删除文件",
                okLabel: "确定",
                cancelLabel: "取消",
              }
            );

            if (confirmed) {
              try {
                // 逐个删除文件
                for (const fileName of fileNames) {
                  await invoke("delete_map_file", {
                    groupName: decodeURIComponent(groupKey),
                    fileName,
                  });
                }

                // 删除后刷新列表
                loadFileList();

                // 显示删除成功提示
                if (fileNames.length > 1) {
                  await dialog.message(
                    `已成功删除 ${groupDisplayName} 分组中的 ${fileNames.length} 个文件！`,
                    {
                      kind: "info",
                      title: "删除成功",
                    }
                  );
                } else {
                  await dialog.message(`已成功删除文件！`, {
                    kind: "info",
                    title: "删除成功",
                  });
                }
              } catch (error) {
                console.error("删除文件失败:", error);
                const errorMsg = error.message || JSON.stringify(error);
                await dialog.message(`删除文件失败: ${errorMsg}`, {
                  kind: "error",
                  title: "删除失败",
                });
              }
            }
          });
        });

      // 添加分组复选框事件监听
      document.querySelectorAll(".group-checkbox").forEach((checkbox) => {
        checkbox.addEventListener("change", function () {
          const groupName = this.getAttribute("data-group");
          const isChecked = this.checked;

          // 更新组头样式
          const groupHeader = document.querySelector(
            `.group-header[data-group="${groupName}"]`
          );
          if (groupHeader) {
            if (isChecked) {
              groupHeader.classList.add("selected");
            } else {
              groupHeader.classList.remove("selected");
            }
          }

          updateSelection();
        });
      });

      // 直接为toggle添加点击事件，这是更直接且可靠的实现方式
      document.querySelectorAll(".group-toggle").forEach((toggle) => {
        toggle.onclick = function (e) {
          e.stopPropagation();
          const groupHeader = this.closest(".group-header");
          if (groupHeader) {
            const groupName = groupHeader.getAttribute("data-group");
            const groupContent = document.getElementById(
              `group-${groupName.replace(/[^a-zA-Z0-9]/g, "-")}`
            );
            if (groupContent) {
              if (groupContent.classList.contains("expanded")) {
                groupContent.classList.remove("expanded");
                this.classList.remove("expanded");
              } else {
                groupContent.classList.add("expanded");
                this.classList.add("expanded");
              }
            }
          }
        };
      });

      // 默认收起所有分组
      // 为每个分组内容添加transition动画效果
      document.querySelectorAll(".group-content").forEach((content) => {
        content.style.transition = "max-height 0.3s ease, opacity 0.3s ease";
      });

      // 显示全选按钮
      document.getElementById("selectAllBtn").style.display = "inline-block";
      document.getElementById("batchDeleteBtn").style.display = "inline-block";
    } else {
      fileListElement.innerHTML =
        '<p class="no-files">没有找到文件</p>';
      // 隐藏全选和批量删除按钮
      document.getElementById("selectAllBtn").style.display = "none";
      document.getElementById("batchDeleteBtn").style.display = "none";
    }
  } catch (error) {
    console.error("加载文件列表失败:", error);
    document.getElementById(
      "fileList"
    ).innerHTML = `<p class="no-files" style="color: red;">加载文件列表失败: ${error.message}</p>`;
    // 隐藏全选和批量删除按钮
    document.getElementById("selectAllBtn").style.display = "none";
    document.getElementById("batchDeleteBtn").style.display = "none";
  }
}

// 更新分组复选框状态（仅基于组复选框自身状态）
function updateGroupCheckboxState(groupName) {
  const groupCheckbox = document.querySelector(
    `.group-checkbox[data-group="${groupName}"]`
  );
  const groupHeader = document.querySelector(
    `.group-header[data-group="${groupName}"]`
  );

  // 确保所有元素都存在
  if (groupCheckbox && groupHeader) {
    if (groupCheckbox.checked) {
      groupHeader.classList.add("selected");
    } else {
      groupHeader.classList.remove("selected");
    }
  }
}

// 更新选择状态
function updateSelection() {
  const groupCheckboxes = document.querySelectorAll(".group-checkbox");
  const selectedCount = Array.from(groupCheckboxes).filter(
    (cb) => cb.checked
  ).length;
  const batchDeleteBtn = document.getElementById("batchDeleteBtn");
  const selectAllBtn = document.getElementById("selectAllBtn");

  // 更新批量删除按钮状态
  batchDeleteBtn.disabled = selectedCount === 0;

  // 更新全选按钮文本
  if (selectedCount === 0) {
    selectAllBtn.textContent = "全选";
  } else if (selectedCount === groupCheckboxes.length) {
    selectAllBtn.textContent = "取消全选";
  } else {
    selectAllBtn.textContent = "全选";
  }

  // 更新分组头部的选中样式
  document.querySelectorAll(".group-header").forEach((header) => {
    const groupCheckbox = header.querySelector(".group-checkbox");
    if (groupCheckbox && groupCheckbox.checked) {
      header.classList.add("selected");
    } else {
      header.classList.remove("selected");
    }
  });
}

// 全选/取消全选
function toggleSelectAll() {
  const groupCheckboxes = document.querySelectorAll(".group-checkbox");
  const selectAllBtn = document.getElementById("selectAllBtn");
  const isAllSelected = Array.from(groupCheckboxes).every((cb) => cb.checked);

  groupCheckboxes.forEach((checkbox) => {
    checkbox.checked = !isAllSelected;
    updateGroupCheckboxState(checkbox.getAttribute("data-group"));
  });

  selectAllBtn.textContent = isAllSelected ? "全选" : "取消全选";
  updateSelection();
}

// 批量删除
async function batchDeleteFiles() {
  const groupCheckboxes = document.querySelectorAll(".group-checkbox:checked");
  let selectedGroups = [];
  let allFileNames = [];

  if (groupCheckboxes.length === 0) return;

  // 收集选中的组信息和文件名
  groupCheckboxes.forEach((groupCheckbox) => {
    const groupKey = groupCheckbox.getAttribute("data-group");
    const groupHeader = document.querySelector(
      `.group-header[data-group="${groupKey}"]`
    );
    const groupDisplayName = groupHeader
      ? groupHeader.querySelector(".group-info").textContent
      : "未命名组";

    selectedGroups.push(groupDisplayName);

    const fileItems = document.querySelectorAll(
      `.file-item[data-group="${groupKey}"]`
    );
    fileItems.forEach((item) => {
      const fileName = item.querySelector(".file-name").textContent;
      allFileNames.push({ groupName: decodeURIComponent(groupKey), fileName });
    });
  });

  // 显示确认对话框
  const confirmed = await dialog.confirm(
    `确定要删除以下 ${selectedGroups.length} 个组吗？\n${selectedGroups.join(
      "\n"
    )}`,
    {
      title: "确认批量删除",
      okLabel: "确定",
      cancelLabel: "取消",
    }
  );

  if (confirmed) {
      try {
        // 实际执行删除操作
        for (const { groupName, fileName } of allFileNames) {
          await invoke("delete_map_file", { groupName, fileName });
        }

      // 删除后刷新列表
      loadFileList();

      // 显示删除成功提示
      await dialog.message(
        `已成功删除 ${selectedGroups.length} 个组中的 ${allFileNames.length} 个文件！`,
        {
          kind: "info",
          title: "删除成功",
        }
      );
    } catch (error) {
      console.error("批量删除文件失败:", error);
      const errorMsg = error.message || JSON.stringify(error);
      await dialog.message(`批量删除文件失败: ${errorMsg}`, {
        kind: "error",
        title: "删除失败",
      });
    }
  }
}

// 格式化文件大小
function formatFileSize(bytes) {
  if (bytes === 0) return "0 Bytes";
  const k = 1024;
  const sizes = ["Bytes", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

// 添加键盘Delete键支持
document.addEventListener("keydown", async (e) => {
  // 当按下Delete键且有选中的组时执行批量删除
  if (e.key === "Delete" || e.key === "Backspace") {
    const groupCheckboxes = document.querySelectorAll(
      ".group-checkbox:checked"
    );
    if (groupCheckboxes.length > 0) {
      e.preventDefault();
      await batchDeleteFiles();
    }
  }
});

// 初始加载文件列表
document.addEventListener("DOMContentLoaded", () => {
  // 刷新按钮事件
  document.getElementById("refreshBtn").addEventListener("click", loadFileList);

  // 全选按钮事件
  document
    .getElementById("selectAllBtn")
    .addEventListener("click", toggleSelectAll);

  // 批量删除按钮事件
  document
    .getElementById("batchDeleteBtn")
    .addEventListener("click", batchDeleteFiles);
});

// 监听窗口show事件，当窗口从隐藏状态重新显示时自动刷新文件列表
if (window.__TAURI__ && window.__TAURI__.event) {
  // 监听来自main窗口的refresh-file-list自定义事件
  window.__TAURI__.event.listen("refresh-file-list", () => {
    console.log(
      "Nyaser Maps Downloader: 收到刷新文件列表事件，开始刷新文件列表"
    );
    loadFileList();
  });
}

// 数据存储目录管理
async function loadDataDir() {
  try {
    const config = await invoke("read_config", { configName: "config.json" });
    const dataDirElement = document.getElementById("dataDir");
    if (config && config.nmd_data) {
      dataDirElement.textContent = config.nmd_data;
    } else {
      dataDirElement.textContent = "未配置";
    }
  } catch (error) {
    console.error("加载数据存储目录失败:", error);
    document.getElementById("dataDir").textContent = "加载失败";
  }
}

// 按钮锁定状态
let isChangingDir = false;

async function changeDataDir() {
  // 检查是否已经在处理中
  if (isChangingDir) return;

  // 获取按钮元素
  const changeBtn = document.getElementById("changeDirBtn");

  try {
    // 锁定按钮
    isChangingDir = true;
    changeBtn.disabled = true;
    changeBtn.textContent = "处理中...";

    // 调用目录选择对话框
    const selectedDir = await invoke("show_directory_dialog");

    // 保存新的目录配置
    const config = await invoke("read_config", { configName: "config.json" });
    await invoke("write_config", {
      configName: "config.json",
      config: { ...config, nmd_data: selectedDir },
    });

    // 更新显示
    document.getElementById("dataDir").textContent = selectedDir;
  } catch (error) {
    console.error("更改数据存储目录失败:", error);
  } finally {
    // 无论成功还是失败，都解锁按钮
    isChangingDir = false;
    changeBtn.disabled = false;
    changeBtn.textContent = "修改目录";
  }
}

// 初始化数据存储目录显示
document.addEventListener("DOMContentLoaded", () => {
  loadDataDir();

  // 添加修改目录按钮事件
  document
    .getElementById("changeDirBtn")
    .addEventListener("click", changeDataDir);
});
