// Toolbar popup: connection status, what the current tab offers, one setting.
// The texts of each entry (`title`, `detail`) come from src/media.js.

const $ = (id) => document.getElementById(id);
const send = (message) => chrome.runtime.sendMessage(message);

const ICONS = {
  page: `<svg viewBox="0 0 20 20" width="18" height="18"><rect x="2" y="4" width="16" height="12" rx="3" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M8.3 7.4v5.2l4.4-2.6z" fill="currentColor"/></svg>`,
  hls: `<svg viewBox="0 0 20 20" width="18" height="18"><path d="M3 7c2.3-2 4.7 2 7 0s4.7-2 7 0M3 12c2.3-2 4.7 2 7 0s4.7-2 7 0" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>`,
  file: `<svg viewBox="0 0 20 20" width="18" height="18"><rect x="3" y="3" width="14" height="14" rx="2.5" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M3 7.5h14M3 12.5h14M7 3v4.5M13 3v4.5M7 12.5V17M13 12.5V17" stroke="currentColor" stroke-width="1.2"/></svg>`,
};
ICONS.dash = ICONS.hls;

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

function rowFor(tab, item, best) {
  const row = element("button", best ? "row best" : "row");
  row.title = item.url;
  const icon = element("div", "icon");
  icon.innerHTML = ICONS[item.kind] || ICONS.file;
  const text = element("div", "text");
  text.append(element("div", "title", item.title), element("div", "detail", item.detail));
  row.append(icon, text);
  if (best) row.append(element("span", "chip", "Best"));
  row.addEventListener("click", async () => {
    await send({ type: "download", item, title: tab.title, referer: tab.url });
    window.close();
  });
  return row;
}

async function showPage(tab) {
  const context = { pageUrl: tab.url, offerPage: true };
  const { primary, others } = (await send({ type: "list", tabId: tab.id, context })) || { primary: null, others: [] };
  $("found").hidden = !primary;
  $("empty").hidden = Boolean(primary);
  if (!primary) return;

  $("primary").replaceChildren(rowFor(tab, primary, true));
  $("others").replaceChildren(...others.map((item) => rowFor(tab, item, false)));
  const toggle = $("toggle");
  toggle.hidden = others.length === 0;
  toggle.textContent = `More on this page (${others.length})`;
  toggle.onclick = () => ($("others").hidden = !$("others").hidden);
}

async function refresh() {
  const { app, settings } = await send({ type: "status" });
  $("status").classList.toggle("ok", Boolean(app));
  $("status").textContent = app ? "Connected" : "Not connected";
  $("version").textContent = app ? `v${app.version}` : "";
  $("capture").checked = settings.capture;
  $("offline").hidden = Boolean(app);

  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (app && /^https?:/i.test(tab?.url || "")) {
    await showPage(tab);
  } else {
    $("found").hidden = true;
    $("empty").hidden = !app;
  }
}

$("capture").addEventListener("change", (event) => send({ type: "set", values: { capture: event.target.checked } }));

refresh();
