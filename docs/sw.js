// 靜態網站的離線副本。改過網頁或單字後，要把 CACHE 換成新名字，
// 手機才會丟掉舊檔、改存新的。本機 127.0.0.1 不會註冊這支程式。
const CACHE = "eng-shell-v3";
const ASSETS = [
  "./",
  "./index.html",
  "./install.html",
  "./cards.json",
  "./manifest.json",
  "./icons/icon-192.png",
  "./icons/icon-512.png",
  "./icons/apple-touch-icon.png",
];

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(ASSETS)));
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const path = new URL(request.url).pathname;
  if (path.endsWith("/sw.js")) return;
  event.respondWith(cacheFirst(request));
});

async function cacheFirst(request) {
  const cache = await caches.open(CACHE);
  const cached = await cache.match(request, { ignoreSearch: true });
  if (cached) {
    refresh(cache, request);
    return cached;
  }
  const response = await fetch(request);
  if (response.ok) cache.put(request, response.clone());
  return response;
}

async function refresh(cache, request) {
  try {
    const response = await fetch(request);
    if (response.ok) await cache.put(request, response);
  } catch (error) {
    /* 沒有網路時繼續用手機裡的副本。 */
  }
}
