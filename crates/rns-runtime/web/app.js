"use strict";

const REQUEST_TIMEOUT_MS = 8000;
const DASHBOARD_REFRESH_MS = 5000;
const views = new Set(["dashboard", "interfaces", "paths"]);
let dashboardRequest = null;

class ApiError extends Error {
  constructor(message, status = 0, body = null) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.body = body;
  }
}

async function apiFetch(path, options = {}) {
  const { timeout = REQUEST_TIMEOUT_MS, ...fetchOptions } = options;
  const controller = new AbortController();
  const timeoutId = window.setTimeout(() => controller.abort(), timeout);

  try {
    const response = await fetch(path, {
      ...fetchOptions,
      headers: {
        accept: "application/json",
        ...fetchOptions.headers,
      },
      signal: controller.signal,
    });

    const contentType = response.headers.get("content-type") || "";
    const body = contentType.includes("application/json")
      ? await response.json()
      : await response.text();

    if (!response.ok) {
      const message = body?.error || body?.message || `Request failed with HTTP ${response.status}`;
      throw new ApiError(message, response.status, body);
    }

    return body;
  } catch (error) {
    if (error.name === "AbortError") {
      throw new ApiError(`Request timed out after ${timeout / 1000} seconds`);
    }
    if (error instanceof ApiError) throw error;
    throw new ApiError(error.message || "Unable to reach rnsd-rs");
  } finally {
    window.clearTimeout(timeoutId);
  }
}

function setBusy(button, busy) {
  if (!button) return;
  if (busy) {
    button.disabled = true;
    button.setAttribute("aria-busy", "true");
  } else {
    button.disabled = false;
    button.removeAttribute("aria-busy");
  }
}

function showError(error) {
  const notice = document.querySelector("#global-error");
  document.querySelector("#global-error-text").textContent = error.message || String(error);
  notice.hidden = false;
}

function clearError() {
  document.querySelector("#global-error").hidden = true;
}

function setRuntimeState(state, heading, detail) {
  const pill = document.querySelector("#daemon-pill");
  const badge = document.querySelector("#runtime-badge");
  pill.dataset.state = state;
  badge.dataset.state = state;
  document.querySelector("#daemon-label").textContent =
    state === "online" ? "rnsd-rs online" : state === "loading" ? "Connecting…" : "Unavailable";
  badge.textContent =
    state === "online" ? "Online" : state === "loading" ? "Checking" : "Unavailable";
  document.querySelector("#runtime-heading").textContent = heading;
  document.querySelector("#runtime-detail").textContent = detail;
}

function formatBytes(value) {
  const bytes = Number(value);
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1000) return `${bytes.toLocaleString()} B`;

  const units = ["kB", "MB", "GB", "TB", "PB"];
  let scaled = bytes;
  let unit = "B";
  for (const candidate of units) {
    scaled /= 1000;
    unit = candidate;
    if (scaled < 1000) break;
  }

  const digits = scaled >= 100 ? 0 : scaled >= 10 ? 1 : 2;
  return `${scaled.toLocaleString(undefined, { maximumFractionDigits: digits })} ${unit}`;
}

function resetDashboardMetrics() {
  document.querySelector("#metric-interfaces").textContent = "—";
  document.querySelector("#metric-online").textContent = "Status unavailable";
  document.querySelector("#metric-rx").textContent = "—";
  document.querySelector("#metric-tx").textContent = "—";
  document.querySelector("#metric-links").textContent = "—";
}

function renderDashboard(status, links) {
  const total = Number(status.interfaces_total) || 0;
  const online = Number(status.interfaces_online) || 0;
  document.querySelector("#metric-interfaces").textContent = total.toLocaleString();
  document.querySelector("#metric-online").textContent = `${online.toLocaleString()} online`;
  document.querySelector("#metric-rx").textContent = formatBytes(status.rx_bytes_total);
  document.querySelector("#metric-tx").textContent = formatBytes(status.tx_bytes_total);
  document.querySelector("#metric-links").textContent =
    (Number(links.link_count) || 0).toLocaleString();
}

function setDashboardBusy(busy) {
  document.querySelectorAll("#view-dashboard [data-refresh]").forEach((button) => {
    setBusy(button, busy);
  });
}

function refreshDashboard() {
  if (dashboardRequest) return dashboardRequest;

  setDashboardBusy(true);
  setRuntimeState("loading", "Connecting to rnsd-rs", "Checking the embedded REST API.");
  dashboardRequest = Promise.all([
    apiFetch("/health"),
    apiFetch("/api/v1/status"),
    apiFetch("/api/v1/links"),
  ])
    .then(([health, status, links]) => {
      if (!health?.ok) throw new ApiError("The health endpoint returned an unexpected response");
      renderDashboard(status, links);
      setRuntimeState(
        "online",
        "Shared instance is available",
        "Runtime metrics are updating automatically every 5 seconds.",
      );
      clearError();
    })
    .catch((error) => {
      resetDashboardMetrics();
      setRuntimeState(
        "unavailable",
        "Shared instance is unavailable",
        "The Web UI could not load runtime status from the embedded REST API.",
      );
      showError(error);
    })
    .finally(() => {
      setDashboardBusy(false);
      dashboardRequest = null;
    });

  return dashboardRequest;
}

function currentView() {
  const requested = window.location.hash.replace(/^#/, "");
  return views.has(requested) ? requested : "dashboard";
}

function showView(name, { focus = false } = {}) {
  const selected = views.has(name) ? name : "dashboard";

  document.querySelectorAll("[data-view-panel]").forEach((panel) => {
    const active = panel.dataset.viewPanel === selected;
    panel.hidden = !active;
    panel.classList.toggle("active", active);
  });

  document.querySelectorAll("[data-view]").forEach((button) => {
    const active = button.dataset.view === selected;
    button.classList.toggle("active", active);
    if (active) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
  });

  document.title = `${selected[0].toUpperCase()}${selected.slice(1)} · rsReticulum`;
  closeNavigation();
  if (focus) document.querySelector("#main-content").focus();
  if (selected === "dashboard") refreshDashboard();
}

function openNavigation() {
  document.querySelector("#sidebar").classList.add("open");
  document.querySelector("#mobile-menu").setAttribute("aria-expanded", "true");
}

function closeNavigation() {
  document.querySelector("#sidebar").classList.remove("open");
  document.querySelector("#mobile-menu").setAttribute("aria-expanded", "false");
}

async function refresh(button) {
  if (currentView() === "dashboard") {
    await refreshDashboard();
    return;
  }

  setBusy(button, true);
  clearError();
  try {
    const health = await apiFetch("/health");
    if (!health?.ok) throw new ApiError("The health endpoint returned an unexpected response");
    setRuntimeState(
      "online",
      "Shared instance is available",
      "The Web configurator is connected to the rnsd-rs control plane.",
    );
  } catch (error) {
    setRuntimeState(
      "unavailable",
      "Shared instance is unavailable",
      "The Web UI could not reach the embedded REST API.",
    );
    showError(error);
  } finally {
    setBusy(button, false);
  }
}

document.querySelectorAll("[data-view]").forEach((button) => {
  button.addEventListener("click", () => {
    const target = button.dataset.view;
    if (window.location.hash === `#${target}`) showView(target, { focus: true });
    else window.location.hash = target;
  });
});

document.querySelector("#mobile-menu").addEventListener("click", () => {
  if (document.querySelector("#sidebar").classList.contains("open")) closeNavigation();
  else openNavigation();
});

document.querySelector("#dismiss-error").addEventListener("click", clearError);
document.querySelector("#add-interface").addEventListener("click", () => {
  document.querySelector("#interface-dialog").showModal();
});

document.querySelectorAll("[data-refresh]").forEach((button) => {
  button.addEventListener("click", () => refresh(button));
});

window.addEventListener("hashchange", () => showView(currentView(), { focus: true }));
window.addEventListener("keydown", (event) => {
  if (event.key === "Escape") closeNavigation();
});
window.addEventListener("resize", () => {
  if (window.innerWidth > 720) closeNavigation();
});
window.setInterval(() => {
  if (currentView() === "dashboard" && !document.hidden) refreshDashboard();
}, DASHBOARD_REFRESH_MS);

showView(currentView());
