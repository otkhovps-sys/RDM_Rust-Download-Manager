// Toolbar popup: connection status, settings and what the current tab offers.

const $ = (id) => document.getElementById(id);
const send = (message) => chrome.runtime.sendMessage(message);

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
  if (item.kind === "page") return { name: `${item.site} video (choose quality in RDM)`, info: "" };
  const name = decodeURIComponent(new URL(item.url).pathname.split("/").pop() || item.url);
  return { name, info: { hls: "HLS", dash: "DASH" }[item.kind] || formatSize(item.size) };
}

async function download(tab, item) {
  await send({
    type: "download",
    tabId: tab.id,
    url: item.url,
    page: item.kind === "page",
    referer: tab.url,
    fileName: tab.title,
  });
  window.close();
}

function showMedia(tab, items) {
  const list = $("media");
  list.textContent = "";
  if (!items.length) {
    const empty = document.createElement("li");
    empty.className = "muted";
    empty.textContent = "No video or audio file seen yet. Play it first.";
    list.appendChild(empty);
  }
  for (const item of items) {
    const { name, info } = describe(item);
    const row = document.createElement("li");
    const label = document.createElement("span");
    label.textContent = name;
    label.title = item.url;
    const size = document.createElement("small");
    size.textContent = info;
    const button = document.createElement("button");
    button.textContent = "Download";
    button.addEventListener("click", () => download(tab, item));
    row.append(label, size, button);
    list.appendChild(row);
  }
}

async function refresh() {
  const { app, settings } = await send({ type: "status" });
  $("dot").classList.toggle("ok", Boolean(app));
  $("status").textContent = app ? "Connected to RDM" : "RDM is not running";
  $("version").textContent = app ? `v${app.version}` : "";
  $("capture").checked = settings.capture;

  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  const web = /^https?:/i.test(tab?.url || "");
  const items = web ? (await send({ type: "list", tabId: tab.id, pageUrl: tab.url })) || [] : [];
  showMedia(tab, items);

  // For players that hide their stream: let yt-dlp look at the page itself.
  const pageButton = $("page");
  pageButton.hidden = !web || items.some((item) => item.kind === "page");
  pageButton.onclick = () => download(tab, { kind: "page", url: tab.url });
}

$("capture").addEventListener("change", (event) => send({ type: "set", values: { capture: event.target.checked } }));

refresh();
