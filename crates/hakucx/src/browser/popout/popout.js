/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// The video pop-out window. The browser opens haku://popout/ with the video's address and
// position in the fragment (it never leaves this computer), plays it here, and gets the
// position back on "Back to tab" or close. This page has no privileges: it talks to the
// browser on the page channel, which only accepts the pop-out messages from this window.

"use strict";

const PREFIX = "\u0001haku-page:";
const post = (msg) => console.debug(PREFIX + JSON.stringify(msg));

const $ = (id) => document.getElementById(id);
const video = $("video");
const seek = $("seek");
const volume = $("volume");

let spec = {};
try {
  // Form encoding: "+" is a space (a real "+" arrives as %2B).
  spec = JSON.parse(decodeURIComponent(location.hash.slice(1).replace(/\+/g, " ")));
} catch (_) {
  spec = {};
}

const fmt = (s) => {
  if (!isFinite(s) || s < 0) return "0:00";
  s = Math.floor(s);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
};

function show(text) {
  const m = $("message");
  m.textContent = text;
  m.hidden = !text;
}

$("title").textContent = spec.title || "Video";
document.title = spec.title ? `${spec.title} - HakuCX` : "HakuCX video";

// Servo (0.7) only fires timeupdate once the element's text track list exists, which it creates
// when something reads `textTracks`.
void video.textTracks;

if (typeof spec.src === "string" && /^https?:/i.test(spec.src)) {
  video.src = spec.src;
  if (typeof spec.poster === "string" && /^https?:/i.test(spec.poster)) video.poster = spec.poster;
  video.volume = Math.min(1, Math.max(0, Number(spec.volume) || 1));
  video.muted = !!spec.muted;
  video.addEventListener(
    "loadedmetadata",
    () => {
      const t = Number(spec.time) || 0;
      if (t > 0 && t < (video.duration || Infinity)) video.currentTime = t;
      if (!spec.paused) video.play().catch(() => {});
    },
    { once: true },
  );
} else {
  show("There is no video to show.");
}

video.addEventListener("error", () => {
  show("This video can't play in the pop-out window. Use “Back to tab” to watch it on the page.");
});

const state = () => ({ time: video.currentTime || Number(spec.time) || 0, paused: video.paused || video.ended });
const back = () => post({ type: "popout-return", ...state() });
const close = () => post({ type: "popout-close", ...state() });
// The browser calls this when the window is closed from outside the page (Alt+F4).
window.hakuClose = close;

const togglePlay = () => (video.paused || video.ended ? video.play().catch(() => {}) : video.pause());

$("back").addEventListener("click", back);
$("close").addEventListener("click", close);
$("play").addEventListener("click", togglePlay);
$("mute").addEventListener("click", () => (video.muted = !video.muted));

seek.addEventListener("input", () => {
  if (isFinite(video.duration)) video.currentTime = (seek.value / 1000) * video.duration;
});
volume.addEventListener("input", () => {
  video.volume = volume.value / 100;
  video.muted = video.volume === 0;
});

const update = () => {
  $("time").textContent = fmt(video.currentTime);
  $("duration").textContent = fmt(video.duration);
  if (isFinite(video.duration) && video.duration > 0 && document.activeElement !== seek) {
    seek.value = String(Math.round((video.currentTime / video.duration) * 1000));
  }
  const paused = video.paused || video.ended;
  $("play").textContent = paused ? "▶︎" : "❚❚";
  document.body.classList.toggle("paused", paused);
  $("mute").textContent = video.muted || video.volume === 0 ? "\u{1F507}︎" : "\u{1F50A}︎";
  volume.value = String(Math.round((video.muted ? 0 : video.volume) * 100));
};
for (const e of ["timeupdate", "play", "pause", "ended", "volumechange", "durationchange", "loadedmetadata"]) {
  video.addEventListener(e, update);
}

// Controls show while the pointer moves, and hide 2 s after it stops.
let idle = 0;
const wake = () => {
  document.body.classList.add("active");
  clearTimeout(idle);
  idle = setTimeout(() => document.body.classList.remove("active"), 2000);
};
document.addEventListener("mousemove", wake);
document.addEventListener("mouseleave", () => document.body.classList.remove("active"));

// Dragging anywhere but a control moves the window.
document.addEventListener("mousedown", (e) => {
  if (e.button !== 0 || e.target.closest("button, input")) return;
  post({ type: "popout-drag" });
});
video.addEventListener("dblclick", back);

document.addEventListener("keydown", (e) => {
  if (e.target.tagName === "INPUT" && e.key !== "Escape") return;
  switch (e.key) {
    case " ":
    case "k":
      togglePlay();
      break;
    case "ArrowLeft":
      video.currentTime = Math.max(0, video.currentTime - 5);
      break;
    case "ArrowRight":
      video.currentTime += 5;
      break;
    case "ArrowUp":
      video.volume = Math.min(1, video.volume + 0.1);
      break;
    case "ArrowDown":
      video.volume = Math.max(0, video.volume - 0.1);
      break;
    case "m":
      video.muted = !video.muted;
      break;
    case "Enter":
      back();
      break;
    case "Escape":
      close();
      break;
    default:
      return;
  }
  e.preventDefault();
  wake();
});

update();
