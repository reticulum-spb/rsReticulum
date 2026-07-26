"use strict";

const REQUEST_TIMEOUT_MS = 8000;
const DASHBOARD_REFRESH_MS = 5000;
const views = new Set(["dashboard", "interfaces", "paths"]);
let dashboardRequest = null;
let interfaceRequest = null;
const interfaceState = {
  items: [],
  expanded: new Set(),
  filter: "",
  showAll: false,
};

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

function formatRate(value) {
  const rate = Number(value);
  if (!Number.isFinite(rate) || rate < 0) return "—";
  return `${formatBytes(rate)}/s`;
}

function formatNumber(value) {
  if (value === null || value === undefined) return "—";
  const number = Number(value);
  return Number.isFinite(number) ? number.toLocaleString() : "—";
}

function formatFrequency(value) {
  const frequency = Number(value);
  if (!Number.isFinite(frequency) || frequency < 0) return "—";
  return `${frequency.toLocaleString(undefined, { maximumFractionDigits: 2 })}/s`;
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

function interfaceType(item) {
  return item.config?.type || "Runtime only";
}

function interfaceEndpoint(item) {
  const config = item.config;
  if (!config) return "Not stored in config";
  if (config.type === "TCPClientInterface") {
    return `${config.target_host || "—"}:${config.target_port ?? "—"}`;
  }
  if (config.type === "TCPServerInterface") {
    return `${config.listen_ip || "—"}:${config.listen_port ?? "—"}`;
  }
  return "—";
}

function detailItem(label, value, className = "") {
  const wrapper = document.createElement("div");
  const term = document.createElement("dt");
  const description = document.createElement("dd");
  term.textContent = label;
  description.textContent = value;
  if (className) description.className = className;
  wrapper.append(term, description);
  return wrapper;
}

function interfaceDetails(item) {
  const details = document.createElement("dl");
  details.className = "interface-details";
  details.append(
    detailItem("Runtime ID", formatNumber(item.id)),
    detailItem("Configuration", item.config ? "Configurable" : "Runtime-managed",
      item.config ? "" : "runtime-only"),
    detailItem("Type", interfaceType(item)),
    detailItem("Endpoint", interfaceEndpoint(item)),
    detailItem("Role", item.role || "—"),
    detailItem("Bitrate", item.bitrate ? `${formatNumber(item.bitrate)} bit/s` : "—"),
    detailItem("Clients", formatNumber(item.clients)),
    detailItem("TX drops", formatNumber(item.tx_drops)),
    detailItem("IFAC size", item.ifac_size ? `${formatNumber(item.ifac_size)} B` : "None"),
    detailItem("Announce queue", formatNumber(item.announce_queue)),
    detailItem("Held announces", formatNumber(item.held_announces)),
    detailItem("Incoming announces", formatFrequency(item.incoming_announce_frequency)),
    detailItem("Outgoing announces", formatFrequency(item.outgoing_announce_frequency)),
  );
  return details;
}

function stackedCell(primary, secondary) {
  const cell = document.createElement("td");
  const stack = document.createElement("span");
  const main = document.createElement("span");
  const small = document.createElement("small");
  stack.className = "cell-stack";
  main.textContent = primary;
  small.textContent = secondary;
  stack.append(main, small);
  cell.append(stack);
  return cell;
}

function interfaceRows(items) {
  const fragment = document.createDocumentFragment();

  for (const item of items) {
    const row = document.createElement("tr");
    const nameCell = document.createElement("td");
    const name = document.createElement("span");
    const nameText = document.createElement("strong");
    const type = document.createElement("small");
    const id = document.createElement("code");
    name.className = "interface-name";
    nameText.textContent = item.name || "Unnamed interface";
    type.textContent = interfaceType(item);
    id.textContent = `#${item.id}`;
    name.append(nameText, type, id);
    nameCell.append(name);

    const statusCell = document.createElement("td");
    const status = document.createElement("span");
    status.className = `status-chip${item.online ? " online" : ""}`;
    status.textContent = item.online ? "Online" : "Offline";
    statusCell.append(status);

    const detailCell = document.createElement("td");
    const detailButton = document.createElement("button");
    const expanded = interfaceState.expanded.has(item.id);
    detailButton.type = "button";
    detailButton.className = "details-button";
    detailButton.textContent = expanded ? "−" : "+";
    detailButton.setAttribute("aria-expanded", String(expanded));
    detailButton.setAttribute("aria-label", `${expanded ? "Hide" : "Show"} details for ${item.name}`);
    detailButton.addEventListener("click", () => {
      if (expanded) interfaceState.expanded.delete(item.id);
      else interfaceState.expanded.add(item.id);
      renderInterfaces();
    });
    detailCell.append(detailButton);

    row.append(
      nameCell,
      statusCell,
      stackedCell(item.mode || "—", item.role || "—"),
      stackedCell(`↓ ${formatBytes(item.rx_bytes)}`, `↑ ${formatBytes(item.tx_bytes)}`),
      stackedCell(`↓ ${formatRate(item.rx_rate)}`, `↑ ${formatRate(item.tx_rate)}`),
      detailCell,
    );
    fragment.append(row);

    if (expanded) {
      const detailRow = document.createElement("tr");
      const container = document.createElement("td");
      detailRow.className = "interface-detail-row";
      container.colSpan = 6;
      container.append(interfaceDetails(item));
      detailRow.append(container);
      fragment.append(detailRow);
    }
  }

  return fragment;
}

function renderInterfaces() {
  const query = interfaceState.filter.trim().toLocaleLowerCase();
  const items = interfaceState.items.filter((item) =>
    !query || (item.name || "").toLocaleLowerCase().includes(query)
      || interfaceType(item).toLocaleLowerCase().includes(query));
  const rows = document.querySelector("#interface-rows");
  const empty = document.querySelector("#interfaces-empty");
  const heading = document.querySelector("#interfaces-empty-heading");
  const detail = document.querySelector("#interfaces-empty-detail");

  rows.replaceChildren(interfaceRows(items));
  empty.hidden = items.length > 0;
  if (!items.length) {
    heading.textContent = query ? "No matching interfaces" : "No interfaces";
    detail.textContent = query
      ? "Try a different name or interface type."
      : "No runtime interfaces were returned by rnsd-rs.";
  }

  const visible = items.length.toLocaleString();
  const totalCount = interfaceState.items.length;
  const total = totalCount.toLocaleString();
  document.querySelector("#interface-count").textContent =
    query ? `${visible} of ${total}` : `${total} interface${totalCount === 1 ? "" : "s"}`;
}

function setInterfacesLoading() {
  document.querySelector("#interface-rows").replaceChildren();
  document.querySelector("#interface-search").disabled = true;
  document.querySelector("#interfaces-empty").hidden = false;
  document.querySelector("#interfaces-empty-heading").textContent = "Loading interfaces…";
  document.querySelector("#interfaces-empty-detail").textContent = "Waiting for runtime data.";
  document.querySelector("#interface-count").textContent = "Loading…";
}

function refreshInterfaces() {
  if (interfaceRequest) return interfaceRequest;

  const refreshButton = document.querySelector("#view-interfaces [data-refresh]");
  const showAll = document.querySelector("#show-all-interfaces");
  setBusy(refreshButton, true);
  showAll.disabled = true;
  setInterfacesLoading();
  const query = interfaceState.showAll ? "?all=true" : "";
  interfaceRequest = apiFetch(`/api/v1/interfaces${query}`)
    .then((body) => {
      interfaceState.items = Array.isArray(body.interfaces) ? body.interfaces : [];
      document.querySelector("#interface-search").disabled = false;
      renderInterfaces();
      document.querySelector("#daemon-pill").dataset.state = "online";
      document.querySelector("#daemon-label").textContent = "rnsd-rs online";
      clearError();
    })
    .catch((error) => {
      interfaceState.items = [];
      document.querySelector("#interface-search").disabled = true;
      document.querySelector("#interfaces-empty-heading").textContent = "Interfaces unavailable";
      document.querySelector("#interfaces-empty-detail").textContent =
        "The interface list could not be loaded from rnsd-rs.";
      document.querySelector("#interface-count").textContent = "Unavailable";
      document.querySelector("#daemon-pill").dataset.state = "unavailable";
      document.querySelector("#daemon-label").textContent = "Unavailable";
      showError(error);
    })
    .finally(() => {
      setBusy(refreshButton, false);
      showAll.disabled = false;
      interfaceRequest = null;
    });

  return interfaceRequest;
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
  if (selected === "interfaces") refreshInterfaces();
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
  if (currentView() === "interfaces") {
    await refreshInterfaces();
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
document.querySelector("#interface-search").addEventListener("input", (event) => {
  interfaceState.filter = event.target.value;
  renderInterfaces();
});
document.querySelector("#show-all-interfaces").addEventListener("change", (event) => {
  interfaceState.showAll = event.target.checked;
  interfaceState.expanded.clear();
  refreshInterfaces();
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
