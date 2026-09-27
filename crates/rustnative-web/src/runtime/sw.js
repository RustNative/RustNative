// The framework's service worker (Web milestone I): offline pages, cached
// assets, server calls queued while offline, and updates the page applies
// when it is ready. The server writes the build's version and the asset
// list over the two placeholders below.

const VERSION = "__RN_VERSION__";
const PRECACHE = __RN_PRECACHE__;
const OFFLINE = "__RN_OFFLINE__";
const ASSETS = `rn-assets-${VERSION}`;
const PAGES = "rn-pages";

self.addEventListener("install", (event) => {
  // The new build waits until the page applies it (`rn:update`).
  event.waitUntil(caches.open(ASSETS).then((cache) => cache.addAll(PRECACHE)));
});

self.addEventListener("activate", (event) => {
  event.waitUntil((async () => {
    for (const name of await caches.keys()) if (name.startsWith("rn-assets-") && name !== ASSETS) await caches.delete(name);
    await self.clients.claim();
  })());
});

self.addEventListener("message", (event) => {
  if (event.data === "rn:apply-update") self.skipWaiting();
  if (event.data === "rn:replay") event.waitUntil(replay());
});

self.addEventListener("sync", (event) => {
  if (event.tag === "rn-calls") event.waitUntil(replay());
});

function isAsset(url) {
  // Hashed, immutable: the runtime, client modules, the worker script.
  return url.origin === self.location.origin && /^\/_rn\/(rn\.|m\/|worker\.)/.test(url.pathname);
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (request.method === "POST" && url.pathname.startsWith("/_fn/")) {
    event.respondWith(call(request));
    return;
  }
  if (request.method !== "GET") return;
  if (isAsset(url)) {
    event.respondWith(caches.match(request).then((hit) => hit || fetch(request).then((response) => {
      if (response.ok) caches.open(ASSETS).then((cache) => cache.put(request, response.clone()));
      return response;
    })));
    return;
  }
  if (request.mode === "navigate") {
    // Network first, so a page is never older than it needs to be; the
    // last copy, or the offline page, when there is no network.
    event.respondWith((async () => {
      try {
        const response = await fetch(request);
        if (response.ok) {
          const cache = await caches.open(PAGES);
          await cache.put(request, response.clone());
        }
        return response;
      } catch (_) {
        return (await caches.match(request, { cacheName: PAGES })) || (await caches.match(OFFLINE)) || Response.error();
      }
    })());
  }
});

// ------------------------------------------------ calls made offline ----

function queue() {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open("rn-sw", 1);
    open.onupgradeneeded = () => open.result.createObjectStore("calls", { keyPath: "id", autoIncrement: true });
    open.onsuccess = () => resolve(open.result);
    open.onerror = () => reject(open.error);
  });
}

async function store(mode, act) {
  const db = await queue();
  try {
    return await new Promise((resolve, reject) => {
      const transaction = db.transaction("calls", mode);
      const result = act(transaction.objectStore("calls"));
      transaction.oncomplete = () => resolve(result.result);
      transaction.onerror = () => reject(transaction.error);
    });
  } finally {
    db.close();
  }
}

/// A server call: through to the server, or, with no network, kept and
/// answered `503` with a note that it will be delivered.
async function call(request) {
  const copy = request.clone();
  try {
    return await fetch(request);
  } catch (_) {
    const saved = { url: copy.url, headers: [...copy.headers], body: await copy.text(), at: Date.now() };
    await store("readwrite", (calls) => calls.add(saved));
    try { await self.registration.sync.register("rn-calls"); } catch (_) { /* no Background Sync: the page asks when it is online */ }
    return new Response(JSON.stringify({ error: "offline: the call is queued and will be delivered when the network is back" }), {
      status: 503,
      headers: { "content-type": "application/json", "x-rn-queued": "1" },
    });
  }
}

let replaying = null;

/// Delivers the queued calls, oldest first, each once: a call is removed
/// when the server has answered it (whatever the answer), and kept when
/// the network is still not there.
function replay() {
  replaying ??= (async () => {
    try {
      const calls = await store("readonly", (all) => all.getAll());
      for (const saved of calls) {
        try {
          await fetch(saved.url, { method: "POST", headers: saved.headers, body: saved.body, credentials: "same-origin" });
        } catch (_) {
          return;
        }
        await store("readwrite", (all) => all.delete(saved.id));
      }
    } finally {
      replaying = null;
    }
  })();
  return replaying;
}
