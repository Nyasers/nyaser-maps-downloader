// Service Worker for OpenList
const CACHE_NAME = "oplist-cache-v1";

// 缓存更新队列，用于存储需要在空闲时更新的请求（使用URL作为键去重）
const updateQueue = new Map();
// 空闲更新计时器
let idleTimer = null;
// 空闲时间阈值（毫秒）
const IDLE_TIMEOUT = 1e4;

/**
 * 处理缓存更新队列
 * 遍历队列中的请求，发起网络请求并更新缓存
 */
const processUpdateQueue = () => {
  // 复制队列内容并清空队列
  const requests = [...updateQueue.values()];
  updateQueue.clear();

  // 处理每个请求
  requests.forEach((request) => {
    fetch(request).then((networkResponse) => {
      // 只有当响应有效时才更新缓存
      if (networkResponse && networkResponse.ok) {
        // 先克隆响应对象，然后再使用
        const clonedResponse = networkResponse.clone();
        caches.open(CACHE_NAME).then((cache) => {
          cache.put(request, clonedResponse);
        });
      }
    });
  });
};

/**
 * 重置空闲计时器
 * 在每次收到 fetch 事件时调用，确保在空闲时才处理更新队列
 */
const resetIdleTimer = () => {
  // 清除现有的计时器
  if (idleTimer) {
    clearTimeout(idleTimer);
  }

  // 如果队列不为空，设置新的计时器
  if (updateQueue.size > 0) {
    idleTimer = setTimeout(() => {
      processUpdateQueue();
    }, IDLE_TIMEOUT);
  }
};

// install 事件：安装完成后立即激活
self.addEventListener("install", (event) => {
  event.waitUntil(self.skipWaiting());
});

// activate 事件：清理旧缓存
self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((cacheNames) =>
        Promise.all(
          cacheNames
            .filter((cacheName) => cacheName !== CACHE_NAME)
            .map((cacheName) => caches.delete(cacheName)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

// fetch 事件：处理网络请求
self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (self.location.hostname !== url.hostname) return;
  if (
    url.pathname === "/" ||
    url.pathname.startsWith("/assets/") ||
    url.pathname.startsWith("/static/")
  ) {
    // 实现 Stale-While-Revalidate 缓存策略
    event.respondWith(
      caches.match(request).then((cachedResponse) => {
        if (cachedResponse) {
          // 缓存存在时，将请求添加到更新队列（使用URL作为键去重）
          updateQueue.set(request.url, request);
          return cachedResponse;
        } else {
          // 缓存不存在时，直接发起网络请求
          return fetch(request).then((networkResponse) => {
            // 只有当响应有效时才更新缓存
            if (networkResponse && networkResponse.ok) {
              // 先克隆响应对象，然后再使用
              const clonedResponse = networkResponse.clone();
              caches.open(CACHE_NAME).then((cache) => {
                cache.put(request, clonedResponse);
              });
            }
            return networkResponse;
          });
        }
      }),
    );
  }
  // 在任何请求被发起时都重置计数器
  resetIdleTimer();
});
