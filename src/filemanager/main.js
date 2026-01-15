const { core, dialog } = window.__TAURI__;
const { invoke, listen } = core;

// 加载文件列表
async function loadFileList() {
  try {
    const fileListElement = document.getElementById("fileList");

    // 保存当前展开的组和滚动位置
    const expandedGroups = new Set();
    document.querySelectorAll(".group-content.expanded").forEach((content) => {
      expandedGroups.add(content.id);
    });
    const scrollTop = fileListElement.scrollTop;

    const groups = await invoke("get_maps");

    if (groups && groups.length > 0) {
      // 获取模板
      const groupItemTemplate =
        document.getElementById("groupItemTemplate").innerHTML;
      const fileItemTemplate =
        document.getElementById("fileItemTemplate").innerHTML;

      // 渲染所有分组
      groups.forEach((group) => {
        const groupName = group.name;
        const files = group.files;
        const groupMounted = group.mounted || false;

        if (files.length > 0) {
          // 计算分组总大小
          const totalSize = files.reduce((sum, file) => sum + file.size, 0);

          // 为分组生成唯一ID
          const groupId = `group-${encodeURIComponent(groupName).replace(
            /[^a-zA-Z0-9]/g,
            "-"
          )}`;
          const groupKey = encodeURIComponent(groupName);

          // 检查分组是否已存在
          let groupElement = document.getElementById(groupId);

          if (!groupElement) {
            // 分组不存在，创建新的
            let groupHtml = groupItemTemplate
              .replace(/\{\{groupKey\}\}/g, groupKey)
              .replace(/\{\{displayGroupName\}\}/g, groupName)
              .replace(/\{\{fileCount\}\}/g, files.length)
              .replace(/\{\{totalSize\}\}/g, formatFileSize(totalSize))
              .replace(/\{\{groupId\}\}/g, groupId)
              .replace(/\{\{groupMounted\}\}/g, groupMounted)
              .replace(
                /\{\{groupMountBtnClass\}\}/g,
                groupMounted ? "unmount" : "mount"
              )
              .replace(
                /\{\{groupMountBtnText\}\}/g,
                groupMounted ? "卸载" : "挂载"
              );

            // 渲染分组内的每个文件
            let filesHtml = "";
            files.forEach((file) => {
              const isMounted = file.mounted || false;
              const fileHtml = fileItemTemplate
                .replace(/\{\{groupKey\}\}/g, groupKey)
                .replace(/\{\{displayName\}\}/g, file.name)
                .replace(/\{\{fileName\}\}/g, file.name)
                .replace(/\{\{fileSize\}\}/g, formatFileSize(file.size))
                .replace(
                  /\{\{mountStatusClass\}\}/g,
                  isMounted ? "mounted" : "unmounted"
                )
                .replace(
                  /\{\{mountStatusText\}\}/g,
                  isMounted ? "已挂载" : "未挂载"
                );
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

            // 创建分组元素并添加到列表
            const tempContainer = document.createElement("div");
            tempContainer.innerHTML = groupHtml;
            const newGroupElement = tempContainer.firstElementChild;
            fileListElement.appendChild(newGroupElement);
          } else {
            // 分组已存在，更新内容
            const groupItem = groupElement.closest(".group-item");

            // 更新分组统计信息
            const statsElement = groupItem.querySelector(".group-stats");
            if (statsElement) {
              statsElement.textContent = `${
                files.length
              } 个文件 · ${formatFileSize(totalSize)}`;
            }

            // 更新挂载按钮
            const mountBtn = groupItem.querySelector(".group-mount-btn");
            if (mountBtn) {
              mountBtn.setAttribute("data-mounted", groupMounted);
              mountBtn.textContent = groupMounted ? "卸载" : "挂载";
              mountBtn.className = `group-mount-btn ${
                groupMounted ? "unmount" : "mount"
              }`;
            }

            // 更新文件列表
            const groupFilesElement = groupElement;
            groupFilesElement.innerHTML = "";

            files.forEach((file) => {
              const isMounted = file.mounted || false;
              const fileHtml = fileItemTemplate
                .replace(/\{\{groupKey\}\}/g, groupKey)
                .replace(/\{\{displayName\}\}/g, file.name)
                .replace(/\{\{fileName\}\}/g, file.name)
                .replace(/\{\{fileSize\}\}/g, formatFileSize(file.size))
                .replace(
                  /\{\{mountStatusClass\}\}/g,
                  isMounted ? "mounted" : "unmounted"
                )
                .replace(
                  /\{\{mountStatusText\}\}/g,
                  isMounted ? "已挂载" : "未挂载"
                );

              const tempDiv = document.createElement("div");
              tempDiv.innerHTML = fileHtml;
              groupFilesElement.appendChild(tempDiv.firstElementChild);
            });
          }
        }
      });

      // 移除不存在的分组
      const existingGroupIds = new Set(
        Array.from(fileListElement.querySelectorAll(".group-content")).map(
          (el) => el.id
        )
      );
      const newGroupIds = new Set(
        groups
          .filter((g) => g.files.length > 0)
          .map(
            (g) =>
              `group-${encodeURIComponent(g.name).replace(
                /[^a-zA-Z0-9]/g,
                "-"
              )}`
          )
      );

      existingGroupIds.forEach((id) => {
        if (!newGroupIds.has(id)) {
          const groupElement = document.getElementById(id);
          if (groupElement) {
            const groupItem = groupElement.closest(".group-item");
            if (groupItem) {
              groupItem.remove();
            }
          }
        }
      });

      // 恢复展开状态
      expandedGroups.forEach((groupId) => {
        const content = document.getElementById(groupId);
        if (content) {
          content.classList.add("expanded");
          const toggle = content
            .closest(".group-item")
            ?.querySelector(".group-toggle");
          if (toggle) {
            toggle.classList.add("expanded");
          }
        }
      });

      // 恢复滚动位置
      fileListElement.scrollTop = scrollTop;

      fileListElement.querySelector(".no-files")?.remove();
      // 显示全选和批量删除按钮
      document.getElementById("selectAllBtn").style.display = "inline-block";
      document.getElementById("batchDeleteBtn").style.display = "inline-block";
    } else {
      fileListElement.innerHTML = '<p class="no-files">没有找到文件</p>';
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
    const groupName = checkbox.getAttribute("data-group");
    updateGroupCheckboxState(groupName);
  });

  updateSelection();
}

// 批量删除文件
async function batchDeleteFiles() {
  const groupCheckboxes = document.querySelectorAll(".group-checkbox:checked");
  const selectedGroups = Array.from(groupCheckboxes).map((cb) =>
    cb.getAttribute("data-group")
  );

  if (selectedGroups.length === 0) {
    await dialog.message("请先选择要删除的分组", {
      kind: "warning",
      title: "提示",
    });
    return;
  }

  const confirmed = await dialog.confirm(
    `确定要删除选中的 ${selectedGroups.length} 个分组中的所有文件吗？`,
    {
      title: "确认批量删除",
      okLabel: "确定",
      cancelLabel: "取消",
    }
  );

  if (confirmed) {
    try {
      let deletedCount = 0;
      for (const groupKey of selectedGroups) {
        // 获取分组内的所有文件项
        const fileItems = document.querySelectorAll(
          `.file-item[data-group="${groupKey}"]`
        );

        // 收集文件名
        const fileNames = [];
        fileItems.forEach((item) => {
          const fileName = item.querySelector(".file-name").textContent;
          fileNames.push(fileName);
        });

        // 逐个删除文件
        for (const fileName of fileNames) {
          await invoke("delete_map_file", {
            groupName: decodeURIComponent(groupKey),
            fileName,
          });
          deletedCount++;
        }
      }

      // 删除后刷新列表
      loadFileList();

      await dialog.message(`已成功删除 ${deletedCount} 个文件！`, {
        kind: "info",
        title: "删除成功",
      });
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
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

// 初始加载文件列表
document.addEventListener("DOMContentLoaded", () => {
  // 使用事件委托处理分组删除按钮点击
  document.getElementById("fileList").addEventListener("click", async (e) => {
    const deleteBtn = e.target.closest(".group-delete-btn");
    if (deleteBtn) {
      const groupKey = deleteBtn.getAttribute("data-group");

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
    }

    // 处理组挂载/卸载按钮点击
    const mountBtn = e.target.closest(".group-mount-btn");
    if (mountBtn) {
      const groupKey = mountBtn.getAttribute("data-group");
      const isMounted = mountBtn.getAttribute("data-mounted") === "true";

      try {
        if (isMounted) {
          await invoke("unmount_group", {
            groupName: decodeURIComponent(groupKey),
          });
        } else {
          await invoke("mount_group", {
            groupName: decodeURIComponent(groupKey),
          });
        }

        // 刷新文件列表
        loadFileList();
      } catch (error) {
        console.error("组挂载/卸载失败:", error);
        const errorMsg = error.message || JSON.stringify(error);
        await dialog.message(
          `组${isMounted ? "卸载" : "挂载"}失败: ${errorMsg}`,
          {
            kind: "error",
            title: "操作失败",
          }
        );
      }
    }

    // 处理分组展开/收起
    const toggle = e.target.closest(".group-toggle");
    if (toggle) {
      e.stopPropagation();
      const groupHeader = toggle.closest(".group-header");
      if (groupHeader) {
        const groupName = groupHeader.getAttribute("data-group");
        const groupContent = document.getElementById(
          `group-${groupName.replace(/[^a-zA-Z0-9]/g, "-")}`
        );
        if (groupContent) {
          if (groupContent.classList.contains("expanded")) {
            groupContent.classList.remove("expanded");
            toggle.classList.remove("expanded");
          } else {
            groupContent.classList.add("expanded");
            toggle.classList.add("expanded");
          }
        }
      }
    }
  });

  // 使用事件委托处理分组复选框变化
  document.getElementById("fileList").addEventListener("change", (e) => {
    if (e.target.classList.contains("group-checkbox")) {
      const groupName = e.target.getAttribute("data-group");
      const isChecked = e.target.checked;

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
    }
  });

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
    if (dataDirElement) {
      if (config.nmd_data) {
        dataDirElement.textContent = config.nmd_data;
      } else {
        dataDirElement.textContent = "未配置";
      }
    }
  } catch (error) {
    console.error("加载数据目录失败:", error);
  }
}

async function changeDataDir() {
  try {
    const newDir = await invoke("show_directory_dialog");
    if (newDir) {
      const dataDirElement = document.getElementById("dataDir");
      if (dataDirElement) {
        dataDirElement.textContent = newDir;
      }
      // 刷新文件列表
      loadFileList();
    }
  } catch (error) {
    console.error("修改数据目录失败:", error);
    const errorMsg = error.message || JSON.stringify(error);
    await dialog.message(`修改数据目录失败: ${errorMsg}`, {
      kind: "error",
      title: "操作失败",
    });
  }
}

document.addEventListener("DOMContentLoaded", () => {
  loadDataDir();

  // 添加修改目录按钮事件
  document
    .getElementById("changeDirBtn")
    .addEventListener("click", changeDataDir);
});
