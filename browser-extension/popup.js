const ENDPOINT = "http://127.0.0.1:19542";

const badgeEl = document.getElementById("daemon-state");
const urlEl = document.getElementById("current-url");
const feedbackEl = document.getElementById("feedback");
const playBtn = document.getElementById("btn-play");
const queueBtn = document.getElementById("btn-queue");

let activeUrl = "";

async function checkDaemon() {
  try {
    const res = await fetch(`${ENDPOINT}/status`);
    if (res.ok) {
      const body = await res.json();
      const state = body?.data?.state || "idle";
      badgeEl.textContent = state;
      badgeEl.classList.add("online");
      return;
    }
  } catch (_err) {
    // Offline; protocol fallback will launch unflick.
  }
  badgeEl.textContent = "via unflick://";
}

async function init() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  activeUrl = tab?.url || "";
  urlEl.textContent = activeUrl || "No active page URL";
  await checkDaemon();
}

function dispatch(action) {
  if (!activeUrl) return;
  feedbackEl.textContent = action === "queue" ? "Queueing…" : "Opening in unflick…";
  chrome.runtime.sendMessage({ type: "unflick-open", url: activeUrl, action }, (res) => {
    if (res?.ok) {
      feedbackEl.textContent = action === "queue" ? "Added to playlist" : "Playing in unflick";
      setTimeout(() => window.close(), 450);
    } else {
      feedbackEl.textContent = res?.error || "Failed to contact unflick";
    }
  });
}

playBtn.addEventListener("click", () => dispatch("play"));
queueBtn.addEventListener("click", () => dispatch("queue"));

void init();
