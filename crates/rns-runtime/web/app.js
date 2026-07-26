"use strict";

const REQUEST_TIMEOUT_MS = 8000;
const DASHBOARD_REFRESH_MS = 5000;
const INTERFACE_REFRESH_MS = 1000;
const INTERFACE_ERROR_REFRESH_MS = 5000;
const PATH_REFRESH_MS = 5000;
const PATH_ERROR_REFRESH_MS = 15000;
const views = new Set(["dashboard", "interfaces", "paths"]);
let dashboardRequest = null;
let interfaceRequest = null;
let interfaceNextPollAt = 0;
let pathRequest = null;
let pathNextPollAt = 0;
const interfaceState = {
  items: [],
  expanded: new Set(),
  filter: "",
  showAll: false,
  loaded: false,
};
const pathState = {
  items: [],
  filter: "",
  maxHops: "",
  loaded: false,
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
    apiFetch("/health", { cache: "no-store" }),
    apiFetch("/api/v1/status", { cache: "no-store" }),
    apiFetch("/api/v1/links", { cache: "no-store" }),
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

function actionButton(label, className, handler) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = className;
  button.textContent = label;
  button.addEventListener("click", handler);
  return button;
}

function interfaceDetails(item) {
  const container = document.createElement("div");
  container.className = "interface-details-container";
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
  container.append(details);
  if (item.config) {
    const actions = document.createElement("div");
    actions.className = "interface-actions";
    actions.append(
      actionButton("Edit configuration", "", () => openInterfaceDialog(item)),
      actionButton("Delete", "danger", () => openDeleteDialog(item)),
    );
    container.append(actions);
  }
  return container;
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

function refreshInterfaces({ background = false } = {}) {
  if (interfaceRequest) return interfaceRequest;

  const refreshButton = document.querySelector("#view-interfaces [data-refresh]");
  const showAll = document.querySelector("#show-all-interfaces");
  if (!background) {
    setBusy(refreshButton, true);
    showAll.disabled = true;
  }
  if (!interfaceState.loaded) setInterfacesLoading();
  const query = interfaceState.showAll ? "?all=true" : "";
  interfaceRequest = apiFetch(`/api/v1/interfaces${query}`, { cache: "no-store" })
    .then((body) => {
      interfaceState.items = Array.isArray(body.interfaces) ? body.interfaces : [];
      interfaceState.loaded = true;
      interfaceNextPollAt = Date.now() + INTERFACE_REFRESH_MS;
      document.querySelector("#interface-search").disabled = false;
      renderInterfaces();
      document.querySelector("#daemon-pill").dataset.state = "online";
      document.querySelector("#daemon-label").textContent = "rnsd-rs online";
      clearError();
    })
    .catch((error) => {
      interfaceNextPollAt = Date.now() + INTERFACE_ERROR_REFRESH_MS;
      if (!interfaceState.loaded) {
        interfaceState.items = [];
        document.querySelector("#interface-search").disabled = true;
        document.querySelector("#interfaces-empty-heading").textContent = "Interfaces unavailable";
        document.querySelector("#interfaces-empty-detail").textContent =
          "The interface list could not be loaded from rnsd-rs.";
        document.querySelector("#interface-count").textContent = "Unavailable";
      } else {
        document.querySelector("#interface-count").textContent = "Update failed · retrying";
      }
      document.querySelector("#daemon-pill").dataset.state = "unavailable";
      document.querySelector("#daemon-label").textContent = "Unavailable";
      showError(error);
    })
    .finally(() => {
      if (!background) {
        setBusy(refreshButton, false);
        showAll.disabled = false;
      }
      interfaceRequest = null;
    });

  return interfaceRequest;
}

function formatTimestamp(value) {
  const seconds = Number(value);
  if (!Number.isFinite(seconds)) return "—";
  return new Date(seconds * 1000).toLocaleString();
}

function formatDuration(seconds) {
  const absolute = Math.max(0, Math.round(Math.abs(seconds)));
  if (absolute < 60) return `${absolute}s`;
  if (absolute < 3600) return `${Math.floor(absolute / 60)}m`;
  if (absolute < 86400) {
    const hours = Math.floor(absolute / 3600);
    const minutes = Math.floor((absolute % 3600) / 60);
    return minutes ? `${hours}h ${minutes}m` : `${hours}h`;
  }
  const days = Math.floor(absolute / 86400);
  const hours = Math.floor((absolute % 86400) / 3600);
  return hours ? `${days}d ${hours}h` : `${days}d`;
}

function relativeTimestamp(value, future = false) {
  const seconds = Number(value);
  if (!Number.isFinite(seconds)) return "—";
  const delta = seconds - Date.now() / 1000;
  if (future) return delta <= 0 ? "Expired" : `in ${formatDuration(delta)}`;
  return delta >= 0 ? "just now" : `${formatDuration(delta)} ago`;
}

function pathTextCell(value, className = "") {
  const cell = document.createElement("td");
  const text = document.createElement(className === "hash-cell" ? "code" : "span");
  text.className = className;
  text.textContent = value || "—";
  cell.append(text);
  return cell;
}

function pathRows(items) {
  const fragment = document.createDocumentFragment();
  for (const path of items) {
    const row = document.createElement("tr");
    row.append(
      pathTextCell(path.hash, "hash-cell"),
      pathTextCell(formatNumber(path.hops)),
      pathTextCell(path.interface),
      pathTextCell(path.via, "hash-cell"),
      stackedCell(formatTimestamp(path.timestamp), relativeTimestamp(path.timestamp)),
      stackedCell(formatTimestamp(path.expires), relativeTimestamp(path.expires, true)),
    );
    fragment.append(row);
  }
  return fragment;
}

function renderPaths() {
  const query = pathState.filter.trim().toLocaleLowerCase();
  const items = pathState.items.filter((path) => {
    if (!query) return true;
    return [path.hash, path.via, path.interface]
      .some((value) => String(value || "").toLocaleLowerCase().includes(query));
  });
  const rows = document.querySelector("#path-rows");
  const empty = document.querySelector("#paths-empty");
  rows.replaceChildren(pathRows(items));
  empty.hidden = items.length > 0;

  const heading = empty.querySelector("strong");
  const detail = empty.querySelector("span:last-child");
  if (!items.length) {
    heading.textContent = query ? "No matching paths" : "No paths";
    detail.textContent = query
      ? "Search matches destination, next-hop address, and interface name."
      : "No routes were returned by rnsd-rs.";
  }

  const total = pathState.items.length;
  document.querySelector("#path-count").textContent = query
    ? `${items.length.toLocaleString()} of ${total.toLocaleString()}`
    : `${total.toLocaleString()} path${total === 1 ? "" : "s"}`;
}

function setPathsLoading() {
  document.querySelector("#path-rows").replaceChildren();
  document.querySelector("#path-search").disabled = true;
  document.querySelector("#paths-empty").hidden = false;
  document.querySelector("#paths-empty strong").textContent = "Loading paths…";
  document.querySelector("#paths-empty span:last-child").textContent =
    "Waiting for transport route data.";
  document.querySelector("#path-count").textContent = "Loading…";
}

function refreshPaths({ background = false } = {}) {
  if (pathRequest) return pathRequest;

  const refreshButton = document.querySelector("#view-paths [data-refresh]");
  const maxHops = document.querySelector("#max-hops");
  if (!background) {
    setBusy(refreshButton, true);
    maxHops.disabled = true;
  }
  if (!pathState.loaded) setPathsLoading();
  const query = pathState.maxHops ? `?max_hops=${encodeURIComponent(pathState.maxHops)}` : "";
  pathRequest = apiFetch(`/api/v1/paths${query}`, { cache: "no-store" })
    .then((body) => {
      pathState.items = Array.isArray(body.paths) ? body.paths : [];
      pathState.loaded = true;
      pathNextPollAt = Date.now() + PATH_REFRESH_MS;
      document.querySelector("#path-search").disabled = false;
      renderPaths();
      document.querySelector("#daemon-pill").dataset.state = "online";
      document.querySelector("#daemon-label").textContent = "rnsd-rs online";
      clearError();
    })
    .catch((error) => {
      pathNextPollAt = Date.now() + PATH_ERROR_REFRESH_MS;
      if (!pathState.loaded) {
        document.querySelector("#path-search").disabled = true;
        document.querySelector("#paths-empty strong").textContent = "Paths unavailable";
        document.querySelector("#paths-empty span:last-child").textContent =
          "The route table could not be loaded from rnsd-rs.";
        document.querySelector("#path-count").textContent = "Unavailable";
      } else {
        document.querySelector("#path-count").textContent = "Update failed · retrying";
      }
      document.querySelector("#daemon-pill").dataset.state = "unavailable";
      document.querySelector("#daemon-label").textContent = "Unavailable";
      showError(error);
    })
    .finally(() => {
      if (!background) {
        setBusy(refreshButton, false);
        maxHops.disabled = false;
      }
      pathRequest = null;
    });

  return pathRequest;
}

function setInterfaceType(type) {
  const client = document.querySelector("#tcp-client-fields");
  const server = document.querySelector("#tcp-server-fields");
  const isClient = type === "TCPClientInterface";
  client.hidden = !isClient;
  client.disabled = !isClient;
  server.hidden = isClient;
  server.disabled = isClient;
}

function setField(selector, value, fallback = "") {
  document.querySelector(selector).value = value ?? fallback;
}

function openInterfaceDialog(item = null) {
  const dialog = document.querySelector("#interface-dialog");
  const form = document.querySelector("#interface-form");
  const config = item?.config || {};
  form.reset();
  setField("#interface-id", item?.id);
  setField("#interface-name", item?.name);
  setField("#interface-type", config.type, "TCPClientInterface");
  setField("#interface-mode", config.interface_mode, "Full");
  setField("#target-host", config.target_host);
  setField("#target-port", config.target_port);
  setField("#connect-timeout", config.connect_timeout);
  setField("#max-reconnect-tries", config.max_reconnect_tries);
  setField("#fixed-mtu", config.fixed_mtu);
  setField("#listen-ip", config.listen_ip, "0.0.0.0");
  setField("#listen-port", config.listen_port);
  setField("#interface-device", config.device);
  document.querySelector("#prefer-ipv6").checked = Boolean(config.prefer_ipv6);
  document.querySelector("#kiss-framing").checked = Boolean(config.kiss_framing);
  document.querySelector("#interface-dialog-title").textContent =
    item ? "Edit interface" : "Add interface";
  document.querySelector("#save-interface").textContent =
    item ? "Save and restart" : "Add interface";
  document.querySelector("#interface-form-error").textContent = "";
  document.querySelector("#interface-form-status").textContent = "";
  setInterfaceType(document.querySelector("#interface-type").value);
  dialog.showModal();
  document.querySelector("#interface-name").focus();
}

function closeInterfaceDialog() {
  document.querySelector("#interface-dialog").close();
}

function optionalInteger(selector) {
  const value = document.querySelector(selector).value.trim();
  return value === "" ? undefined : Number(value);
}

function interfacePayload() {
  const type = document.querySelector("#interface-type").value;
  const payload = {
    name: document.querySelector("#interface-name").value.trim(),
    type,
    interface_mode: document.querySelector("#interface-mode").value,
    kiss_framing: document.querySelector("#kiss-framing").checked,
  };

  if (type === "TCPClientInterface") {
    payload.target_host = document.querySelector("#target-host").value.trim();
    payload.target_port = Number(document.querySelector("#target-port").value);
    const connectTimeout = optionalInteger("#connect-timeout");
    const reconnectTries = optionalInteger("#max-reconnect-tries");
    const fixedMtu = optionalInteger("#fixed-mtu");
    if (connectTimeout !== undefined) payload.connect_timeout = connectTimeout;
    if (reconnectTries !== undefined) payload.max_reconnect_tries = reconnectTries;
    if (fixedMtu !== undefined) payload.fixed_mtu = fixedMtu;
  } else {
    payload.listen_ip = document.querySelector("#listen-ip").value.trim();
    payload.listen_port = Number(document.querySelector("#listen-port").value);
    payload.prefer_ipv6 = document.querySelector("#prefer-ipv6").checked;
    const device = document.querySelector("#interface-device").value.trim();
    if (device) payload.device = device;
  }

  return payload;
}

async function saveInterface(event) {
  event.preventDefault();
  const form = event.currentTarget;
  if (!form.reportValidity()) return;

  const id = document.querySelector("#interface-id").value;
  const button = document.querySelector("#save-interface");
  const errorNode = document.querySelector("#interface-form-error");
  const statusNode = document.querySelector("#interface-form-status");
  const editing = id !== "";
  errorNode.textContent = "";
  statusNode.textContent = editing ? "Restarting interface…" : "Starting interface…";
  setBusy(button, true);

  try {
    await apiFetch(editing ? `/api/v1/interfaces/${id}` : "/api/v1/interfaces", {
      method: editing ? "PUT" : "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(interfacePayload()),
    });
    closeInterfaceDialog();
    interfaceState.expanded.clear();
    await Promise.all([refreshInterfaces(), refreshDashboard()]);
  } catch (error) {
    errorNode.textContent =
      error.status === 409 ? `Name conflict: ${error.message}`
        : error.status === 422 ? `Invalid configuration: ${error.message}`
          : error.message;
    statusNode.textContent = "";
  } finally {
    setBusy(button, false);
  }
}

function openDeleteDialog(item) {
  const dialog = document.querySelector("#delete-interface-dialog");
  dialog.dataset.interfaceId = item.id;
  document.querySelector("#delete-interface-name").textContent = item.name;
  document.querySelector("#delete-interface-error").textContent = "";
  dialog.showModal();
  document.querySelector("#confirm-delete-interface").focus();
}

function closeDeleteDialog() {
  const dialog = document.querySelector("#delete-interface-dialog");
  dialog.close();
  delete dialog.dataset.interfaceId;
}

async function deleteInterface(event) {
  event.preventDefault();
  const dialog = document.querySelector("#delete-interface-dialog");
  const id = dialog.dataset.interfaceId;
  if (!id) return;

  const button = document.querySelector("#confirm-delete-interface");
  const errorNode = document.querySelector("#delete-interface-error");
  errorNode.textContent = "";
  setBusy(button, true);
  try {
    await apiFetch(`/api/v1/interfaces/${id}`, { method: "DELETE" });
    closeDeleteDialog();
    interfaceState.expanded.delete(Number(id));
    await Promise.all([refreshInterfaces(), refreshDashboard()]);
  } catch (error) {
    errorNode.textContent = error.message;
  } finally {
    setBusy(button, false);
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
  if (selected === "dashboard") refreshDashboard();
  if (selected === "interfaces") refreshInterfaces();
  if (selected === "paths") refreshPaths();
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
  if (currentView() === "paths") {
    await refreshPaths();
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
document.querySelector("#add-interface").addEventListener("click", () => openInterfaceDialog());
document.querySelector("#interface-type").addEventListener("change", (event) => {
  setInterfaceType(event.target.value);
});
document.querySelector("#interface-form").addEventListener("submit", saveInterface);
document.querySelector("#delete-interface-form").addEventListener("submit", deleteInterface);
document.querySelectorAll("[data-close-interface]").forEach((button) => {
  button.addEventListener("click", closeInterfaceDialog);
});
document.querySelectorAll("[data-close-delete]").forEach((button) => {
  button.addEventListener("click", closeDeleteDialog);
});
document.querySelector("#interface-search").addEventListener("input", (event) => {
  interfaceState.filter = event.target.value;
  renderInterfaces();
});
document.querySelector("#show-all-interfaces").addEventListener("change", (event) => {
  interfaceState.showAll = event.target.checked;
  interfaceState.expanded.clear();
  interfaceState.loaded = false;
  if (interfaceRequest) interfaceRequest.finally(() => refreshInterfaces());
  else refreshInterfaces();
});
document.querySelector("#path-search").addEventListener("input", (event) => {
  pathState.filter = event.target.value;
  renderPaths();
});
document.querySelector("#max-hops").addEventListener("change", (event) => {
  if (event.target.value && !event.target.checkValidity()) {
    event.target.reportValidity();
    return;
  }
  pathState.maxHops = event.target.value;
  pathState.loaded = false;
  if (pathRequest) pathRequest.finally(() => refreshPaths());
  else refreshPaths();
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
window.setInterval(() => {
  if (
    currentView() === "interfaces"
    && !document.hidden
    && Date.now() >= interfaceNextPollAt
  ) {
    refreshInterfaces({ background: true });
  }
}, INTERFACE_REFRESH_MS);
window.setInterval(() => {
  if (
    currentView() === "paths"
    && !document.hidden
    && Date.now() >= pathNextPollAt
  ) {
    refreshPaths({ background: true });
  }
}, PATH_REFRESH_MS);

showView(currentView());
