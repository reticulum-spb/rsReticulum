"use strict";

const REQUEST_TIMEOUT_MS = 8000;
const views = new Set(["dashboard", "interfaces", "paths"]);

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

async function checkHealth() {
  setRuntimeState("loading", "Connecting to rnsd-rs", "Checking the embedded REST API.");
  try {
    const health = await apiFetch("/health");
    if (!health?.ok) throw new ApiError("The health endpoint returned an unexpected response");
    setRuntimeState(
      "online",
      "Shared instance is available",
      "The Web configurator is connected to the rnsd-rs control plane.",
    );
    clearError();
  } catch (error) {
    setRuntimeState(
      "unavailable",
      "Shared instance is unavailable",
      "The Web UI could not reach the embedded REST API.",
    );
    showError(error);
  }
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
  setBusy(button, true);
  clearError();
  try {
    await checkHealth();
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

showView(currentView());
checkHealth();
