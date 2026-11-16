// RDM content script: a "Download" button over videos (like IDM's video
// panel) and magnet links that open in RDM.
//
// It runs in every frame, so embedded players get the button too. Which
// entry is "the" video is decided by the background (src/media.js); this
// file only collects what it can see around the video and draws the panel.
(() => {
  if (window.__rdmContent) return;
  window.__rdmContent = true;

  const MIN_WIDTH = 220;
  const MIN_HEIGHT = 120;
  const HIDE_AFTER_MS = 2500;
  const MAX_LINKS = 40;

  const ICONS = {
    logo: `<svg viewBox="0 0 16 16" width="14" height="14"><path fill="currentColor" d="M7 1h2v8l3-3 1.4 1.4L8 12.8 2.6 7.4 4 6l3 3z"/><rect x="2" y="13.5" width="12" height="1.8" rx=".6" fill="currentColor"/></svg>`,
    page: `<svg viewBox="0 0 20 20" width="18" height="18"><rect x="2" y="4" width="16" height="12" rx="3" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M8.3 7.4v5.2l4.4-2.6z" fill="currentColor"/></svg>`,
    hls: `<svg viewBox="0 0 20 20" width="18" height="18"><path d="M3 7c2.3-2 4.7 2 7 0s4.7-2 7 0M3 12c2.3-2 4.7 2 7 0s4.7-2 7 0" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>`,
    file: `<svg viewBox="0 0 20 20" width="18" height="18"><rect x="3" y="3" width="14" height="14" rx="2.5" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M3 7.5h14M3 12.5h14M7 3v4.5M13 3v4.5M7 12.5V17M13 12.5V17" stroke="currentColor" stroke-width="1.2"/></svg>`,
  };
  ICONS.dash = ICONS.hls;

  const STYLE = `
    :host { all: initial; }
    * { box-sizing: border-box; font: 13px/1.35 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; }
    .wrap { --bg: #ffffff; --fg: #1d2330; --muted: #6b7385; --line: #e8ebf1; --hover: #f1f6fd;
            --accent: #1e88e5; --accent-fg: #ffffff; color: var(--fg); }
    @media (prefers-color-scheme: dark) {
      .wrap { --bg: #1f232b; --fg: #e9edf5; --muted: #9aa3b5; --line: #2e3440; --hover: #29303b; --accent: #64b5f6; --accent-fg: #10141a; }
    }
    .pill { display: inline-flex; align-items: center; gap: 7px; padding: 6px 12px 6px 7px; border-radius: 999px;
            border: 1px solid rgba(255,255,255,.16); background: rgba(22,25,32,.84); color: #fff; cursor: pointer;
            backdrop-filter: blur(8px); box-shadow: 0 4px 14px rgba(0,0,0,.35); font-weight: 600;
            animation: rdm-in .16s ease-out; transition: background .12s, transform .12s; }
    .pill:hover { background: rgba(30,136,229,.95); transform: translateY(-1px); }
    .pill .dot { display: grid; place-items: center; width: 22px; height: 22px; border-radius: 50%; background: #1e88e5; }
    .pill:hover .dot { background: rgba(255,255,255,.22); }
    .card { display: none; margin-top: 6px; width: 320px; background: var(--bg); border: 1px solid var(--line);
            border-radius: 12px; box-shadow: 0 12px 32px rgba(0,0,0,.32); overflow: hidden; animation: rdm-in .14s ease-out; }
    .card.open { display: block; }
    .head { padding: 9px 12px; font-weight: 700; border-bottom: 1px solid var(--line); display: flex; justify-content: space-between; }
    .head span { color: var(--muted); font-weight: 500; }
    .row { display: flex; align-items: center; gap: 10px; padding: 9px 12px; cursor: pointer; border-bottom: 1px solid var(--line); }
    .row:last-child { border-bottom: 0; }
    .row:hover { background: var(--hover); }
    .row .icon { flex: none; display: grid; place-items: center; width: 32px; height: 32px; border-radius: 9px;
                 background: var(--hover); color: var(--accent); }
    .row.best .icon { background: var(--accent); color: var(--accent-fg); }
    .row .text { min-width: 0; flex: 1; }
    .row .title { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .row .detail { color: var(--muted); font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .chip { flex: none; padding: 2px 7px; border-radius: 999px; background: var(--accent); color: var(--accent-fg); font-size: 11px; font-weight: 700; }
    .more { padding: 8px 12px; color: var(--accent); cursor: pointer; font-weight: 600; border-bottom: 1px solid var(--line); }
    .more:hover { background: var(--hover); }
    .others { display: none; max-height: 232px; overflow: auto; }
    .others.open { display: block; }
    .note { padding: 12px; color: var(--muted); }
    @keyframes rdm-in { from { opacity: 0; transform: translateY(-4px); } to { opacity: 1; transform: none; } }
  `;

  /** The floating panel, created on first use. */
  let panel = null;
  let activeVideo = null;
  let hideTimer = 0;

  function element(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text) node.textContent = text;
    return node;
  }

  function createPanel() {
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;z-index:2147483647;top:0;left:0;display:none;";
    const root = host.attachShadow({ mode: "closed" });
    root.innerHTML = `<style>${STYLE}</style>
      <div class="wrap">
        <button class="pill" title="Download with RDM"><span class="dot">${ICONS.logo}</span>Download</button>
        <div class="card"></div>
      </div>`;
    const card = root.querySelector(".card");
    root.querySelector(".pill").addEventListener("click", () => toggleCard());
    host.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    host.addEventListener("mouseleave", scheduleHide);
    document.documentElement.appendChild(host);
    return { host, card };
  }

  function scheduleHide() {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      panel.host.style.display = "none";
      panel.card.classList.remove("open");
      activeVideo = null;
    }, HIDE_AFTER_MS);
  }

  function showFor(video) {
    panel ??= createPanel();
    clearTimeout(hideTimer);
    const r = video.getBoundingClientRect();
    panel.host.style.top = `${Math.max(r.top + 10, 6)}px`;
    panel.host.style.left = `${Math.max(Math.min(r.right - 130, window.innerWidth - 340), r.left + 6, 6)}px`;
    panel.host.style.display = "block";
    if (activeVideo !== video) panel.card.classList.remove("open");
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

  /** The boxes around a video that hold no other video, innermost first. */
  function* boxesOf(video) {
    for (let box = video.parentElement; box; box = box.parentElement) {
      if (box.querySelectorAll("video").length > 1) return;
      yield box;
    }
  }

  /**
   * Feeds that do not link a video to its own page. There the address is
   * put together from what the player carries; "" when that fails.
   */
  const FEEDS = [
    {
      // TikTok: the player box is named "xgwrapper-<n>-<video id>" and the
      // author's "/@name" link sits in the same card.
      host: /(^|\.)tiktok\.com$/,
      pageOf(video) {
        let id = "";
        let author = "";
        for (const box of boxesOf(video)) {
          id ||= box.id.match(/(?:^|\D)(\d{15,20})$/)?.[1] || "";
          author ||= box.querySelector('a[href^="/@"]')?.getAttribute("href").match(/^\/@([\w.-]+)/)?.[1] || "";
          if (id && author) break;
        }
        return id ? `https://www.tiktok.com/@${author}/video/${id}` : "";
      },
    },
  ];

  /**
   * The video's own page where known, then the links around the video,
   * nearest first: most feeds (Reels, Shorts, ...) put every video in a box
   * that links to its page.
   */
  function linksAround(video) {
    const links = new Set();
    const own = FEEDS.find((feed) => feed.host.test(location.hostname))?.pageOf(video);
    if (own) links.add(own);
    for (const box of boxesOf(video)) {
      for (const a of box.querySelectorAll("a[href]")) {
        if (links.size >= MAX_LINKS) return [...links];
        links.add(a.href);
      }
    }
    return [...links];
  }

  function contextOf(video) {
    return {
      pageUrl: location.href,
      hasVideo: true,
      currentSrc: video.currentSrc || "",
      links: linksAround(video),
      media: { width: video.videoWidth, height: video.videoHeight, duration: video.duration },
    };
  }

  /** The video the user is watching: the largest one in view. */
  function mainVideo() {
    let best = null;
    let bestArea = 0;
    for (const video of document.querySelectorAll("video")) {
      const r = video.getBoundingClientRect();
      const width = Math.min(r.right, window.innerWidth) - Math.max(r.left, 0);
      const height = Math.min(r.bottom, window.innerHeight) - Math.max(r.top, 0);
      if (r.width < MIN_WIDTH || r.height < MIN_HEIGHT || width <= 0 || height <= 0) continue;
      if (width * height > bestArea) [best, bestArea] = [video, width * height];
    }
    return best;
  }

  // The toolbar popup asks what is being watched (top frame only).
  chrome.runtime.onMessage.addListener((message, _sender, reply) => {
    if (message?.type !== "context") return;
    const video = mainVideo();
    reply(video ? contextOf(video) : null);
  });

  function rowFor(item, best) {
    const row = element("div", best ? "row best" : "row");
    row.title = item.url;
    const icon = element("div", "icon");
    icon.innerHTML = ICONS[item.kind] || ICONS.file;
    const text = element("div", "text");
    text.append(element("div", "title", item.title), element("div", "detail", item.detail));
    row.append(icon, text);
    if (best) row.append(element("span", "chip", "Best"));
    row.addEventListener("click", async () => {
      const sent = await chrome.runtime.sendMessage({ type: "download", item, title: document.title, referer: location.href });
      panel.card.textContent = "";
      panel.card.append(element("div", "note", sent ? "Sent to RDM." : "RDM is not running - start it and try again."));
      setTimeout(() => panel.card.classList.remove("open"), 1600);
    });
    return row;
  }

  async function toggleCard() {
    const { card } = panel;
    if (card.classList.toggle("open") === false) return;
    card.textContent = "";
    const head = element("div", "head", "Download with RDM");
    card.append(head);
    const reply = await chrome.runtime.sendMessage({ type: "list", context: contextOf(activeVideo) });
    const { primary, others } = reply || { primary: null, others: [] };
    if (!primary) {
      card.append(element("div", "note", "Nothing to download yet. Play the video for a moment first."));
      return;
    }
    card.append(rowFor(primary, true));
    if (others.length) {
      const more = element("div", "more", `More on this page (${others.length})`);
      const list = element("div", "others");
      for (const item of others) list.append(rowFor(item, false));
      more.addEventListener("click", () => list.classList.toggle("open"));
      card.append(more, list);
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
