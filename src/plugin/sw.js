// Service Worker for OpenList
const CACHE_NAME = "oplist-cache-v1";

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
  if (url.hostname !== self.location.hostname) return;
  if (
    url.pathname === "/" ||
    url.pathname.startsWith("/assets/") ||
    url.pathname.startsWith("/static/")
  ) {
    // 实现 Stale-While-Revalidate 缓存策略
    event.respondWith(
      caches.match(request).then((cachedResponse) => {
        // 无论缓存是否存在，都在后台发起网络请求更新缓存
        const networkFetch = fetch(request).then((networkResponse) => {
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
        // 如果缓存存在，立即返回缓存的响应
        // 否则等待网络请求的响应
        return cachedResponse || networkFetch;
      }),
    );
  }
});
