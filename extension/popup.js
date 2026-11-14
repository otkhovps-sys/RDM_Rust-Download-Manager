const $ = (id) => document.getElementById(id);

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

async function refresh() {
  const { app, settings } = await chrome.runtime.sendMessage({ type: "status" });
  $("dot").classList.toggle("ok", Boolean(app));
  $("status").textContent = app ? `Connected to RDM ${app.version}` : "RDM is not running";
  $("capture").checked = settings.capture;
  $("port").value = settings.port;

  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  const items = (await chrome.runtime.sendMessage({ type: "list", tabId: tab.id })) || [];
  const list = $("media");
  list.textContent = "";
  if (!items.length) {
    const li = document.createElement("li");
    li.className = "muted";
    li.textContent = "Nothing detected yet. Play a video or audio first.";
    list.appendChild(li);
  }
  for (const item of items.slice().reverse()) {
    const li = document.createElement("li");
    const name = document.createElement("span");
    name.textContent = decodeURIComponent(new URL(item.url).pathname.split("/").pop() || item.url);
    name.title = item.url;
    const info = document.createElement("small");
    info.textContent = item.kind === "hls" ? "HLS" : formatSize(item.size);
    const button = document.createElement("button");
    button.textContent = "Download";
    button.addEventListener("click", async () => {
      await chrome.runtime.sendMessage({ type: "download", tabId: tab.id, url: item.url, referer: tab.url, fileName: tab.title });
      window.close();
    });
    li.append(name, info, button);
    list.appendChild(li);
  }
}

$("capture").addEventListener("change", (e) =>
  chrome.runtime.sendMessage({ type: "set", values: { capture: e.target.checked } })
);
$("port").addEventListener("change", async (e) => {
  await chrome.runtime.sendMessage({ type: "set", values: { port: Number(e.target.value) || 9614 } });
  refresh();
});

refresh();
