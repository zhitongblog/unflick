const ENDPOINT = "http://127.0.0.1:19542";

async function sendToUnflick(url, action = "play") {
  try {
    const res = await fetch(`${ENDPOINT}/open`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url, action }),
    });
    if (!res.ok) {
      throw new Error(`HTTP ${res.status}`);
    }
    return await res.json();
  } catch (_err) {
    // When the unflick daemon isn't running yet, fall back to the
    // OS-registered unflick:// URL scheme so the system launches unflick.
    const scheme = `unflick://${action === "queue" ? "queue" : "play"}?url=${encodeURIComponent(url)}`;
    await chrome.tabs.create({ url: scheme, active: false });
    return { status: "launched_via_scheme", url };
  }
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "unflick-play",
    title: "Play in unflick",
    contexts: ["page", "link", "video", "audio"],
  });
  chrome.contextMenus.create({
    id: "unflick-queue",
    title: "Queue in unflick",
    contexts: ["page", "link", "video", "audio"],
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  const target = info.srcUrl || info.linkUrl || info.pageUrl || tab?.url;
  if (!target) return;
  const action = info.menuItemId === "unflick-queue" ? "queue" : "play";
  void sendToUnflick(target, action);
});

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type === "unflick-open") {
    sendToUnflick(msg.url, msg.action || "play")
      .then((res) => sendResponse({ ok: true, data: res }))
      .catch((err) => sendResponse({ ok: false, error: String(err) }));
    return true;
  }
  return false;
});
