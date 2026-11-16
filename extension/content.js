// RDM content script: a "Download this video" button over videos (like IDM's
// video panel) and magnet links that open in RDM.
//
// It runs in every frame, so embedded players (for example a YouTube video
// inside another site) get the button too.
(() => {
  if (window.__rdmContent) return;
  window.__rdmContent = true;

  const MIN_WIDTH = 220;
  const MIN_HEIGHT = 120;
  const HIDE_AFTER_MS = 2500;

  const STYLE = `
    * { box-sizing: border-box; font: 13px/1.3 system-ui, sans-serif; }
    .button { display: flex; align-items: center; gap: 6px; padding: 6px 10px; border: 0; border-radius: 6px;
              background: #1e88e5; color: #fff; cursor: pointer; box-shadow: 0 2px 8px rgba(0,0,0,.35); }
    .button:hover { background: #1565c0; }
    .menu { display: none; margin-top: 4px; min-width: 270px; max-width: 440px; background: #fff; color: #222;
            border-radius: 6px; box-shadow: 0 4px 16px rgba(0,0,0,.35); overflow: hidden; }
    .menu.open { display: block; }
    .item { padding: 7px 10px; cursor: pointer; border-bottom: 1px solid #eee; white-space: nowrap;
            overflow: hidden; text-overflow: ellipsis; }
    .item:hover { background: #e3f2fd; }
    .item small { color: #666; margin-left: 6px; }
    .note { padding: 8px 10px; color: #666; }
  `;
  const ARROW = `<svg width="14" height="14" viewBox="0 0 16 16"><path fill="#fff" d="M7 1h2v8l3-3 1.4 1.4L8 12.8 2.6 7.4 4 6l3 3z"/><rect x="2" y="13.5" width="12" height="1.8" fill="#fff"/></svg>`;

  /** The floating panel, created on first use. */
  let panel = null;
  let activeVideo = null;
  let hideTimer = 0;

  function createPanel() {
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;z-index:2147483647;top:0;left:0;display:none;";
    const root = host.attachShadow({ mode: "closed" });
    root.innerHTML = `<style>${STYLE}</style>
      <button class="button" title="Download with RDM">${ARROW} Download this video</button>
      <div class="menu"></div>`;
    const menu = root.querySelector(".menu");
    root.querySelector(".button").addEventListener("click", () => toggleMenu());
    host.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    host.addEventListener("mouseleave", scheduleHide);
    document.documentElement.appendChild(host);
    return { host, menu };
  }

  function scheduleHide() {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      panel.host.style.display = "none";
      panel.menu.classList.remove("open");
      activeVideo = null;
    }, HIDE_AFTER_MS);
  }

  function showFor(video) {
    panel ??= createPanel();
    clearTimeout(hideTimer);
    const r = video.getBoundingClientRect();
    panel.host.style.top = `${Math.max(r.top + 8, 4)}px`;
    panel.host.style.left = `${Math.max(r.right - 200, r.left + 4)}px`;
    panel.host.style.display = "block";
    if (activeVideo !== video) panel.menu.classList.remove("open");
    activeVideo = video;
  }

  function videoUnder(x, y) {
    for (const video of document.querySelectorAll("video")) {
      const r = video.getBoundingClientRect();
      const bigEnough = r.width >= MIN_WIDTH && r.height >= MIN_HEIGHT;
      if (bigEnough && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return video;
    }
    return null;
  }

  let lastMove = 0;
  document.addEventListener(
    "mousemove",
    (event) => {
      const now = Date.now();
      if (now - lastMove < 150) return;
      lastMove = now;
      const video = videoUnder(event.clientX, event.clientY);
      if (video) showFor(video);
      else if (panel && panel.host.style.display === "block" && !panel.host.matches(":hover")) scheduleHide();
    },
    { passive: true }
  );

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

  function describe(item) {
    if (item.kind === "page") return `${item.site || "This page's"} video - choose quality in RDM`;
    const name = decodeURIComponent(new URL(item.url).pathname.split("/").pop() || "");
    const kind = { hls: "HLS stream (m3u8)", dash: "DASH stream (mpd)" }[item.kind] || item.type || "Media file";
    return `${kind} ${name}`;
  }

  /** What can be downloaded here: the background's list plus the video's own address. */
  async function downloadable() {
    const items = (await chrome.runtime.sendMessage({ type: "list", pageUrl: location.href, hasVideo: true })) || [];
    const src = activeVideo?.currentSrc || "";
    const direct = /^https?:/i.test(src) && !items.some((i) => i.url === src);
    return direct ? [...items, { kind: "file", url: src, size: 0, type: "video" }] : items;
  }

  async function toggleMenu() {
    const { menu } = panel;
    if (menu.classList.toggle("open") === false) return;
    menu.textContent = "";
    const items = await downloadable();
    if (!items.length) {
      const note = document.createElement("div");
      note.className = "note";
      note.textContent = "Nothing found yet. Play the video first.";
      menu.appendChild(note);
    }
    for (const item of items) {
      const row = document.createElement("div");
      row.className = "item";
      row.title = item.url;
      row.textContent = describe(item);
      const size = document.createElement("small");
      size.textContent = formatSize(item.size);
      row.appendChild(size);
      row.addEventListener("click", () => {
        chrome.runtime.sendMessage({
          type: "download",
          url: item.url,
          page: item.kind === "page",
          referer: location.href,
          fileName: document.title,
        });
        menu.classList.remove("open");
      });
      menu.appendChild(row);
    }
  }

  // Magnet links open RDM instead of another torrent program - unless RDM is
  // not running, then the browser handles them as usual.
  document.addEventListener(
    "click",
    async (event) => {
      const link = event.target.closest?.('a[href^="magnet:"]');
      if (!link || event.defaultPrevented || event.button !== 0) return;
      event.preventDefault();
      const taken = await chrome.runtime.sendMessage({ type: "magnet", url: link.href });
      if (!taken) location.href = link.href;
    },
    true
  );
})();
