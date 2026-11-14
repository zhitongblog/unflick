const ENDPOINT = "http://127.0.0.1:19542";

async function probeTabVideo(tabId, pauseVideo = false) {
  if (!tabId) return null;
  try {
    const results = await chrome.scripting.executeScript({
      target: { tabId },
      func: (shouldPause) => {
        const videos = Array.from(document.querySelectorAll("video"));
        const active =
          videos.find((v) => !v.paused && v.currentTime > 0) ||
          videos.find((v) => v.currentTime > 0) ||
          videos[0];
        if (!active) return null;
        const currentTime = Math.floor(active.currentTime || 0);
        const duration = Math.floor(active.duration || 0);
        if (shouldPause && !active.paused) {
          active.pause();
        }
        return { currentTime, duration };
      },
      args: [pauseVideo],
    });
    return results?.[0]?.result || null;
  } catch (_err) {
    return null;
  }
}

async function sendToUnflick(url, action = "play", seek = null) {
  const validSeek =
    typeof seek === "number" && Number.isFinite(seek) && seek > 0
      ? Math.floor(seek)
      : null;
  try {
    const payload = { url, action };
    if (validSeek !== null && action === "play") {
      payload.seek = validSeek;
    }
    const res = await fetch(`${ENDPOINT}/open`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!res.ok) {
      throw new Error(`HTTP ${res.status}`);
    }
    return await res.json();
  } catch (_err) {
    // When the unflick daemon isn't running yet, fall back to the
    // OS-registered unflick:// URL scheme so the system launches unflick.
    const verb = action === "queue" ? "queue" : "play";
    let scheme = `unflick://${verb}?url=${encodeURIComponent(url)}`;
    if (validSeek !== null && verb === "play") {
      scheme += `&t=${validSeek}`;
    }
    await chrome.tabs.create({ url: scheme, active: false });
    return { status: "launched_via_scheme", url, seek: validSeek };
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

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const target = info.srcUrl || info.linkUrl || info.pageUrl || tab?.url;
  if (!target) return;
  const action = info.menuItemId === "unflick-queue" ? "queue" : "play";
  let seek = null;
  if (action === "play" && !info.linkUrl && tab?.id) {
    const probe = await probeTabVideo(tab.id, true);
    if (probe && probe.currentTime >= 2) {
      seek = probe.currentTime;
    }
  }
  void sendToUnflick(target, action, seek);
});

chrome.commands.onCommand.addListener(async (command) => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.url) return;
  if (command === "play-current-tab") {
    const probe = await probeTabVideo(tab.id, true);
    const seek = probe && probe.currentTime >= 2 ? probe.currentTime : null;
    void sendToUnflick(tab.url, "play", seek);
  } else if (command === "queue-current-tab") {
    void sendToUnflick(tab.url, "queue", null);
  }
});

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type === "unflick-probe") {
    probeTabVideo(msg.tabId, false)
      .then((probe) => sendResponse({ ok: true, probe }))
      .catch(() => sendResponse({ ok: false, probe: null }));
    return true;
  }
  if (msg?.type === "unflick-open") {
    (async () => {
      if (msg.pauseTabId) {
        await probeTabVideo(msg.pauseTabId, true);
      }
      return await sendToUnflick(msg.url, msg.action || "play", msg.seek);
    })()
      .then((res) => sendResponse({ ok: true, data: res }))
      .catch((err) => sendResponse({ ok: false, error: String(err) }));
    return true;
  }
  return false;
});
