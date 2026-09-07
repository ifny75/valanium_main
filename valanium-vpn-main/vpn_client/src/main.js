const hasTauri = Boolean(window.__TAURI__);
const invoke = hasTauri
  ? window.__TAURI__.core.invoke
  : async (cmd, args) => mockInvoke(cmd, args);

// WebView2 shows its own right-click menu (Reload / Inspect / ...) by
// default, which has no place in a shipped desktop app.
document.addEventListener("contextmenu", (e) => e.preventDefault());

// Fallback mock backend so the UI can be previewed in a plain browser
// (no Rust/Tauri build needed) — mirrors src-tauri/src/servers.rs + vpn.rs.
// Flat list, grouped by country_code for display — map_x/map_y are real
// geographic positions (Mercator-projected capital coordinates, matching
// familiar web maps) as a percentage of assets/map-fill.svg's viewBox, see
// src/assets/build_map.py, REGION_VIEWBOX — shared by every server in the
// same country (no per-city coordinates).
const MOCK_SERVERS = [
  { id: "de-fra-01", name: "Frankfurt-01", country_name: "Germany", country_code: "de", load_percent: 5, ping_ms: 23, is_current: false, map_x: 48.5, map_y: 57.7 },
  { id: "nl-ams-01", name: "Amsterdam-01", country_name: "Netherlands", country_code: "nl", load_percent: 5, ping_ms: 23, is_current: false, map_x: 40.1, map_y: 57.9 },
  { id: "fi-hel-01", name: "Helsinki-01", country_name: "Finland", country_code: "fi", load_percent: 5, ping_ms: 23, is_current: false, map_x: 59.9, map_y: 42.1 },
  { id: "se-sto-01", name: "Stockholm-01", country_name: "Sweden", country_code: "se", load_percent: 5, ping_ms: 23, is_current: true, map_x: 53.2, map_y: 44.1 },
  { id: "se-sto-02", name: "Stockholm-02", country_name: "Sweden", country_code: "se", load_percent: 12, ping_ms: 26, is_current: false, map_x: 53.2, map_y: 44.1 },
];
// Starts disconnected, matching src-tauri/src/vpn.rs's VpnManager::new() —
// so the mock and the real backend agree on what "just launched" looks like.
let mockState = { status: "disconnected", server: null };
let mockSessionActive = false;

// Поддельный аккаунт для просмотра интерфейса в обычном браузере. Повторяет
// форму ответов /v1 (см. main_server/app/routers/v1.py): ни имени, ни почты
// в нём нет — сервер их не знает.
const MOCK_ACCOUNT = {
  expires_at: "2027-01-31T00:00:00",
  is_active: true,
  plan_name: "Standard",
  traffic_limit_gb: 0,
  traffic_used_gb: 12.4,
  max_devices: 5,
  devices_used: 3,
};
let mockDevices = [
  { device_pub: "9f2c41aa5d7e0b3364c1f0a9d2b784e5cc1029384756abcdef0123456789abcd", created_at: "2026-08-02T10:11:00", is_current: true },
  { device_pub: "1a7b3c5d9e0f2143658796a8b9cadbecfd0e1f2031425364758697a8b9cadbec", created_at: "2026-08-14T22:03:00", is_current: false },
  { device_pub: "44556677889900aabbccddeeff0011223344556677889900aabbccddeeff0011", created_at: "2026-09-01T08:40:00", is_current: false },
];
let mockEnrolled = false;

async function mockInvoke(cmd, args) {
  await new Promise((r) => setTimeout(r, 120));
  if (cmd === "list_servers") return MOCK_SERVERS;
  if (cmd === "get_status") return mockState;
  if (cmd === "connect") {
    if (!mockSessionActive) throw new Error("Войдите кодом доступа, прежде чем подключаться");
    const server = MOCK_SERVERS.find((s) => s.id === args.serverId);
    mockState = { status: "connected", server };
    return mockState;
  }
  if (cmd === "disconnect") {
    mockState = { status: "disconnected", server: null };
    return mockState;
  }
  if (cmd === "enroll_code") {
    // Контрольную сумму настоящего кода здесь не проверить — она считается
    // на сервере (services/access_code.py). Для просмотра интерфейса хватает
    // длины: так видно и удачный путь, и текст ошибки.
    const code = String(args.code ?? "").replace(/[\s-]/g, "");
    if (code.length !== 20) throw new Error("Код введён с ошибкой — проверьте последнюю группу");
    mockEnrolled = true;
    mockSessionActive = true;
    return MOCK_ACCOUNT;
  }
  if (cmd === "sign_in") {
    if (!mockEnrolled) throw new Error("Нет сессии — войдите кодом доступа");
    mockSessionActive = true;
    return MOCK_ACCOUNT;
  }
  if (cmd === "get_account") return MOCK_ACCOUNT;
  if (cmd === "sign_out") {
    mockEnrolled = false;
    mockSessionActive = false;
    return null;
  }
  if (cmd === "list_devices") return mockDevices;
  if (cmd === "revoke_device") {
    const target = mockDevices.find((d) => d.device_pub === args.devicePub);
    if (!target) throw new Error("Такого устройства нет");
    mockDevices = mockDevices.filter((d) => d.device_pub !== args.devicePub);
    return mockDevices;
  }
  throw new Error(`Unknown mock command: ${cmd}`);
}

const els = {
  app: document.getElementById("app"),
  authScreen: document.getElementById("authScreen"),
  codeForm: document.getElementById("codeForm"),
  accessCode: document.getElementById("accessCode"),
  codeSubmit: document.getElementById("codeSubmit"),
  logoutBtn: document.getElementById("logoutBtn"),
  accountPlan: document.getElementById("accountPlan"),
  accountExpires: document.getElementById("accountExpires"),
  accountTraffic: document.getElementById("accountTraffic"),
  deviceList: document.getElementById("deviceList"),
  deviceCount: document.getElementById("deviceCount"),
  accountStatusView: document.getElementById("accountStatusView"),
  accountStatusIcon: document.getElementById("accountStatusIcon"),
  accountStatusTitle: document.getElementById("accountStatusTitle"),
  accountStatusMessage: document.getElementById("accountStatusMessage"),
  accountStatusBack: document.getElementById("accountStatusBack"),
  toastContainer: document.getElementById("toastContainer"),
  mapPane: document.querySelector(".map-pane"),
  mapViewport: document.getElementById("mapViewport"),
  mapBg: document.getElementById("mapBg"),
  markers: document.getElementById("markers"),
  serverList: document.getElementById("serverList"),
  search: document.getElementById("search"),
  connectFab: document.getElementById("connectFab"),
  connectLabel: document.getElementById("connectLabel"),
  settingsBtn: document.getElementById("settingsBtn"),
  settingsOverlay: document.getElementById("settingsOverlay"),
  settingsPanel: document.querySelector(".settings-panel"),
  settingsCloseBtn: document.getElementById("settingsCloseBtn"),
};

let servers = [];
let status = { status: "disconnected", server: null };

// Must match REGION_VIEWBOX's width/height in tools/build_map.py — the
// intrinsic aspect ratio assets/map-fill.svg is rendered at under
// `background-size: cover`.
const MAP_NATURAL_W = 280;
const MAP_NATURAL_H = 248;

// Replicates the browser's own `background-size: cover; background-position:
// center` math for .map-bg, so a server's map_x/map_y (percent of the SVG's
// viewBox) can be converted to an exact pixel inside .map-pane. Percentage-
// based left/top on the marker looked right at one window size and drifted
// off the coastline at any other, because `cover` crops non-uniformly once
// the pane's aspect ratio stops matching the map's — this tracks the actual
// rendered/cropped image rect instead of guessing.
function mapCoverGeometry() {
  const rect = els.mapPane.getBoundingClientRect();
  // The very first layout pass (before the webview has committed a real
  // size) can report a 0×0 rect. Computing off that produces a garbage
  // position that then never gets corrected if the window is never
  // resized afterward — bail out and let the caller keep waiting instead.
  if (rect.width <= 0 || rect.height <= 0) return null;
  const scale = Math.max(rect.width / MAP_NATURAL_W, rect.height / MAP_NATURAL_H);
  const renderedW = MAP_NATURAL_W * scale;
  const renderedH = MAP_NATURAL_H * scale;
  return {
    paneW: rect.width,
    paneH: rect.height,
    offsetX: (rect.width - renderedW) / 2,
    offsetY: (rect.height - renderedH) / 2,
    renderedW,
    renderedH,
  };
}

// Pixel position of a server's map_x/map_y within .map-pane (and thus
// .map-viewport, which exactly overlays it before its zoom transform).
// Returns null when the pane doesn't have a real size yet (see above).
function mapPointPx(server) {
  const g = mapCoverGeometry();
  if (!g) return null;
  return {
    x: g.offsetX + (server.map_x / 100) * g.renderedW,
    y: g.offsetY + (server.map_y / 100) * g.renderedH,
    paneW: g.paneW,
    paneH: g.paneH,
  };
}

function flagHtml(code) {
  if (code === "de") return `<div class="flag de"><span></span><span></span><span></span></div>`;
  if (code === "nl") return `<div class="flag nl"><span></span><span></span><span></span></div>`;
  if (code === "se") return `<div class="flag se"></div>`;
  if (code === "fi") return `<div class="flag fi"></div>`;
  return `<div class="flag"></div>`;
}

// Countries the user has manually toggled open/closed. Starts empty —
// whichever country holds the active server auto-expands until the user
// touches a header themselves (see groupsToRender).
const expandedCountries = new Set();
let expandedTouched = false;

function groupByCountry(list) {
  const groups = [];
  const byCode = new Map();
  for (const s of list) {
    let g = byCode.get(s.country_code);
    if (!g) {
      g = { country_code: s.country_code, country_name: s.country_name, servers: [] };
      byCode.set(s.country_code, g);
      groups.push(g);
    }
    g.servers.push(s);
  }
  return groups;
}

function serverRowHtml(s) {
  const isCurrent = status.server?.id === s.id && status.status !== "disconnected";
  return `
    <div class="server-card sub ${isCurrent ? "current" : ""}" data-id="${s.id}">
      <div class="server-info">
        <div class="server-name">${s.name}${isCurrent ? " (Current)" : ""}</div>
        <div class="server-meta">${serverMetaText(s)}</div>
      </div>
    </div>`;
}

function renderServerList(filter = "") {
  const q = filter.trim().toLowerCase();
  const list = servers.filter(
    (s) => s.name.toLowerCase().includes(q) || s.country_name.toLowerCase().includes(q)
  );
  const groups = groupByCountry(list);

  // Until the user has clicked a header themselves, auto-expand whichever
  // country holds the active/current server so it's visible on first paint.
  if (!expandedTouched) {
    const active = status.server ?? servers.find((s) => s.is_current);
    if (active) expandedCountries.add(active.country_code);
  }

  els.serverList.innerHTML = groups
    .map((g) => {
      const isOpen = expandedCountries.has(g.country_code) || Boolean(q);
      const hasCurrent = g.servers.some(
        (s) => status.server?.id === s.id && status.status !== "disconnected"
      );
      return `
        <div class="country-group">
          <button type="button" class="country-header ${hasCurrent ? "current" : ""}" data-code="${g.country_code}">
            ${flagHtml(g.country_code)}
            <span class="country-name">${g.country_name}</span>
            <span class="country-count">${g.servers.length}</span>
            <svg class="chevron ${isOpen ? "open" : ""}" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M9 6l6 6-6 6"></path>
            </svg>
          </button>
          <div class="server-sublist ${isOpen ? "open" : ""}">
            <div class="server-sublist-inner">
              ${g.servers.map(serverRowHtml).join("")}
            </div>
          </div>
        </div>`;
    })
    .join("");

  els.serverList.querySelectorAll(".country-header").forEach((header) => {
    header.addEventListener("click", () => {
      expandedTouched = true;
      const code = header.dataset.code;
      if (expandedCountries.has(code)) expandedCountries.delete(code);
      else expandedCountries.add(code);
      renderServerList(els.search.value);
    });
  });

  els.serverList.querySelectorAll(".server-card[data-id]").forEach((card) => {
    card.addEventListener("click", () => connectTo(card.dataset.id));
  });
}

// A single marker glides between server positions — the coastline art is
// decorative, not a real projection, so multiple simultaneous pins had no
// meaningful distinct locations and only ended up overlapping each other.
function renderMarkers() {
  const active = status.server ?? servers.find((s) => s.is_current) ?? servers[0];
  if (!active) {
    els.markers.innerHTML = "";
    return;
  }

  const isLive = status.status !== "disconnected";
  if (!els.markers.querySelector(".marker")) {
    els.markers.innerHTML = `
      <div class="marker-shadow"></div>
      <div class="marker" title=""></div>`;
  }

  const marker = els.markers.querySelector(".marker");
  const shadow = els.markers.querySelector(".marker-shadow");
  marker.classList.toggle("dim", !isLive);
  marker.title = `${active.name}, ${active.country_name}`;

  const p = mapPointPx(active);
  if (!p) return; // pane not laid out yet — retried via ResizeObserver/rAF
  marker.style.left = shadow.style.left = `${p.x}px`;
  marker.style.top = shadow.style.top = `${p.y}px`;
}

function renderFab() {
  els.connectFab.dataset.status = status.status;
  els.connectLabel.textContent =
    status.status === "connected"
      ? `Подключено · ${status.server?.name ?? ""}, ${status.server?.country_name ?? ""}`
      : status.status === "connecting"
      ? "Подключение…"
      : "Подключиться";
}

// How far the camera zooms in on the active server. Only applied once a
// server is actually being connected to / connected — at rest (disconnected)
// the map sits at its plain, un-zoomed crop, so "zooming in" reads as a
// deliberate move rather than the map always looking cropped-in tight.
const CAMERA_ZOOM = 1.18;

// Drives both the ambient glow dome and the camera. Panning to a new point
// is expressed as translate()+scale() (not transform-origin, see the CSS
// comment on .map-viewport) so the browser tweens it as one smooth matrix
// interpolation instead of a jump cut.
function renderGlow() {
  const active = status.server ?? servers.find((s) => s.is_current) ?? servers[0];
  if (!active) return;
  const p = mapPointPx(active);
  if (!p) return; // pane not laid out yet — retried via ResizeObserver/rAF

  els.mapPane.style.setProperty("--spot-x", `${(p.x / p.paneW) * 100}%`);
  els.mapPane.style.setProperty("--spot-y", `${(p.y / p.paneH) * 100}%`);

  const zoom = status.status === "disconnected" ? 1 : CAMERA_ZOOM;
  // Zoom while keeping (p.x, p.y) fixed on screen: with transform-origin
  // at 0 0, that's translate(p*(1-zoom)) then scale(zoom) — at zoom=1 this
  // is just the identity transform, i.e. the plain, un-zoomed crop.
  const tx = p.x * (1 - zoom);
  const ty = p.y * (1 - zoom);
  els.mapViewport.style.transform = `translate(${tx}px, ${ty}px) scale(${zoom})`;
}

function renderAll() {
  renderServerList(els.search.value);
  renderMarkers();
  renderFab();
  renderGlow();
}

async function connectTo(id) {
  status = { status: "connecting", server: servers.find((s) => s.id === id) ?? null };
  renderAll();
  try {
    status = await invoke("connect", { serverId: id });
  } catch (e) {
    console.error(e);
    status = { status: "disconnected", server: null };
  }
  renderAll();
}

async function toggleConnection() {
  if (status.status === "connected") {
    status = await invoke("disconnect");
  } else if (status.status === "disconnected" && status.server) {
    await connectTo(status.server.id);
    return;
  } else if (status.status === "disconnected" && servers.length) {
    await connectTo(servers.find((s) => s.is_current)?.id ?? servers[0].id);
    return;
  }
  renderAll();
}

// ---------- Settings ----------

const SETTINGS_KEY = "valanium.settings";
const DEFAULT_SETTINGS = {
  launchAtStartup: false,
  autoConnect: false,
  // Ни один из трёх пока ничем не подкреплён — переключатели в интерфейсе
  // отключены, а значение по умолчанию не должно обещать защиту, которой нет.
  killSwitch: false,
  notifications: true,
};

function loadSettings() {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    return { ...DEFAULT_SETTINGS, ...JSON.parse(raw) };
  } catch (e) {
    console.error("Failed to read settings, using defaults", e);
    return { ...DEFAULT_SETTINGS };
  }
}

function saveSettings(settings) {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch (e) {
    console.error("Failed to persist settings", e);
  }
}

let appSettings = loadSettings();

function renderSettingsForm() {
  els.settingsPanel.querySelectorAll(".switch[data-setting]").forEach((input) => {
    input.checked = Boolean(appSettings[input.dataset.setting]);
  });
}

// ---------- DNS setting ----------
// Lives in Rust (dns_settings.rs), not localStorage: the tray's own connect
// flow never touches this webview, so the choice has to be readable from
// the backend for both the window's Connect button and the tray toggle to
// apply the same DNS.

const dnsModeSelect = document.getElementById("dnsModeSelect");
const dnsCustomRow = document.getElementById("dnsCustomRow");
const dnsCustomPrimary = document.getElementById("dnsCustomPrimary");
const dnsCustomSecondary = document.getElementById("dnsCustomSecondary");

async function loadDnsForm() {
  if (!hasTauri) return;
  try {
    const dns = await invoke("get_dns_settings");
    dnsModeSelect.value = dns.mode;
    dnsCustomPrimary.value = dns.customPrimary ?? "";
    dnsCustomSecondary.value = dns.customSecondary ?? "";
    dnsCustomRow.hidden = dns.mode !== "custom";
  } catch (e) {
    console.error("Failed to load DNS settings", e);
  }
}

async function saveDnsForm() {
  if (!hasTauri) return;
  const settings = {
    mode: dnsModeSelect.value,
    customPrimary: dnsCustomPrimary.value.trim(),
    customSecondary: dnsCustomSecondary.value.trim(),
  };
  try {
    await invoke("set_dns_settings", { settings });
  } catch (e) {
    console.error("Failed to save DNS settings", e);
  }
}

dnsModeSelect.addEventListener("change", () => {
  dnsCustomRow.hidden = dnsModeSelect.value !== "custom";
  saveDnsForm();
});
dnsCustomPrimary.addEventListener("change", saveDnsForm);
dnsCustomSecondary.addEventListener("change", saveDnsForm);

// ---------- HTTPS-mask setting ----------
// Same reasoning as DNS above: lives in Rust so the tray's connect flow
// picks it up too, not just the window's own Connect button.

const maskHttpsToggle = document.getElementById("maskHttpsToggle");

async function loadMaskForm() {
  if (!hasTauri) return;
  try {
    const mask = await invoke("get_mask_settings");
    maskHttpsToggle.checked = Boolean(mask.enabled);
  } catch (e) {
    console.error("Failed to load mask settings", e);
  }
}

maskHttpsToggle.addEventListener("change", async () => {
  if (!hasTauri) return;
  try {
    await invoke("set_mask_settings", { settings: { enabled: maskHttpsToggle.checked } });
  } catch (e) {
    console.error("Failed to save mask settings", e);
  }
});

// Real device-type icons (desktop / laptop / phone) rather than a letter
// badge — keyed by platform, falling back to a generic desktop glyph.
const DEVICE_ICONS = {
  Windows: `<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="12" rx="1.5"/><path d="M8 20h8M12 16v4"/></svg>`,
  Linux: `<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="12" rx="1.5"/><path d="M8 20h8M12 16v4"/></svg>`,
  macOS: `<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="4" width="16" height="10" rx="1.2"/><path d="M2 18h20l-1.5-3h-17z"/></svg>`,
  iOS: `<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="2" width="10" height="20" rx="2"/><path d="M11 18h2"/></svg>`,
  Android: `<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="2" width="10" height="20" rx="2"/><path d="M11 18h2"/></svg>`,
};

// Ноль означает «не измеряли», а не «мгновенно» и не «пусто». Раньше клиент
// сам мерил время до открытого порта панели, но панель больше не смотрит в
// мир, а адреса узлов клиенту не отдаются намеренно (ARCHITECTURE.md §10):
// настоящие числа должен присылать сам узел. Пока их нет, честнее не рисовать
// строку вовсе, чем показывать «Ping: 0ms».
function serverMetaText(s) {
  const parts = [];
  if (s.load_percent > 0) parts.push(`Загрузка: ${s.load_percent}%`);
  if (s.ping_ms > 0) parts.push(`Отклик: ${s.ping_ms} мс`);
  return parts.join(" &nbsp;·&nbsp; ");
}

function deviceIconHtml(platform) {
  return DEVICE_ICONS[platform] ?? DEVICE_ICONS.Windows;
}

// Имя устройства придумывает клиент из его же публичного ключа. Сервер имён
// не хранит: любое поле, куда можно записать «ноутбук Ивана», однажды им
// заполнят. Пара слов детерминирована, поэтому одно и то же устройство
// называется одинаково на всех экранах, где его видно.
const DEVICE_ADJECTIVES = [
  "тихий", "быстрый", "южный", "медный", "синий", "дальний", "ночной", "ровный",
  "серый", "лёгкий", "верхний", "тёплый", "острый", "полный", "чистый", "прямой",
];
const DEVICE_NOUNS = [
  "сокол", "барс", "невод", "маяк", "ветер", "кедр", "залив", "гранит",
  "янтарь", "ручей", "утёс", "иней", "омут", "клевер", "прибой", "камыш",
];

function deviceName(devicePub) {
  const a = parseInt(devicePub.slice(0, 2), 16) % DEVICE_ADJECTIVES.length;
  const n = parseInt(devicePub.slice(2, 4), 16) % DEVICE_NOUNS.length;
  return `${DEVICE_ADJECTIVES[a]}-${DEVICE_NOUNS[n]}`;
}

/// Короткий отпечаток: имён из двух слов на 256 сочетаний не хватит, чтобы
/// различить устройства наверняка, а по восьми знакам ключа видно точно.
function deviceFingerprint(devicePub) {
  return devicePub.slice(0, 8).replace(/(.{4})(.{4})/, "$1 $2");
}

function deviceRowHtml(device) {
  return `
    <div class="device-row ${device.is_current ? "current" : ""}">
      <div class="device-icon">${deviceIconHtml("Windows")}</div>
      <div class="device-info">
        <div class="device-name">${deviceName(device.device_pub)}</div>
        <div class="device-meta">${deviceFingerprint(device.device_pub)} · добавлено ${formatDate(device.created_at)}</div>
      </div>
      ${
        device.is_current
          ? `<span class="device-badge">Это устройство</span>`
          : `<button type="button" class="device-remove" data-pub="${device.device_pub}">Отвязать</button>`
      }
    </div>`;
}

async function renderDevices() {
  const devices = await invoke("list_devices");
  const max = currentAccount?.max_devices ?? devices.length;
  els.deviceList.innerHTML = devices.map(deviceRowHtml).join("");
  els.deviceCount.textContent = `${devices.length}/${max}`;
  els.deviceCount.classList.toggle("at-limit", devices.length >= max);

  els.deviceList.querySelectorAll(".device-remove").forEach((btn) => {
    btn.addEventListener("click", async () => {
      btn.disabled = true;
      try {
        await invoke("revoke_device", { devicePub: btn.dataset.pub });
        showToast("Устройство отвязано.", "success");
        renderDevices();
      } catch (err) {
        showToast(String(err.message ?? err) || "Не удалось отвязать устройство.", "error");
        btn.disabled = false;
      }
    });
  });
}

function switchSettingsTab(tab) {
  els.settingsPanel.querySelectorAll(".settings-tab").forEach((btn) => {
    const active = btn.dataset.tab === tab;
    btn.classList.toggle("active", active);
    btn.setAttribute("aria-selected", String(active));
  });
  els.settingsPanel.querySelectorAll(".settings-panel-tab").forEach((panel) => {
    panel.hidden = panel.dataset.panel !== tab;
  });
}

els.settingsPanel.querySelectorAll(".settings-tab").forEach((btn) => {
  btn.addEventListener("click", () => switchSettingsTab(btn.dataset.tab));
});

function openSettings() {
  renderSettingsForm();
  renderDevices();
  loadDnsForm();
  loadMaskForm();
  switchSettingsTab("general");
  els.settingsOverlay.hidden = false;
}

function closeSettings() {
  els.settingsOverlay.hidden = true;
}

els.settingsPanel.querySelectorAll(".switch[data-setting]").forEach((input) => {
  input.addEventListener("change", () => {
    appSettings = { ...appSettings, [input.dataset.setting]: input.checked };
    saveSettings(appSettings);
  });
});

els.settingsBtn.addEventListener("click", openSettings);
els.settingsCloseBtn.addEventListener("click", closeSettings);
els.settingsOverlay.addEventListener("click", (e) => {
  if (e.target === els.settingsOverlay) closeSettings();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !els.settingsOverlay.hidden) closeSettings();
});

// ---------- Wiring ----------

els.search.addEventListener("input", () => renderServerList(els.search.value));
els.connectFab.addEventListener("click", toggleConnection);

// Re-derive marker/glow pixel positions whenever the window is resized —
// the pane's aspect ratio (and thus the cover-crop of the map) changes.
// ResizeObserver also fires once as soon as it starts observing, which is
// what normally recovers from the pane reporting 0×0 on the very first
// layout pass — but if that guaranteed first callback itself lands before
// layout is ready, there may be no *further* resize to retry on, so
// ensureMapPositioned() below backs it up with a short rAF retry loop.
new ResizeObserver(() => {
  renderMarkers();
  renderGlow();
}).observe(els.mapPane);

function ensureMapPositioned(attemptsLeft = 10) {
  renderMarkers();
  renderGlow();
  const rect = els.mapPane.getBoundingClientRect();
  if ((rect.width <= 0 || rect.height <= 0) && attemptsLeft > 0) {
    requestAnimationFrame(() => ensureMapPositioned(attemptsLeft - 1));
  }
}

let appStarted = false;

async function initApp() {
  if (appStarted) return; // logging out and back in shouldn't re-wire listeners
  appStarted = true;

  servers = await invoke("list_servers");
  status = await invoke("get_status");
  renderAll();
  ensureMapPositioned();

  // The tray menu's Connect/Disconnect toggles VPN state independently of
  // this window — listen for its broadcast so the UI stays in sync instead
  // of only updating on the next click inside the window.
  if (hasTauri) {
    window.__TAURI__.event
      .listen("vpn-status-changed", (event) => {
        status = event.payload;
        renderAll();
      })
      .catch((err) => {
        // If this silently fails to register (e.g. a missing "core:event:default"
        // capability grant — see src-tauri/capabilities/default.json), the tray's
        // Connect/Disconnect never reaches the window and the two look out of
        // sync, so surface it instead of failing quietly.
        console.error("Failed to listen for vpn-status-changed", err);
      });
  }
}

// ---------- Toast notifications ----------
//
// Bottom-right, auto-dismissing — replaces the old inline red banner on the
// auth forms, and doubles as the general "Desktop notifications" surface
// (connect/disconnect/error) the settings toggle already promises.

const TOAST_ICONS = { error: "!", success: "✓", info: "i" };

function showToast(message, type = "info", duration = 4500) {
  if (type !== "error" && appSettings && !appSettings.notifications) return;

  const toast = document.createElement("div");
  toast.className = `toast ${type}`;
  toast.innerHTML = `
    <span class="toast-icon">${TOAST_ICONS[type] ?? TOAST_ICONS.info}</span>
    <span class="toast-message"></span>`;
  toast.querySelector(".toast-message").textContent = message;

  const remove = () => {
    toast.classList.add("leaving");
    toast.addEventListener("animationend", () => toast.remove(), { once: true });
  };
  toast.addEventListener("click", remove);
  els.toastContainer.appendChild(toast);
  setTimeout(remove, duration);
}

// ---------- Вход по коду доступа ----------
//
// Учётная запись — это код. Он тратится один раз, на привязку устройства
// (`enroll_code`), и дальше клиент входит подписью своего ключа (`sign_in`,
// src-tauri/src/identity.rs). Ни имени, ни почты, ни пароля здесь нет —
// сервер их не знает, хранить нечего.
//
// Сессии в localStorage больше нет: возвращаться к серверу с ключом дешевле
// и честнее, чем держать на диске то, что можно украсть без ключа.

let currentAccount = null;

function formatDate(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleDateString("ru-RU", { year: "numeric", month: "long", day: "numeric" });
}

/// Почему аккаунт не пускает внутрь. Сервер отвечает одним признаком
/// `is_active`, а причина видна по остальным полям: истёкший срок и
/// приостановленный доступ — разные разговоры с человеком.
function accountBlockReason(account) {
  if (account.is_active) return null;
  if (new Date(account.expires_at).getTime() < Date.now()) {
    return {
      title: "Срок доступа истёк",
      message: "Действие этого кода закончилось. Продлить его может тот, кто его выдал.",
    };
  }
  return {
    title: "Доступ приостановлен",
    message: "Этот аккаунт отключён. Если это ошибка — скажите тому, кто выдал код.",
  };
}

function showAccountStatus(account) {
  const reason = accountBlockReason(account);
  if (!reason) return false;
  els.accountStatusTitle.textContent = reason.title;
  els.accountStatusMessage.textContent = reason.message;
  showAuth("status");
  return true;
}

function renderAccountPanel(account) {
  els.accountPlan.textContent = account.plan_name || "—";
  els.accountExpires.textContent = formatDate(account.expires_at);
  els.accountTraffic.textContent =
    account.traffic_limit_gb > 0
      ? `${account.traffic_used_gb} из ${account.traffic_limit_gb} ГБ`
      : `${account.traffic_used_gb} ГБ · без лимита`;
}

function enterApp(account) {
  if (showAccountStatus(account)) return;
  currentAccount = account;
  renderAccountPanel(account);
  els.authScreen.hidden = true;
  els.app.hidden = false;
  initApp();
}

function showAuth(form) {
  els.codeForm.hidden = form !== "code";
  els.accountStatusView.hidden = form !== "status";
}

els.accountStatusBack.addEventListener("click", () => {
  els.accessCode.value = "";
  showAuth("code");
});

// Разбиение на группы по мере ввода: код читают с бумаги по пять знаков, и
// набирать его сплошной строкой — гарантированная потеря места.
els.accessCode.addEventListener("input", () => {
  const el = els.accessCode;
  const atEnd = el.selectionStart === el.value.length;
  const raw = el.value.toUpperCase().replace(/[^0-9A-Z]/g, "").slice(0, 20);
  const grouped = raw.match(/.{1,5}/g)?.join("-") ?? "";
  el.value = grouped;
  if (atEnd) el.setSelectionRange(grouped.length, grouped.length);
});

els.codeForm.addEventListener("submit", async (e) => {
  e.preventDefault();
  const code = els.accessCode.value.trim();
  if (!code) {
    showToast("Введите код доступа.", "error");
    return;
  }
  els.codeSubmit.disabled = true;
  try {
    const account = await invoke("enroll_code", { code });
    enterApp(account);
  } catch (err) {
    showToast(String(err.message ?? err) || "Не удалось войти.", "error");
  } finally {
    els.codeSubmit.disabled = false;
  }
});

els.logoutBtn.addEventListener("click", async () => {
  closeSettings();
  currentAccount = null;
  appStarted = false;
  try {
    // Отвязывает устройство на сервере и стирает ключи локально: выйти,
    // оставив за собой занятое место в лимите устройств, — не выход.
    await invoke("sign_out");
  } catch (err) {
    showToast(String(err.message ?? err) || "Выход прошёл не полностью.", "error");
  }
  status = { status: "disconnected", server: null };
  els.app.hidden = true;
  els.accessCode.value = "";
  showAuth("code");
  els.authScreen.hidden = false;
});

async function initAuth() {
  // Ключ устройства уже лежит рядом с программой — значит, спрашивать код
  // не за чем: вход обходится одной подписью.
  try {
    enterApp(await invoke("sign_in"));
  } catch {
    showAuth("code");
  }
}

initAuth();
