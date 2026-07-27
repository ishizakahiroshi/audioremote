// audioremote v0.1 built-in Web UI.
// Vanilla JS, no framework. Poll every POLL_MS.
// i18n: language packs live under /lang/<code>.json. Adding a new language
// means dropping a new JSON with a `_lang: { code, name }` block; the server
// auto-discovers it via GET /api/languages.

const POLL_MS = 3000;
const VOLUME_DEBOUNCE_MS = 200;
const TOKEN_KEY = "audioremote.token";
const LANG_KEY = "audioremote.lang";

const app = document.getElementById("app");
const toastEl = document.getElementById("toast");

let state = {
  token: readTokenFromHashOrStorage(),
  status: null,
  devices: null,
  volume: null,
  showSettings: false,
  showAbout: false,
  about: null,
  showInactive: false,
  pendingId: null,
  pollTimer: null,
  needsToken: false,
  lang: null,           // active language code, e.g. "ja"
  strings: {},          // key -> translated string
  langs: [],            // available packs [{code, name}]
};

// Kept outside render state so rebuilding the page during polling never loses
// an in-progress slider drag or reorders a newer volume request behind an old
// response.
const volumeControl = {
  active: false,
  draft: null,
  timer: null,
  sending: false,
  queued: null,
  revision: 0,
};

// ---------- Token from hash ----------

function readTokenFromHashOrStorage() {
  const stored = localStorage.getItem(TOKEN_KEY) || "";
  const hash = location.hash.startsWith("#") ? location.hash.slice(1) : "";
  const params = new URLSearchParams(hash);
  const fromUrl = params.get("t");
  if (fromUrl && fromUrl.trim()) {
    const t = fromUrl.trim();
    localStorage.setItem(TOKEN_KEY, t);
    const clean = location.pathname + location.search;
    history.replaceState(null, "", clean);
    return t;
  }
  return stored;
}

// ---------- i18n ----------

function pickInitialLang(available) {
  const stored = localStorage.getItem(LANG_KEY);
  if (stored && available.some((l) => l.code === stored)) return stored;
  const nav = (navigator.language || "en").toLowerCase();
  const base = nav.split("-")[0];
  const match = available.find((l) => l.code === base);
  if (match) return match.code;
  return available[0]?.code || "en";
}

async function loadLang(code) {
  try {
    const res = await fetch("/lang/" + code + ".json");
    if (!res.ok) throw new Error("lang " + code + " HTTP " + res.status);
    state.strings = await res.json();
    state.lang = code;
    localStorage.setItem(LANG_KEY, code);
    document.documentElement.lang = code;
  } catch (e) {
    console.warn("lang load failed:", e);
    state.strings = state.strings || {};
  }
}

async function bootI18n() {
  try {
    const res = await fetch("/api/languages");
    if (res.ok) state.langs = await res.json();
  } catch (_) {
    state.langs = [{ code: "en", name: "English" }];
  }
  const pick = pickInitialLang(state.langs);
  await loadLang(pick);
}

/** Translate a key. params values are substituted into `{name}` placeholders. */
function t(key, params) {
  let s = state.strings[key];
  if (s == null) s = key; // fall back to the key so missing translations are visible
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      s = s.replaceAll("{" + k + "}", String(v));
    }
  }
  return s;
}

// ---------- Networking ----------

async function api(path, opts = {}) {
  const headers = new Headers(opts.headers || {});
  if (state.token) headers.set("authorization", "Bearer " + state.token);
  const res = await fetch(path, { ...opts, headers });
  if (res.status === 401) {
    if (state.token) {
      state.token = "";
      localStorage.removeItem(TOKEN_KEY);
    }
    state.needsToken = true;
    render();
    throw new Error("unauthorized");
  }
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(res.status + " " + res.statusText + " " + text);
  }
  if (res.status === 204) return null;
  return res.json();
}

async function pollOnce() {
  try {
    const results = await Promise.allSettled([
      api("/api/status"),
      api("/api/devices"),
      api("/api/volume"),
    ]);
    const [statusResult, devicesResult, volumeResult] = results;
    if (statusResult.status === "rejected" || devicesResult.status === "rejected") {
      const failure = statusResult.status === "rejected" ? statusResult.reason : devicesResult.reason;
      if (failure?.message === "unauthorized") return;
      throw failure;
    }

    const status = statusResult.value;
    const devicesResp = devicesResult.value;
    state.status = status;
    state.devices = devicesResp.devices || [];
    if (volumeResult.status === "fulfilled") {
      if (!volumeControl.active && !volumeControl.timer && !volumeControl.sending && !volumeControl.queued) {
        state.volume = volumeResult.value;
      }
    } else if (volumeResult.reason?.message !== "unauthorized") {
      console.warn("volume poll failed:", volumeResult.reason);
    }
    state.needsToken = false;
    if (state.pendingId) {
      const d = state.devices.find((x) => x.id === state.pendingId);
      if (d && d.is_default_multimedia) state.pendingId = null;
    }
    render();
  } catch (e) {
    if (e.message === "unauthorized") return;
    console.warn("poll failed:", e);
  }
}

function startPolling() {
  stopPolling();
  pollOnce();
  state.pollTimer = setInterval(pollOnce, POLL_MS);
}
function stopPolling() {
  if (state.pollTimer) { clearInterval(state.pollTimer); state.pollTimer = null; }
}

// ---------- Actions ----------

async function setDefault(deviceId) {
  state.pendingId = deviceId;
  render();
  try {
    await api("/api/devices/" + encodeURIComponent(deviceId) + "/default", {
      method: "POST",
    });
    setTimeout(pollOnce, 400);
  } catch (e) {
    state.pendingId = null;
    toast(t("toast.switchFailed", { msg: e.message }), "err");
    render();
    pollOnce();
  }
}

function saveToken(tok) {
  state.token = tok.trim();
  localStorage.setItem(TOKEN_KEY, state.token);
  state.needsToken = false;
  render();
  startPolling();
}

function clearToken() {
  state.token = "";
  localStorage.removeItem(TOKEN_KEY);
  state.status = null;
  state.devices = null;
  state.volume = null;
  state.showSettings = false;
  state.needsToken = true;
  stopPolling();
  render();
}

async function changeLang(code) {
  await loadLang(code);
  render();
}

// ---------- Master volume ---------------------------------------------------

function volumePercent(level) {
  return Math.round(Math.max(0, Math.min(1, Number(level))) * 100);
}

function volumeButtonText(muted) {
  return (muted ? "🔇 " : "🔊 ") + t(muted ? "volume.unmute" : "volume.mute");
}

function volumeButtonLabel(muted) {
  return t(muted ? "volume.unmuteLabel" : "volume.muteLabel");
}

function updateVolumeDom(volume) {
  if (!volume) return;
  const percent = volumePercent(volume.level);
  const slider = document.getElementById("volumeSlider");
  const output = document.getElementById("volumeLevel");
  const muteButton = document.getElementById("volumeMuteButton");
  if (slider) slider.value = String(percent);
  if (output) output.textContent = percent + "%";
  if (muteButton) {
    muteButton.textContent = volumeButtonText(volume.muted);
    muteButton.setAttribute("aria-label", volumeButtonLabel(volume.muted));
    muteButton.setAttribute("aria-pressed", String(volume.muted));
  }
}

function scheduleVolumePost() {
  if (volumeControl.timer) clearTimeout(volumeControl.timer);
  volumeControl.timer = setTimeout(() => {
    volumeControl.timer = null;
    const draft = volumeControl.draft;
    if (!draft) return;
    const request = { revision: volumeControl.revision, ...draft };
    if (volumeControl.sending) {
      volumeControl.queued = request;
    } else {
      void sendVolume(request);
    }
  }, VOLUME_DEBOUNCE_MS);
}

function updateVolumeDraft(patch) {
  const current = volumeControl.draft || state.volume;
  if (!current) return;
  volumeControl.draft = { ...current, ...patch };
  volumeControl.revision += 1;
  volumeControl.active = true;
  state.volume = { ...state.volume, ...patch };
  updateVolumeDom(state.volume);
  scheduleVolumePost();
}

async function refreshVolumeAfterFailure(revision) {
  try {
    const volume = await api("/api/volume");
    if (revision !== volumeControl.revision) return;
    state.volume = volume;
    volumeControl.draft = null;
    updateVolumeDom(volume);
    render();
  } catch (e) {
    if (e.message !== "unauthorized") console.warn("volume refresh failed:", e);
  }
}

async function sendVolume(request) {
  volumeControl.sending = true;
  try {
    const volume = await api("/api/volume", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ level: request.level, muted: request.muted }),
    });
    if (request.revision === volumeControl.revision) {
      state.volume = volume;
      volumeControl.draft = volume;
      updateVolumeDom(volume);
    }
  } catch (e) {
    if (request.revision === volumeControl.revision) {
      volumeControl.active = false;
      toast(t("toast.volumeFailed", { msg: e.message }), "err");
      await refreshVolumeAfterFailure(request.revision);
    }
  } finally {
    volumeControl.sending = false;
    if (volumeControl.queued) {
      const next = volumeControl.queued;
      volumeControl.queued = null;
      void sendVolume(next);
    } else if (request.revision === volumeControl.revision) {
      volumeControl.active = false;
      volumeControl.draft = null;
    }
  }
}

// ---------- Rendering ----------

function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "onclick") el.addEventListener("click", v);
    else if (k === "oninput") el.addEventListener("input", v);
    else if (k === "onchange") el.addEventListener("change", v);
    else if (k === "class") el.className = v;
    else if (k === "html") el.innerHTML = v;
    else el.setAttribute(k, v);
  }
  for (const c of children.flat()) {
    if (c == null) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

function render() {
  app.innerHTML = "";
  app.className = "app";
  if (state.needsToken) return renderTokenEntry();
  if (!state.status || !state.devices) return renderLoading();
  renderMain();
  if (state.showSettings) renderSettingsSheet();
  if (state.showAbout) renderAboutSheet();
}

function renderLoading() {
  app.append(h("div", { class: "loading" }, t("connecting")));
}

function renderTokenEntry() {
  app.append(
    h("div", { class: "center" },
      h("h1", {}, t("brand.name")),
      h("p", {}, t("tokenEntry.hint")),
      h("input", {
        id: "tokenInput", class: "field", type: "text",
        placeholder: "ar_live_…", autocomplete: "off",
      }),
      h("br"),
      h("button", {
        class: "btn",
        onclick: () => {
          const v = document.getElementById("tokenInput").value;
          if (v.trim()) saveToken(v);
        },
      }, t("tokenEntry.button")),
    )
  );
  const inp = document.getElementById("tokenInput");
  if (inp) {
    inp.focus();
    inp.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && e.target.value.trim()) saveToken(e.target.value);
    });
  }
}

function renderMain() {
  const s = state.status;
  const lanBadge = s.lan_exposed
    ? h("span", { class: "badge badge-warn" }, t("header.lanExposed"))
    : null;

  const header = h("div", { class: "header" },
    h("div", { class: "brand" },
      h("img", { src: "/icons/favicon.svg", class: "brand-icon", alt: "" }),
      h("h1", {}, t("brand.name")),
    ),
    h("div", { class: "actions" },
      lanBadge,
      h("button", {
        class: "icon-btn", title: t("header.settingsTitle"),
        onclick: () => { state.showSettings = true; render(); },
      }, "⚙"),
    ),
  );

  const sortName = t("sort." + s.device_sort);

  const meta = h("div", { class: "meta" },
    t("meta.line", { bind: s.bind, port: s.port, sort: sortName }),
  );

  const share = renderSharePanel(s.share_urls || []);
  const volume = renderVolumePanel();

  const activeDevices = state.devices.filter((d) => d.state === "active");
  const hiddenCount = state.devices.length - activeDevices.length;
  const visible = state.showInactive ? state.devices : activeDevices;
  const cards = visible.map((d) => renderDeviceCard(d));

  app.append(header, meta, share, volume, h("div", {}, ...cards));

  if (hiddenCount > 0) {
    app.append(h("button", {
      class: "show-all",
      onclick: () => { state.showInactive = !state.showInactive; render(); },
    }, state.showInactive
      ? t("devices.hideInactive")
      : t("devices.showInactive", { count: hiddenCount })));
  }

  app.append(
    h("div", { class: "footer-note" }, t("devices.footerNote")),
  );
}

function renderVolumePanel() {
  const v = state.volume;
  if (!v) return document.createDocumentFragment();
  const percent = volumePercent(v.level);
  const slider = h("input", {
    id: "volumeSlider",
    class: "volume-slider",
    type: "range",
    min: "0",
    max: "100",
    step: "1",
    value: String(percent),
    "aria-label": t("volume.sliderLabel"),
    oninput: (e) => updateVolumeDraft({ level: Number(e.target.value) / 100 }),
    onchange: () => { volumeControl.active = false; },
  });
  const muteButton = h("button", {
    id: "volumeMuteButton",
    class: "btn volume-mute",
    type: "button",
    "aria-label": volumeButtonLabel(v.muted),
    "aria-pressed": String(v.muted),
    onclick: () => updateVolumeDraft({ muted: !((volumeControl.draft || state.volume).muted) }),
  }, volumeButtonText(v.muted));

  return h("section", { class: "volume-panel", "aria-labelledby": "volumeTitle" },
    h("div", { class: "volume-head" },
      h("div", {},
        h("h2", { id: "volumeTitle" }, t("volume.title")),
        h("div", { class: "volume-hint" }, t("volume.hint")),
      ),
      h("output", { id: "volumeLevel", class: "volume-level", for: "volumeSlider" }, percent + "%"),
    ),
    slider,
    muteButton,
  );
}

function renderSharePanel(entries) {
  const isLoopback = /^https?:\/\/(127\.0\.0\.1|localhost|\[::1\])(:|\/|$)/.test(location.origin);
  if (!isLoopback) return document.createDocumentFragment();
  if (!entries.length) return document.createDocumentFragment();

  const wrap = h("div", { class: "share" },
    h("div", { class: "share-title" }, t("share.title")),
    h("div", { class: "share-meta" }, t("share.hint")),
  );
  entries.forEach((e) => {
    const url = e.url;
    const iface = e.interface || "";
    const isVirtual = !!e.virtual_iface;

    const urlInput = h("input", { class: "share-url", type: "text", readonly: "readonly", value: url });
    const row = h("div", { class: "share-row" },
      urlInput,
      h("button", {
        class: "btn small",
        onclick: (evt) => {
          navigator.clipboard.writeText(url).then(
            () => {
              const btn = evt.currentTarget;
              const orig = btn.textContent;
              // Green flash + bounce on the button, plus a soft highlight on the
              // URL input so the user sees "this is what got copied".
              btn.textContent = "✓ " + t("share.copied");
              btn.classList.add("copied");
              urlInput.classList.remove("copied-flash");
              // Re-trigger animation by forcing reflow.
              void urlInput.offsetWidth;
              urlInput.classList.add("copied-flash");
              setTimeout(() => {
                btn.textContent = orig;
                btn.classList.remove("copied");
              }, 1600);
            },
            () => { toast(t("share.copyFailed"), "err"); }
          );
        },
      }, t("share.copy")),
    );

    const label = h("div", { class: "share-iface" + (isVirtual ? " virtual" : "") },
      iface + (isVirtual ? t("share.virtualSwitch") : ""),
    );

    wrap.append(row, label);
  });
  return wrap;
}

function renderDeviceCard(d) {
  const optimisticSelected = state.pendingId
    ? d.id === state.pendingId
    : d.is_default_multimedia;
  const cls = [
    "card",
    optimisticSelected ? "selected" : "",
    d.state !== "active" ? "dim" : "",
    state.pendingId === d.id ? "busy" : "",
  ].filter(Boolean).join(" ");

  const stateLabel = t("deviceState." + d.state);

  const roleNames = [];
  if (d.is_default_console) roleNames.push(t("roles.console"));
  if (d.is_default_multimedia) roleNames.push(t("roles.multimedia"));
  if (d.is_default_communications) roleNames.push(t("roles.communications"));

  const sub = roleNames.length
    ? stateLabel + " / " + t("roles.default_prefix") + roleNames.join(", ")
    : stateLabel;

  return h(
    "div",
    {
      class: cls,
      onclick: () => {
        if (d.state !== "active") return;
        if (state.pendingId === d.id) return;
        if (optimisticSelected && roleNames.length === 3) return;
        setDefault(d.id);
      },
    },
    h("div", { class: "dot" + (optimisticSelected ? " on" : "") }),
    h("div", { class: "info" },
      h("div", { class: "name" }, d.name || "(unnamed)"),
      h("div", { class: "sub" }, sub),
    ),
    state.pendingId === d.id
      ? h("div", { class: "status meta" }, t("devices.applying"))
      : null,
  );
}

function renderSettingsSheet() {
  const s = state.status || {};
  const tokenPreview = state.token.length > 12
    ? state.token.slice(0, 8) + "…" + state.token.slice(-4)
    : state.token;

  const langSelect = h("select", {
    class: "field small",
    onchange: (e) => { changeLang(e.target.value); },
  },
    ...state.langs.map((l) =>
      h("option", { value: l.code, ...(l.code === state.lang ? { selected: "selected" } : {}) },
        l.name),
    ),
  );

  const sheet = h("div", { class: "sheet-bg", onclick: (e) => {
    if (e.target.classList.contains("sheet-bg")) {
      state.showSettings = false; render();
    }
  }},
    h("div", { class: "sheet" },
      h("h2", {}, t("settings.title")),
      h("div", { class: "row" },
        h("div", { class: "label" }, t("settings.server")),
        h("div", { class: "val" }, (s.bind || "?") + ":" + (s.port || "?")),
      ),
      h("div", { class: "row" },
        h("div", { class: "label" }, t("settings.version")),
        h("div", { class: "val" }, s.version || "?"),
      ),
      h("div", { class: "row" },
        h("div", { class: "label" }, t("settings.sort")),
        h("div", { class: "val" }, s.device_sort || "?"),
      ),
      h("div", { class: "row" },
        h("div", { class: "label" }, t("settings.token")),
        h("div", { class: "val" }, tokenPreview || t("settings.tokenEmpty")),
      ),
      h("div", { class: "row" },
        h("div", { class: "label" }, t("settings.lanExposed")),
        h("div", { class: "val" }, s.lan_exposed ? t("settings.lanExposed.yes") : t("settings.lanExposed.no")),
      ),
      h("div", { class: "row" },
        h("div", { class: "label" }, t("settings.language")),
        h("div", { class: "val" }, langSelect),
      ),
      h("div", { class: "row" },
        h("div", { class: "label" }, t("settings.about")),
        h("button", { class: "btn ghost small",
          onclick: () => {
            state.showSettings = false;
            state.showAbout = true;
            render();
            loadAbout();
          },
        }, t("settings.aboutOpen")),
      ),
      h("div", { style: "text-align:right; margin-top:16px" },
        h("button", { class: "btn ghost small", style: "margin-right:8px",
          onclick: () => { state.showSettings = false; render(); },
        }, t("settings.close")),
        h("button", { class: "btn small",
          onclick: () => {
            if (confirm(t("settings.reissueConfirm"))) {
              clearToken();
            }
          },
        }, t("settings.reissueToken")),
      ),
    ),
  );
  app.append(sheet);
}

async function loadAbout() {
  if (state.about) return;
  try {
    state.about = await api("/api/about");
    if (state.showAbout) render();
  } catch (e) {
    console.warn("about fetch failed:", e);
  }
}

function renderAboutSheet() {
  const bg = h("div", {
    class: "sheet-bg",
    onclick: (e) => {
      if (e.target.classList.contains("sheet-bg")) {
        state.showAbout = false; render();
      }
    },
  });

  const sheet = h("div", { class: "sheet about" });

  sheet.append(
    h("div", { class: "about-head" },
      h("h2", {}, t("about.title")),
      h("button", { class: "icon-btn", title: t("settings.close"),
        onclick: () => { state.showAbout = false; render(); },
      }, "×"),
    ),
  );

  if (!state.about) {
    sheet.append(h("div", { class: "loading" }, t("loading")));
    bg.append(sheet); app.append(bg); return;
  }

  const a = state.about.app || {};

  sheet.append(
    h("div", { class: "about-block" },
      h("div", { class: "about-name" }, a.name || "audioremote"),
      h("div", { class: "meta" }, t("about.versionRow", { version: a.version || "?" })),
      h("p", {}, a.description || ""),
    ),
  );

  sheet.append(
    h("div", { class: "about-block" },
      h("div", { class: "about-h" }, t("about.licenseSection")),
      h("div", {}, t("about.licenseBody", {
        name: a.name || "audioremote",
        license: a.license || "MIT",
        copyright: a.copyright || "",
      })),
    ),
  );

  if (a.frontend_note) {
    sheet.append(
      h("div", { class: "about-block" },
        h("div", { class: "about-h" }, t("about.frontendSection")),
        h("div", {}, a.frontend_note),
      ),
    );
  }

  const oss = state.about.oss || [];
  const listBox = h("div", { class: "about-block" },
    h("div", { class: "about-h" }, t("about.ossSection", { count: oss.length })),
  );
  oss.forEach((o) => {
    listBox.append(
      h("div", { class: "oss-row" },
        h("div", { class: "oss-name" }, o.name + " " + o.version),
        h("div", { class: "oss-meta" },
          o.license + (o.purpose ? " · " + o.purpose : "")),
      ),
    );
  });
  sheet.append(listBox);

  bg.append(sheet);
  app.append(bg);
}

// ---------- Toast ----------

let toastTimer = null;
function toast(msg, kind = "") {
  toastEl.textContent = msg;
  toastEl.className = "toast" + (kind ? " " + kind : "");
  toastEl.hidden = false;
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { toastEl.hidden = true; }, 2400);
}

// ---------- Boot ----------

(async function boot() {
  await bootI18n();
  render();
  startPolling();
})();

document.addEventListener("visibilitychange", () => {
  if (document.hidden) stopPolling();
  else startPolling();
});
