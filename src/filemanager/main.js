const {
  core: { invoke },
  dialog,
  event: { listen },
} = window.__TAURI__;

function naturalSortCompare(a, b) {
  return a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });
}

// 筛选状态变量
let currentFilters = {
  category: "all",
  mountStatus: "all",
  search: "",
};

/**
 * 对 DOM 中的分组元素进行排序
 * @param {HTMLElement} container - 包含分组元素的容器
 * @description 根据存储在 data-* 属性中的排序数据对 DOM 元素进行排序
 */
function sortGroupElements(container) {
  // 获取所有分组元素
  const groupElements = Array.from(container.querySelectorAll(".group-item"));

  // 对元素进行排序
  groupElements.sort((a, b) => {
    // 获取排序数据
    const scoreA = parseFloat(a.dataset.searchScore);
    const scoreB = parseFloat(b.dataset.searchScore);

    // 检查是否有搜索分数（只有在搜索时才会有分数）
    const hasScoreA = !isNaN(scoreA);
    const hasScoreB = !isNaN(scoreB);

    // 如果两个都有搜索分数，则按分数排序
    if (hasScoreA && hasScoreB) {
      return scoreA - scoreB;
    }

    const typeA = a.dataset.category || "unsorted";
    const typeB = b.dataset.category || "unsorted";

    if (typeA !== typeB) {
      return typeA.localeCompare(typeB);
    }

    // 分类相同则按清理后的名称排序
    return naturalSortCompare(a.dataset.cleanName, b.dataset.cleanName);
  });

  // 重新排列 DOM 元素
  groupElements.forEach((element) => {
    container.appendChild(element);
  });
}

/**
 * 获取并处理分组数据
 * @returns {Promise<Array>} 处理后的分组数组
 * @description 从后端获取分组数据，并添加分类和清理名称信息
 * @returns {Array<{name: string, files: Array, mounted: boolean, category: string, cleanName: string}>} 处理后的分组数组
 */
async function getMaps() {
  // 使用 ??= 实现 inflight 缓存，避免重复请求
  return (getMaps.p ??= invoke("get_maps").then((groups) => {
    // 为每个分组添加分类信息，并去掉相应的前缀
    groups.forEach((group) => {
      let groupName = group.name;
      // 去掉相应的前缀
      if (groupName.startsWith("【Map】")) {
        group.category = "kitasoda";
        group.cleanName = groupName.replace("【Map】", "").trim();
      } else if (/^[A-Za-z]-/.test(groupName)) {
        group.category = "ssdraid0";
        group.cleanName = groupName.replace(/^([A-Za-z]-)/, "").trim();
      } else {
        group.category = "unsorted";
        group.cleanName = groupName.trim();
      }
    });
    return groups;
  }));
}

/**
 * 加载文件列表
 * @param {number} clearMode - 清空模式：0=不清空列表，1=清空列表，2=清空列表并重新加载
 * @description 加载并渲染文件列表，根据 clearMode 参数决定是否清空现有列表
 */
async function loadFileList(clearMode = 0) {
  try {
    const fileListElement = document.getElementById("fileList");

    // 保存当前展开的组和滚动位置
    const expandedGroups = new Set();
    document.querySelectorAll(".group-content.expanded").forEach((content) => {
      expandedGroups.add(content.id);
    });
    const scrollTop = fileListElement.scrollTop;

    switch (clearMode) {
      case 2:
        fileListElement.innerHTML = `<p class="no-files">正在加载...</p>`;
      case 1:
        delete getMaps.p;
      default:
        break;
    }

    // 获取处理后的分组数据
    /** @type {Array<{name: string, files: Array, mounted: boolean, category: string, cleanName: string}>} */
    const groups = await getMaps();

    if (groups && groups.length > 0) {
      // 根据筛选条件过滤分组
      let filteredGroups = groups.filter((group) => {
        // 分类筛选
        if (
          currentFilters.category !== "all" &&
          group.category !== currentFilters.category
        ) {
          return false;
        }

        // 挂载状态筛选
        if (currentFilters.mountStatus !== "all") {
          const isMounted = group.mounted || false;
          if (currentFilters.mountStatus === "mounted" && !isMounted) {
            return false;
          }
          if (currentFilters.mountStatus === "unmounted" && isMounted) {
            return false;
          }
        }

        return true;
      });

      // 使用 Fuse.js 进行搜索
      if (currentFilters.search) {
        // 配置 Fuse.js
        const fuse = new Fuse(filteredGroups, {
          keys: ["cleanName"],
          includeScore: true,
          distance: 100,
          minMatchCharLength: 1,
        });

        // 执行搜索
        const searchResults = fuse.search(currentFilters.search);
        // 提取匹配的分组，并添加匹配分数
        filteredGroups = searchResults.map((result) => {
          result.item.searchScore = result.score;
          return result.item;
        });
      } else {
        // 当搜索关键词被清空时，确保所有分组都没有 searchScore 属性
        filteredGroups.forEach((group) => {
          delete group.searchScore;
        });
      }

      // 获取模板
      const groupItemTemplate =
        document.getElementById("groupItemTemplate").innerHTML;
      const fileItemTemplate =
        document.getElementById("fileItemTemplate").innerHTML;

      // 渲染所有分组
      filteredGroups.forEach((group) => {
        const groupName = group.name;
        const files = group.files;
        const groupMounted = group.mounted || false;

        if (files.length > 0) {
          // 计算分组总大小
          const totalSize = files.reduce((sum, file) => sum + file.size, 0);

          // 为分组生成唯一ID
          const groupId = `group-${encodeURIComponent(groupName).replace(
            /[^a-zA-Z0-9]/g,
            "-",
          )}`;
          const groupKey = encodeURIComponent(groupName);

          // 检查分组是否已存在
          let groupElement = document.getElementById(groupId);

          if (!groupElement) {
            // 分组不存在，创建新的
            let groupHtml = groupItemTemplate
              .replace(/\{\{groupKey\}\}/g, groupKey)
              .replace(
                /\{\{displayGroupName\}\}/g,
                group.cleanName || groupName,
              )
              .replace(/\{\{fileCount\}\}/g, files.length)
              .replace(/\{\{totalSize\}\}/g, formatFileSize(totalSize))
              .replace(/\{\{category\}\}/g, group.category || "unsorted")
              .replace(/\{\{groupId\}\}/g, groupId)
              .replace(/\{\{groupMounted\}\}/g, groupMounted)
              .replace(
                /\{\{groupMountBtnClass\}\}/g,
                groupMounted ? "unmount" : "mount",
              )
              .replace(
                /\{\{groupMountBtnText\}\}/g,
                groupMounted ? "卸载" : "挂载",
              );

            // 渲染分组内的每个文件
            let filesHtml = "";
            files
              .sort((a, b) => naturalSortCompare(a.name, b.name))
              .forEach((file) => {
                const isMounted = file.mounted || false;
                const fileHtml = fileItemTemplate
                  .replace(/\{\{groupKey\}\}/g, groupKey)
                  .replace(/\{\{displayName\}\}/g, file.name)
                  .replace(/\{\{fileName\}\}/g, file.name)
                  .replace(/\{\{fileSize\}\}/g, formatFileSize(file.size))
                  .replace(
                    /\{\{mountStatusClass\}\}/g,
                    isMounted ? "mounted" : "unmounted",
                  )
                  .replace(
                    /\{\{mountStatusText\}\}/g,
                    isMounted ? "已挂载" : "未挂载",
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
            // 存储排序所需的数据
            if (group.searchScore !== undefined) {
              newGroupElement.dataset.searchScore = group.searchScore;
            }
            newGroupElement.dataset.category = group.category;
            newGroupElement.dataset.cleanName = group.cleanName;
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
            const groupFilesElement =
              groupElement.querySelector(".group-files");
            if (groupFilesElement) {
              groupFilesElement.innerHTML = "";

              files
                .sort((a, b) => naturalSortCompare(a.name, b.name))
                .forEach((file) => {
                  const isMounted = file.mounted || false;
                  const fileHtml = fileItemTemplate
                    .replace(/\{\{groupKey\}\}/g, groupKey)
                    .replace(/\{\{displayName\}\}/g, file.name)
                    .replace(/\{\{fileName\}\}/g, file.name)
                    .replace(/\{\{fileSize\}\}/g, formatFileSize(file.size))
                    .replace(
                      /\{\{mountStatusClass\}\}/g,
                      isMounted ? "mounted" : "unmounted",
                    )
                    .replace(
                      /\{\{mountStatusText\}\}/g,
                      isMounted ? "已挂载" : "未挂载",
                    );

                  const tempDiv = document.createElement("div");
                  tempDiv.innerHTML = fileHtml;
                  groupFilesElement.appendChild(tempDiv.firstElementChild);
                });
            }

            // 更新排序所需的数据
            if (group.searchScore !== undefined) {
              groupItem.dataset.searchScore = group.searchScore;
            } else {
              // 当搜索关键词被清空时，移除 data-search-score 属性
              delete groupItem.dataset.searchScore;
            }
            groupItem.dataset.category = group.category;
            groupItem.dataset.cleanName = group.cleanName;
          }
        }
      });

      // 移除不存在的分组
      const existingGroupIds = new Set(
        Array.from(fileListElement.querySelectorAll(".group-content")).map(
          (el) => el.id,
        ),
      );
      const newGroupIds = new Set(
        filteredGroups
          .filter((g) => g.files.length > 0)
          .map(
            (g) =>
              `group-${encodeURIComponent(g.name).replace(
                /[^a-zA-Z0-9]/g,
                "-",
              )}`,
          ),
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

      // 对 DOM 元素进行排序
      sortGroupElements(fileListElement);

      // 恢复滚动位置
      fileListElement.scrollTop = scrollTop;

      fileListElement.querySelector(".no-files")?.remove();
      // 显示全选和批量删除按钮
      document.getElementById("selectAllBtn").style.display = "inline-block";
      document.getElementById("batchDeleteBtn").style.display = "inline-block";
      document.getElementById("batchMountBtn").style.display = "inline-block";
      document.getElementById("batchUnmountBtn").style.display = "inline-block";
    } else {
      fileListElement.innerHTML = '<p class="no-files">没有找到文件</p>';
      // 隐藏全选和批量删除按钮
      document.getElementById("selectAllBtn").style.display = "none";
      document.getElementById("batchDeleteBtn").style.display = "none";
      document.getElementById("batchMountBtn").style.display = "none";
      document.getElementById("batchUnmountBtn").style.display = "none";
    }
  } catch (error) {
    console.error("加载文件列表失败:", error);
    document.getElementById("fileList").innerHTML =
      `<p class="no-files" style="color: red;">加载文件列表失败: ${error.message}</p>`;
    // 隐藏全选和批量删除按钮
    document.getElementById("selectAllBtn").style.display = "none";
    document.getElementById("batchDeleteBtn").style.display = "none";
    document.getElementById("batchMountBtn").style.display = "none";
    document.getElementById("batchUnmountBtn").style.display = "none";
  }
}

// 应用筛选
function applyFilters() {
  currentFilters.category = document.getElementById("categoryFilter").value;
  currentFilters.mountStatus =
    document.getElementById("mountStatusFilter").value;
  currentFilters.search = document
    .getElementById("searchBox")
    .value.trim()
    .toLowerCase();
  loadFileList();
}

// 批量挂载选中的分组
async function batchMountGroups() {
  // 禁用批量操作按钮
  const batchMountBtn = document.getElementById("batchMountBtn");
  const batchUnmountBtn = document.getElementById("batchUnmountBtn");
  const batchDeleteBtn = document.getElementById("batchDeleteBtn");
  batchMountBtn.disabled = true;
  batchUnmountBtn.disabled = true;
  batchDeleteBtn.disabled = true;

  const groupCheckboxes = document.querySelectorAll(".group-checkbox:checked");
  const selectedGroups = Array.from(groupCheckboxes).map((cb) =>
    cb.getAttribute("data-group"),
  );

  if (selectedGroups.length === 0) {
    await dialog.message("请先选择要挂载的分组", {
      kind: "warning",
      title: "提示",
    });
    // 重新启用批量操作按钮
    batchMountBtn.disabled = false;
    batchUnmountBtn.disabled = false;
    batchDeleteBtn.disabled = false;
    return;
  }

  try {
    let mountedCount = 0;
    for (const groupKey of selectedGroups) {
      const groupName = decodeURIComponent(groupKey);

      // 检查是否已挂载
      const groupHeader = document.querySelector(
        `.group-header[data-group="${groupKey}"]`,
      );
      const mountBtn = groupHeader?.querySelector(".group-mount-btn");
      const isMounted = mountBtn?.getAttribute("data-mounted") === "true";

      if (!isMounted) {
        try {
          await invoke("mount_group", {
            groupName,
          });
          mountedCount++;
        } catch (error) {
          console.warn(`挂载分组 ${groupName} 失败:`, error);
        }
      }
    }

    // 刷新列表
    await loadFileList(1);

    if (mountedCount > 0) {
      await dialog.message(`已成功挂载 ${mountedCount} 个分组！`, {
        kind: "info",
        title: "挂载成功",
      });
    }
  } catch (error) {
    console.error("批量挂载失败:", error);
    const errorMsg = error.message || JSON.stringify(error);
    await dialog.message(`批量挂载失败: ${errorMsg}`, {
      kind: "error",
      title: "挂载失败",
    });
  } finally {
    // 重新启用批量操作按钮
    batchMountBtn.disabled = false;
    batchUnmountBtn.disabled = false;
    batchDeleteBtn.disabled = false;
  }
}

// 批量卸载选中的分组
async function batchUnmountGroups() {
  // 禁用批量操作按钮
  const batchMountBtn = document.getElementById("batchMountBtn");
  const batchUnmountBtn = document.getElementById("batchUnmountBtn");
  const batchDeleteBtn = document.getElementById("batchDeleteBtn");
  batchMountBtn.disabled = true;
  batchUnmountBtn.disabled = true;
  batchDeleteBtn.disabled = true;

  const groupCheckboxes = document.querySelectorAll(".group-checkbox:checked");
  const selectedGroups = Array.from(groupCheckboxes).map((cb) =>
    cb.getAttribute("data-group"),
  );

  if (selectedGroups.length === 0) {
    await dialog.message("请先选择要卸载的分组", {
      kind: "warning",
      title: "提示",
    });
    // 重新启用批量操作按钮
    batchMountBtn.disabled = false;
    batchUnmountBtn.disabled = false;
    batchDeleteBtn.disabled = false;
    return;
  }

  try {
    let unmountedCount = 0;
    for (const groupKey of selectedGroups) {
      const groupName = decodeURIComponent(groupKey);

      // 检查是否已挂载
      const groupHeader = document.querySelector(
        `.group-header[data-group="${groupKey}"]`,
      );
      const mountBtn = groupHeader?.querySelector(".group-mount-btn");
      const isMounted = mountBtn?.getAttribute("data-mounted") === "true";

      if (isMounted) {
        try {
          await invoke("unmount_group", {
            groupName,
          });
          unmountedCount++;
        } catch (error) {
          console.warn(`卸载分组 ${groupName} 失败:`, error);
        }
      }
    }

    // 刷新列表
    await loadFileList(1);

    if (unmountedCount > 0) {
      await dialog.message(`已成功卸载 ${unmountedCount} 个分组！`, {
        kind: "info",
        title: "卸载成功",
      });
    }
  } catch (error) {
    console.error("批量卸载失败:", error);
    const errorMsg = error.message || JSON.stringify(error);
    await dialog.message(`批量卸载失败: ${errorMsg}`, {
      kind: "error",
      title: "卸载失败",
    });
  } finally {
    // 重新启用批量操作按钮
    batchMountBtn.disabled = false;
    batchUnmountBtn.disabled = false;
    batchDeleteBtn.disabled = false;
  }
}

// 更新分组复选框状态（仅基于组复选框自身状态）
function updateGroupCheckboxState(groupName) {
  const groupCheckbox = document.querySelector(
    `.group-checkbox[data-group="${groupName}"]`,
  );
  const groupHeader = document.querySelector(
    `.group-header[data-group="${groupName}"]`,
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
    (cb) => cb.checked,
  ).length;
  const batchDeleteBtn = document.getElementById("batchDeleteBtn");
  const batchMountBtn = document.getElementById("batchMountBtn");
  const batchUnmountBtn = document.getElementById("batchUnmountBtn");
  const selectAllBtn = document.getElementById("selectAllBtn");

  // 更新批量删除按钮状态
  batchDeleteBtn.disabled = selectedCount === 0;

  // 更新批量挂载/卸载按钮状态
  batchMountBtn.disabled = selectedCount === 0;
  batchUnmountBtn.disabled = selectedCount === 0;

  // 更新全选按钮文本
  if (selectedCount && selectedCount === groupCheckboxes.length) {
    selectAllBtn.textContent = "反选";
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
  // 禁用批量操作按钮
  const batchMountBtn = document.getElementById("batchMountBtn");
  const batchUnmountBtn = document.getElementById("batchUnmountBtn");
  const batchDeleteBtn = document.getElementById("batchDeleteBtn");
  batchMountBtn.disabled = true;
  batchUnmountBtn.disabled = true;
  batchDeleteBtn.disabled = true;

  const groupCheckboxes = document.querySelectorAll(".group-checkbox:checked");
  const selectedGroups = Array.from(groupCheckboxes).map((cb) =>
    cb.getAttribute("data-group"),
  );

  if (selectedGroups.length === 0) {
    await dialog.message("请先选择要删除的分组", {
      kind: "warning",
      title: "提示",
    });
    // 重新启用批量操作按钮
    batchMountBtn.disabled = false;
    batchUnmountBtn.disabled = false;
    batchDeleteBtn.disabled = false;
    return;
  }

  const confirmed = await dialog.confirm(
    `确定要删除选中的 ${selectedGroups.length} 个分组吗？`,
    {
      title: "确认批量删除",
      okLabel: "确定",
      cancelLabel: "取消",
    },
  );

  if (confirmed) {
    try {
      let deletedCount = 0;
      for (const groupKey of selectedGroups) {
        const groupName = decodeURIComponent(groupKey);

        // 先卸载分组
        try {
          await invoke("unmount_group", {
            groupName,
          });
        } catch (error) {
          console.warn(`卸载分组 ${groupName} 失败:`, error);
        }

        // 删除分组
        await invoke("delete_group", {
          groupName,
        });
        deletedCount++;
      }

      // 删除后刷新列表
      await loadFileList(1);

      await dialog.message(`已成功删除 ${deletedCount} 个分组！`, {
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
    } finally {
      // 重新启用批量操作按钮
      batchMountBtn.disabled = false;
      batchUnmountBtn.disabled = false;
      batchDeleteBtn.disabled = false;
    }
  } else {
    // 取消删除操作，重新启用批量操作按钮
    batchMountBtn.disabled = false;
    batchUnmountBtn.disabled = false;
    batchDeleteBtn.disabled = false;
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

// 修改数据目录
async function changeDataDir() {
  try {
    document.getElementById("changeDirBtn").setAttribute("disabled", "");
    const newDir = await invoke("show_directory_dialog");
    if (newDir) {
      const dataDirElement = document.getElementById("dataDir");
      if (dataDirElement) {
        dataDirElement.textContent = newDir;
      }
      // 保存配置
      const config = await invoke("read_config", {
        configName: "config.json",
      });
      await invoke("write_config", {
        configName: "config.json",
        config: { ...config, nmd_data: newDir },
      });
      // 刷新文件列表
      loadFileList(2);
    }
  } catch (error) {
    console.error("修改数据目录失败:", error);
  } finally {
    document.getElementById("changeDirBtn").removeAttribute("disabled");
  }
}

// 初始加载文件列表
!(async function () {
  await loadDataDir();

  // 添加修改目录按钮事件
  document
    .getElementById("changeDirBtn")
    .addEventListener("click", changeDataDir);

  // 使用事件委托处理分组删除按钮点击
  document.getElementById("fileList").addEventListener("click", async (e) => {
    const deleteBtn = e.target.closest(".group-delete-btn");
    if (deleteBtn) {
      const groupKey = deleteBtn.getAttribute("data-group");

      // 获取分组的显示名称（用于对话框标题）
      const groupHeader = document.querySelector(
        `.group-header[data-group="${groupKey}"]`,
      );
      const groupDisplayName = groupHeader
        ? groupHeader.querySelector(".group-info").textContent
        : "未命名组";

      // 显示确认对话框
      const confirmed = await dialog.confirm(
        `确定要删除 ${groupDisplayName} 分组吗？`,
        {
          title: "确认删除分组",
          okLabel: "确定",
          cancelLabel: "取消",
        },
      );

      if (confirmed) {
        try {
          const groupName = decodeURIComponent(groupKey);

          // 先卸载分组
          try {
            await invoke("unmount_group", {
              groupName,
            });
          } catch (error) {
            console.warn(`卸载分组 ${groupName} 失败:`, error);
          }

          // 删除分组
          await invoke("delete_group", {
            groupName,
          });

          // 删除后刷新列表
          loadFileList(1);

          // 显示删除成功提示
          await dialog.message(`已成功删除分组！`, {
            kind: "info",
            title: "删除成功",
          });
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
        loadFileList(1);
      } catch (error) {
        console.error("组挂载/卸载失败:", error);
        const errorMsg = error.message || JSON.stringify(error);
        await dialog.message(
          `组${isMounted ? "卸载" : "挂载"}失败: ${errorMsg}`,
          {
            kind: "error",
            title: "操作失败",
          },
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
          `group-${groupName.replace(/[^a-zA-Z0-9]/g, "-")}`,
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
        `.group-header[data-group="${groupName}"]`,
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
  document
    .getElementById("refreshBtn")
    .addEventListener("click", () => location.reload());

  // 清理无效链接按钮事件
  document
    .getElementById("cleanupInvalidLinksBtn")
    .addEventListener("click", async () => {
      try {
        console.log("开始清理无效链接...");
        const result = await window.__TAURI__.core.invoke(
          "cleanup_invalid_links",
        );
        console.log("清理无效链接完成:", result);
        // 显示清理结果
        alert(result);
        // 刷新文件列表
        await loadFileList(0);
      } catch (error) {
        console.error("清理无效链接失败:", error);
        alert("清理无效链接失败: " + error.message);
      }
    });

  // 全选按钮事件
  document
    .getElementById("selectAllBtn")
    .addEventListener("click", toggleSelectAll);

  // 分类筛选事件
  document
    .getElementById("categoryFilter")
    .addEventListener("change", applyFilters);

  // 挂载状态筛选事件
  document
    .getElementById("mountStatusFilter")
    .addEventListener("change", applyFilters);

  // 搜索框事件
  document.getElementById("searchBox").addEventListener("input", applyFilters);

  // 批量挂载按钮事件
  document
    .getElementById("batchMountBtn")
    .addEventListener("click", batchMountGroups);

  // 批量卸载按钮事件
  document
    .getElementById("batchUnmountBtn")
    .addEventListener("click", batchUnmountGroups);

  // 批量删除按钮事件
  document
    .getElementById("batchDeleteBtn")
    .addEventListener("click", batchDeleteFiles);

  // 初始加载文件列表
  loadFileList(2);
})();
