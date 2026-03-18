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
 * 每次从队列中取一个请求处理，使用函数属性保存promise防止并发
 * @returns {Promise} 当前正在处理的promise或新发起的promise
 */
const processUpdateQueue = () => {
  return updateQueue.size === 0
    ? Promise.resolve() // 如果队列为空，返回已完成的promise
    : // 直接返回当前promise或创建新的promise
      (processUpdateQueue.p ??= (async () => {
        // 从队列中取出第一个请求并移除
        let it = updateQueue.entries().next();
        if (!it) return;

        const [key, request] = it.value;
        updateQueue.delete(key);

        // 发起请求并处理
        const networkResponse = await fetch(request);
        // 只有当响应有效时才更新缓存
        if (networkResponse.ok) {
          // 先克隆响应对象，然后再使用
          const clonedResponse = networkResponse.clone();
          const cache = await caches.open(CACHE_NAME);
          await cache.put(request, clonedResponse);
        } else throw new Error(networkResponse.statusText);
      })().finally(() => delete processUpdateQueue.p));
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
      idleTimer = null;
      // 处理队列中的所有请求
      const processQueue = () => {
        if (updateQueue.size > 0 && !idleTimer) {
          // 当前请求处理完成后，继续处理下一个
          processUpdateQueue().then(() => processQueue());
        }
      };
      processQueue();
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
  // 在任何请求被发起时都重置计时器
  resetIdleTimer();
});
