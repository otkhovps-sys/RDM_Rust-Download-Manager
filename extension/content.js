// Shows a "Download this video" button over videos, like IDM's video panel.
(() => {
  if (window.__rdmContent) return;
  window.__rdmContent = true;

  const MIN_WIDTH = 220;
  const MIN_HEIGHT = 120;
  let host, button, menu, hideTimer, activeVideo;

  function formatSize(bytes) {
    if (!bytes) return "";
    const units = ["B", "KB", "MB", "GB"];
    let i = 0;
    while (bytes >= 1024 && i < units.length - 1) {
      bytes /= 1024;
      i++;
    }
    return `${bytes.toFixed(i ? 1 : 0)} ${units[i]}`;
  }

  function createUi() {
    host = document.createElement("div");
    host.style.cssText = "position:fixed;z-index:2147483647;top:0;left:0;display:none;";
    const shadow = host.attachShadow({ mode: "closed" });
    shadow.innerHTML = `
      <style>
        * { box-sizing: border-box; font: 13px/1.3 system-ui, sans-serif; }
        .btn { display:flex; align-items:center; gap:6px; padding:6px 10px; border:0; border-radius:6px;
               background:#1e88e5; color:#fff; cursor:pointer; box-shadow:0 2px 8px rgba(0,0,0,.35); }
        .btn:hover { background:#1565c0; }
        .menu { display:none; margin-top:4px; min-width:260px; max-width:420px; background:#fff; color:#222;
                border-radius:6px; box-shadow:0 4px 16px rgba(0,0,0,.35); overflow:hidden; }
        .item { padding:7px 10px; cursor:pointer; border-bottom:1px solid #eee; white-space:nowrap;
                overflow:hidden; text-overflow:ellipsis; }
        .item:hover { background:#e3f2fd; }
        .item small { color:#666; margin-left:6px; }
        .empty { padding:8px 10px; color:#666; }
      </style>
      <button class="btn" title="Download with RDM">
        <svg width="14" height="14" viewBox="0 0 16 16"><path fill="#fff" d="M7 1h2v8l3-3 1.4 1.4L8 12.8 2.6 7.4 4 6l3 3z"/><rect x="2" y="13.5" width="12" height="1.8" fill="#fff"/></svg>
        Download this video
      </button>
      <div class="menu"></div>`;
    button = shadow.querySelector(".btn");
    menu = shadow.querySelector(".menu");
    button.addEventListener("click", toggleMenu);
    host.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    host.addEventListener("mouseleave", scheduleHide);
    document.documentElement.appendChild(host);
  }

  function scheduleHide() {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      host.style.display = "none";
      menu.style.display = "none";
      activeVideo = null;
    }, 2500);
  }

  function showFor(video) {
    if (!host) createUi();
    clearTimeout(hideTimer);
    const r = video.getBoundingClientRect();
    host.style.top = `${Math.max(r.top + 8, 4)}px`;
    host.style.left = `${Math.max(r.right - 200, r.left + 4)}px`;
    host.style.display = "block";
    if (activeVideo !== video) menu.style.display = "none";
    activeVideo = video;
  }

  function videoUnder(x, y) {
    for (const v of document.querySelectorAll("video")) {
      const r = v.getBoundingClientRect();
      if (r.width < MIN_WIDTH || r.height < MIN_HEIGHT) continue;
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return v;
    }
    return null;
  }

  let lastMove = 0;
  document.addEventListener(
    "mousemove",
    (e) => {
      const now = Date.now();
      if (now - lastMove < 150) return;
      lastMove = now;
      const video = videoUnder(e.clientX, e.clientY);
      if (video) showFor(video);
      else if (host && host.style.display === "block" && !host.matches(":hover")) scheduleHide();
    },
    { passive: true }
  );

  async function toggleMenu() {
    if (menu.style.display === "block") {
      menu.style.display = "none";
      return;
    }
    let items = (await chrome.runtime.sendMessage({ type: "list" })) || [];
    const src = activeVideo && activeVideo.currentSrc;
    if (src && /^https?:/i.test(src) && !items.some((i) => i.url === src)) {
      items = [{ url: src, kind: "file", size: 0, type: "video" }, ...items];
    }
    menu.textContent = "";
    if (!items.length) {
      const empty = document.createElement("div");
      empty.className = "empty";
      empty.textContent = "No downloadable stream found yet. Play the video first.";
      menu.appendChild(empty);
    }
    for (const item of items.slice().reverse()) {
      const row = document.createElement("div");
      row.className = "item";
      const label = item.kind === "hls" ? "HLS stream (m3u8)" : item.type || "Media file";
      const name = decodeURIComponent(new URL(item.url).pathname.split("/").pop() || "");
      row.textContent = `${label} ${name}`;
      const small = document.createElement("small");
      small.textContent = formatSize(item.size);
      row.appendChild(small);
      row.title = item.url;
      row.addEventListener("click", () => {
        chrome.runtime.sendMessage({ type: "download", url: item.url, referer: location.href, fileName: document.title });
        menu.style.display = "none";
      });
      menu.appendChild(row);
    }
    menu.style.display = "block";
  }
})();
