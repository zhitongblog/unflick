const ENDPOINT = "http://127.0.0.1:19542";

const badgeEl = document.getElementById("daemon-state");
const urlEl = document.getElementById("current-url");
const feedbackEl = document.getElementById("feedback");
const playBtn = document.getElementById("btn-play");
const queueBtn = document.getElementById("btn-queue");
const handoffRow = document.getElementById("handoff-row");
const resumeChk = document.getElementById("chk-resume");
const resumeLabel = document.getElementById("resume-label");

let activeUrl = "";
let activeTabId = null;
let detectedSeek = null;

function formatClock(sec) {
  const s = Math.max(0, Math.floor(sec));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  const pad = (n) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(r)}` : `${m}:${pad(r)}`;
}

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
  activeTabId = tab?.id || null;
  urlEl.textContent = activeUrl || "No active page URL";

  if (activeTabId) {
    chrome.runtime.sendMessage(
      { type: "unflick-probe", tabId: activeTabId },
      (res) => {
        const t = res?.probe?.currentTime;
        if (typeof t === "number" && t >= 2) {
          detectedSeek = t;
          resumeLabel.textContent = `Continue at ${formatClock(t)} (& pause tab)`;
          handoffRow.classList.add("visible");
        }
      },
    );
  }

  await checkDaemon();
}

function dispatch(action) {
  if (!activeUrl) return;
  feedbackEl.textContent = action === "queue" ? "Queueing…" : "Opening in unflick…";
  const useSeek =
    action === "play" && detectedSeek !== null && resumeChk.checked
      ? detectedSeek
      : null;
  chrome.runtime.sendMessage(
    {
      type: "unflick-open",
      url: activeUrl,
      action,
      seek: useSeek,
      pauseTabId: useSeek !== null ? activeTabId : null,
    },
    (res) => {
      if (res?.ok) {
        feedbackEl.textContent =
          action === "queue" ? "Added to playlist" : "Playing in unflick";
        setTimeout(() => window.close(), 450);
      } else {
        feedbackEl.textContent = res?.error || "Failed to contact unflick";
      }
    },
  );
}

playBtn.addEventListener("click", () => dispatch("play"));
queueBtn.addEventListener("click", () => dispatch("queue"));

void init();
