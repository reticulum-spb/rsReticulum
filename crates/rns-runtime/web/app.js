"use strict";

const REQUEST_TIMEOUT_MS = 8000;
const DASHBOARD_REFRESH_MS = 5000;
const INTERFACE_REFRESH_MS = 1000;
const INTERFACE_ERROR_REFRESH_MS = 5000;
const PATH_REFRESH_MS = 5000;
const PATH_ERROR_REFRESH_MS = 15000;
const views = new Set(["dashboard", "interfaces", "paths", "settings", "logs"]);
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
const pluginState = {
  items: [],
  loaded: false,
  request: null,
};
const pathState = {
  items: [],
  filter: "",
  maxHops: "",
  loaded: false,
};
const logState = {
  entries: [],
  ids: new Set(),
  source: null,
  paused: false,
  filter: "",
  level: "info",
  historyLoaded: false,
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
      if (response.status === 401 && path !== "/api/v1/auth/login") openLoginDialog();
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

function openLoginDialog() {
  const dialog = document.querySelector("#login-dialog");
  if (!dialog.open) dialog.showModal();
}

async function submitLogin(event) {
  event.preventDefault();
  const button = document.querySelector("#login-submit");
  setBusy(button, true);
  document.querySelector("#login-error").textContent = "";
  try {
    await apiFetch("/api/v1/auth/login", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        user: document.querySelector("#login-user").value,
        password: document.querySelector("#login-password").value,
      }),
    });
    document.querySelector("#login-password").value = "";
    document.querySelector("#login-dialog").close();
    logState.entries = [];
    logState.ids.clear();
    logState.historyLoaded = false;
    await refresh();
  } catch (error) {
    document.querySelector("#login-error").textContent = error.message;
  } finally {
    setBusy(button, false);
  }
}

async function logout() {
  stopLogs();
  logState.entries = [];
  logState.ids.clear();
  logState.historyLoaded = false;
  await apiFetch("/api/v1/auth/logout", { method: "POST" });
  openLoginDialog();
  document.querySelector("#login-user").focus();
}

function renderLogs() {
  if (logState.paused) return;
  const levels = { trace: 0, debug: 1, info: 2, warn: 3, error: 4 };
  const minimum = levels[logState.level] ?? 2;
  const query = logState.filter.toLocaleLowerCase();
  const output = document.querySelector("#log-output");
  const fragment = document.createDocumentFragment();
  let count = 0;
  for (const entry of logState.entries) {
    if ((levels[entry.level] ?? 2) < minimum) continue;
    if (query && !`${entry.target} ${entry.message}`.toLocaleLowerCase().includes(query)) continue;
    const line = document.createElement("div");
    line.className = "log-line";
    line.dataset.level = entry.level;
    const timestamp = new Date(Number(entry.timestamp_ms)).toLocaleTimeString();
    for (const [className, value] of [
      ["log-time", timestamp],
      ["log-level", entry.level],
      ["log-target", entry.target],
      ["log-message", entry.message],
    ]) {
      const span = document.createElement("span");
      span.className = className;
      span.textContent = value;
      line.append(span);
    }
    fragment.append(line);
    count += 1;
  }
  output.replaceChildren(fragment);
  document.querySelector("#logs-count").textContent = `${count} entries`;
  if (document.querySelector("#logs-autoscroll").checked) output.scrollTop = output.scrollHeight;
}

function appendLog(entry) {
  if (logState.ids.has(entry.id)) return;
  logState.ids.add(entry.id);
  logState.entries.push(entry);
  while (logState.entries.length > 2000) {
    logState.ids.delete(logState.entries.shift().id);
  }
  renderLogs();
}

async function startLogs() {
  if (logState.source) return;
  if (!logState.historyLoaded) {
    const history = await apiFetch("/api/v1/logs");
    for (const entry of history.entries || []) appendLog(entry);
    logState.historyLoaded = true;
  }
  const source = new EventSource("/api/v1/logs/stream");
  source.addEventListener("log", (event) => appendLog(JSON.parse(event.data)));
  source.onerror = () => {
    document.querySelector("#logs-count").textContent = "Stream reconnecting…";
  };
  logState.source = source;
  renderLogs();
}

function stopLogs() {
  logState.source?.close();
  logState.source = null;
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

function matchesInterface(item, query) {
  const normalized = String(query || "").trim().toLocaleLowerCase();
  if (!normalized) return true;
  return [item.name, interfaceType(item)]
    .some((value) => String(value || "").toLocaleLowerCase().includes(normalized));
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
  if (config.type === "UDPInterface") {
    const listen = `${config.listen_ip || "0.0.0.0"}:${config.listen_port ?? "—"}`;
    const forward = `${config.forward_ip || "—"}:${config.forward_port ?? "—"}`;
    return `${listen} → ${forward}`;
  }
  if (config.type === "AutoInterface") {
    return config.group_id || "reticulum";
  }
  if (config.type === "BackboneInterface") {
    const host = config.target_host || config.listen_on || "0.0.0.0";
    return `${host}:${config.port ?? "—"}`;
  }
  if (
    config.type === "SerialInterface"
    || config.type === "KISSInterface"
    || config.type === "RNodeInterface"
    || config.type === "AX25KISSInterface"
  ) {
    return config.port || "—";
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
    detailItem("Runtime ID", item.id === null ? "Not running" : formatNumber(item.id)),
    detailItem("Configuration", item.configured ? (item.enabled ? "Enabled" : "Disabled") : "Runtime-managed",
      item.configured ? "" : "runtime-only"),
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
  if (item.configured) {
    const actions = document.createElement("div");
    actions.className = "interface-actions";
    const editable = [
      "TCPClientInterface",
      "TCPServerInterface",
      "UDPInterface",
      "AutoInterface",
      "BackboneInterface",
      "SerialInterface",
      "KISSInterface",
      "RNodeInterface",
      "AX25KISSInterface",
    ].includes(item.config?.type);
    if (editable) {
      actions.append(actionButton("Edit configuration", "", () => openInterfaceDialog(item)));
    }
    actions.append(actionButton("Delete", "danger", () => openDeleteDialog(item)));
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
    id.textContent = item.id === null ? "config only" : `#${item.id}`;
    name.append(nameText, type, id);
    nameCell.append(name);

    const statusCell = document.createElement("td");
    const status = document.createElement("span");
    status.className = `status-chip${item.online ? " online" : ""}`;
    status.textContent = !item.enabled ? "Disabled" : item.online ? "Online" : "Offline";
    statusCell.append(status);

    const detailCell = document.createElement("td");
    const detailButton = document.createElement("button");
    const itemKey = item.name;
    const expanded = interfaceState.expanded.has(itemKey);
    detailButton.type = "button";
    detailButton.className = "details-button";
    detailButton.textContent = expanded ? "−" : "+";
    detailButton.setAttribute("aria-expanded", String(expanded));
    detailButton.setAttribute("aria-label", `${expanded ? "Hide" : "Show"} details for ${item.name}`);
    detailButton.addEventListener("click", () => {
      if (expanded) interfaceState.expanded.delete(itemKey);
      else interfaceState.expanded.add(itemKey);
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
  const query = interfaceState.filter.trim();
  const items = interfaceState.items.filter((item) => matchesInterface(item, query));
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

function matchesPath(path, query) {
  const normalized = String(query || "").trim().toLocaleLowerCase();
  if (!normalized) return true;
  return [path.hash, path.via, path.interface]
    .some((value) => String(value || "").toLocaleLowerCase().includes(normalized));
}

function renderPaths() {
  const query = pathState.filter.trim();
  const items = pathState.items.filter((path) => matchesPath(path, query));
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
  const udp = document.querySelector("#udp-fields");
  const auto = document.querySelector("#auto-fields");
  const backbone = document.querySelector("#backbone-fields");
  const serial = document.querySelector("#serial-fields");
  const kissSerial = document.querySelector("#kiss-fields");
  const rnode = document.querySelector("#rnode-fields");
  const ax25 = document.querySelector("#ax25-fields");
  const plugin = document.querySelector("#plugin-fields");
  const kiss = document.querySelector("#kiss-framing-field");
  const isClient = type === "TCPClientInterface";
  const isServer = type === "TCPServerInterface";
  const isUdp = type === "UDPInterface";
  const isAuto = type === "AutoInterface";
  const isBackbone = type === "BackboneInterface";
  const isSerial = type === "SerialInterface";
  const isKiss = type === "KISSInterface";
  const isRNode = type === "RNodeInterface";
  const isAx25 = type === "AX25KISSInterface";
  const isPlugin = type === "PluginInterface";
  client.hidden = !isClient;
  client.disabled = !isClient;
  server.hidden = !isServer;
  server.disabled = !isServer;
  udp.hidden = !isUdp;
  udp.disabled = !isUdp;
  auto.hidden = !isAuto;
  auto.disabled = !isAuto;
  backbone.hidden = !isBackbone;
  backbone.disabled = !isBackbone;
  serial.hidden = !isSerial;
  serial.disabled = !isSerial;
  kissSerial.hidden = !isKiss;
  kissSerial.disabled = !isKiss;
  rnode.hidden = !isRNode;
  rnode.disabled = !isRNode;
  ax25.hidden = !isAx25;
  ax25.disabled = !isAx25;
  plugin.hidden = !isPlugin;
  plugin.disabled = !isPlugin;
  const usesKissFraming = isClient || isServer;
  kiss.hidden = !usesKissFraming;
  document.querySelector("#kiss-framing").disabled = !usesKissFraming;
}

async function loadPlugins() {
  if (pluginState.loaded) return pluginState.items;
  if (!pluginState.request) {
    pluginState.request = apiFetch("/api/v1/plugins", { cache: "no-store" })
      .then((body) => {
        pluginState.items = (Array.isArray(body.plugins) ? body.plugins : [])
          .filter((plugin) => plugin.web_configurable && plugin.schema);
        pluginState.loaded = true;
        return pluginState.items;
      })
      .finally(() => { pluginState.request = null; });
  }
  return pluginState.request;
}

function schemaDefinition(schema, property) {
  if (!property?.$ref) return property;
  const prefix = "#/$defs/";
  return property.$ref.startsWith(prefix)
    ? schema.$defs?.[property.$ref.slice(prefix.length)]
    : null;
}

function schemaInput(property, value, required) {
  let input;
  if (Array.isArray(property.enum)) {
    input = document.createElement("select");
    for (const optionValue of property.enum) {
      const option = document.createElement("option");
      option.value = String(optionValue);
      option.textContent = String(optionValue);
      input.append(option);
    }
  } else {
    input = document.createElement("input");
    input.type = property.type === "string" ? "text" : "number";
    if (property.minimum != null) input.min = String(property.minimum);
    if (property.maximum != null) input.max = String(property.maximum);
    if (property.minLength != null) input.minLength = Number(property.minLength);
    input.step = property.type === "integer" ? "1" : "any";
  }
  input.required = required;
  const initial = value ?? property.default;
  if (initial != null) input.value = String(initial);
  input.dataset.pluginValueType = property.type || "string";
  return input;
}

function renderPluginSchema(pluginId, values = {}) {
  const container = document.querySelector("#plugin-config-fields");
  const description = document.querySelector("#plugin-description");
  container.replaceChildren();
  const plugin = pluginState.items.find((item) => item.id === pluginId);
  description.textContent = plugin
    ? `${plugin.name} ${plugin.version} — ${plugin.description}`
    : "This plugin is unavailable or does not publish a configuration schema.";
  if (!plugin) return;

  const schema = plugin.schema;
  const required = new Set(schema.required || []);
  const groups = new Map();
  const entries = Object.entries(schema.properties || {}).sort(([, left], [, right]) =>
    Number(left["x-order"] || 0) - Number(right["x-order"] || 0));

  for (const [key, declared] of entries) {
    const property = schemaDefinition(schema, declared) || declared;
    const groupName = declared["x-ui-group"] || property["x-ui-group"] || "Configuration";
    let group = groups.get(groupName);
    if (!group) {
      group = document.createElement("fieldset");
      group.className = "form-grid plugin-schema-group";
      const legend = document.createElement("legend");
      legend.textContent = groupName;
      group.append(legend);
      groups.set(groupName, group);
      container.append(group);
    }

    const title = declared.title || property.title || key;
    if (property.type === "object") {
      const wrapper = document.createElement("fieldset");
      wrapper.className = "form-grid wide plugin-object";
      const legend = document.createElement("legend");
      legend.textContent = title;
      wrapper.append(legend);
      const objectRequired = required.has(key);
      let toggle = null;
      if (!objectRequired) {
        const toggleLabel = document.createElement("label");
        toggleLabel.className = "check-field wide";
        toggle = document.createElement("input");
        toggle.type = "checkbox";
        toggle.checked = values[key] != null;
        toggle.dataset.pluginOptionalObject = key;
        const text = document.createElement("span");
        text.textContent = "Enable";
        toggleLabel.append(toggle, text);
        wrapper.append(toggleLabel);
      }
      const childRequired = new Set(property.required || []);
      for (const [childKey, childProperty] of Object.entries(property.properties || {})) {
        const label = document.createElement("label");
        label.textContent = childProperty.title || childKey;
        const input = schemaInput(childProperty, values[key]?.[childKey], childRequired.has(childKey));
        input.dataset.pluginObject = key;
        input.dataset.pluginChild = childKey;
        if (toggle && !toggle.checked) input.disabled = true;
        label.append(input);
        wrapper.append(label);
      }
      if (toggle) {
        toggle.addEventListener("change", () => {
          wrapper.querySelectorAll("[data-plugin-object]").forEach((input) => {
            input.disabled = !toggle.checked;
          });
        });
      }
      group.append(wrapper);
      continue;
    }

    const label = document.createElement("label");
    label.textContent = declared["x-unit"] ? `${title}, ${declared["x-unit"]}` : title;
    const input = schemaInput(property, values[key], required.has(key));
    input.dataset.pluginField = key;
    if (declared.description) input.title = declared.description;
    label.append(input);
    group.append(label);
  }
}

function populatePluginSelect(selected, values = {}) {
  const select = document.querySelector("#plugin-name");
  select.replaceChildren();
  for (const plugin of pluginState.items) {
    const option = document.createElement("option");
    option.value = plugin.id;
    option.textContent = `${plugin.name} ${plugin.version}`;
    select.append(option);
  }
  if (selected && !pluginState.items.some((plugin) => plugin.id === selected)) {
    const option = document.createElement("option");
    option.value = selected;
    option.textContent = `${selected} (schema unavailable)`;
    option.disabled = true;
    select.append(option);
  }
  select.value = selected || pluginState.items[0]?.id || "";
  renderPluginSchema(select.value, values);
}

function pluginConfigPayload() {
  const config = {};
  document.querySelectorAll("[data-plugin-field]").forEach((input) => {
    config[input.dataset.pluginField] = input.dataset.pluginValueType === "string"
      ? input.value
      : Number(input.value);
  });
  document.querySelectorAll("[data-plugin-object]").forEach((input) => {
    if (input.disabled) return;
    const key = input.dataset.pluginObject;
    config[key] ||= {};
    config[key][input.dataset.pluginChild] = input.dataset.pluginValueType === "string"
      ? input.value
      : Number(input.value);
  });
  return config;
}

function setBackboneRole(role) {
  const client = role === "client";
  document.querySelector("#backbone-target-field").hidden = !client;
  document.querySelector("#backbone-listen-field").hidden = client;
  document.querySelector("#backbone-target-host").required = client;
}

function setField(selector, value, fallback = "") {
  document.querySelector(selector).value = value ?? fallback;
}

async function openInterfaceDialog(item = null) {
  const dialog = document.querySelector("#interface-dialog");
  const form = document.querySelector("#interface-form");
  const config = item?.config || {};
  try {
    await loadPlugins();
  } catch (error) {
    pluginState.items = [];
    showError(error);
  }
  form.reset();
  setField("#interface-id", item?.id);
  setField("#interface-original-name", item?.name);
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
  setField("#udp-listen-ip", config.listen_ip);
  setField("#udp-listen-port", config.listen_port);
  setField("#udp-forward-ip", config.forward_ip);
  setField("#udp-forward-port", config.forward_port);
  setField("#udp-device", config.device);
  setField("#auto-group-id", config.group_id, "reticulum");
  setField("#auto-discovery-scope", config.discovery_scope, "link");
  setField("#auto-discovery-port", config.discovery_port, 29716);
  setField("#auto-data-port", config.data_port, 42671);
  setField(
    "#auto-multicast-address-type",
    config.multicast_address_type,
    "temporary",
  );
  setField("#auto-devices", config.devices);
  setField("#auto-ignored-devices", config.ignored_devices);
  setField("#auto-configured-bitrate", config.configured_bitrate);
  const backboneRole = config.target_host ? "client" : "listener";
  setField("#backbone-role", backboneRole);
  setField("#backbone-port", config.port);
  setField("#backbone-target-host", config.target_host);
  setField("#backbone-listen-on", config.listen_on);
  setField("#backbone-device", config.device);
  setField("#backbone-connect-timeout", config.connect_timeout, 5);
  setField("#backbone-reconnect-limit", config.max_reconnect_tries);
  document.querySelector("#backbone-prefer-ipv6").checked =
    Boolean(config.prefer_ipv6);
  document.querySelector("#backbone-i2p-tunneled").checked =
    Boolean(config.i2p_tunneled);
  setBackboneRole(backboneRole);
  setField("#advanced-bitrate", config.bitrate);
  setField("#advanced-announce-cap", config.announce_cap);
  setField("#advanced-rate-target", config.announce_rate_target);
  setField("#advanced-rate-grace", config.announce_rate_grace);
  setField("#advanced-rate-penalty", config.announce_rate_penalty);
  setField("#advanced-network-name", config.network_name);
  setField("#advanced-passphrase", config.passphrase);
  setField("#advanced-ifac-size", config.ifac_size);
  document.querySelector("#advanced-outgoing").checked = config.outgoing !== false;
  document.querySelector("#advanced-ingress-control").checked =
    config.ingress_control !== false;
  document.querySelector("#advanced-recursive-prs").checked =
    Boolean(config.recursive_prs);
  document.querySelector("#advanced-announces-internal").checked =
    config.announces_from_internal !== false;
  for (const [id, key] of [
    ["#advanced-ic-burst-freq-new", "ic_burst_freq_new"],
    ["#advanced-ic-burst-freq", "ic_burst_freq"],
    ["#advanced-ic-pr-burst-freq-new", "ic_pr_burst_freq_new"],
    ["#advanced-ic-pr-burst-freq", "ic_pr_burst_freq"],
    ["#advanced-ic-new-time", "ic_new_time"],
    ["#advanced-ic-burst-hold", "ic_burst_hold"],
    ["#advanced-ic-burst-penalty", "ic_burst_penalty"],
    ["#advanced-ic-max-held", "ic_max_held_announces"],
    ["#advanced-ic-held-release", "ic_held_release_interval"],
    ["#advanced-ec-pr-freq", "ec_pr_freq"],
  ]) setField(id, config[key]);
  setField(
    "#advanced-egress-control",
    config.egress_control == null ? "" : String(config.egress_control),
  );
  setField("#serial-port", config.port);
  setField("#serial-speed", config.speed, 9600);
  setField("#serial-databits", config.databits, 8);
  setField("#serial-parity", config.parity, "N");
  setField("#serial-stopbits", config.stopbits, 1);
  setField("#kiss-port", config.port);
  setField("#kiss-speed", config.speed, 9600);
  setField("#kiss-databits", config.databits, 8);
  setField("#kiss-parity", config.parity, "N");
  setField("#kiss-stopbits", config.stopbits, 1);
  setField("#kiss-preamble", config.preamble, 350);
  setField("#kiss-txtail", config.txtail, 20);
  setField("#kiss-persistence", config.persistence, 64);
  setField("#kiss-slottime", config.slottime, 20);
  document.querySelector("#kiss-flow-control").checked = Boolean(config.flow_control);
  setField("#rnode-port", config.port);
  setField("#rnode-frequency", config.frequency);
  setField("#rnode-bandwidth", config.bandwidth, 125000);
  setField("#rnode-spreading-factor", config.spreadingfactor, 8);
  setField("#rnode-coding-rate", config.codingrate, 5);
  setField("#rnode-tx-power", config.txpower, 7);
  setField("#rnode-airtime-short", config.airtime_limit_short);
  setField("#rnode-airtime-long", config.airtime_limit_long);
  document.querySelector("#rnode-flow-control").checked = Boolean(config.flow_control);
  setField("#ax25-port", config.port);
  setField("#ax25-callsign", config.callsign);
  setField("#ax25-ssid", config.ssid, 0);
  setField("#ax25-speed", config.speed, 9600);
  setField("#ax25-databits", config.databits, 8);
  setField("#ax25-parity", config.parity, "N");
  setField("#ax25-stopbits", config.stopbits, 1);
  setField("#ax25-preamble", config.preamble, 350);
  setField("#ax25-txtail", config.txtail, 20);
  setField("#ax25-persistence", config.persistence, 64);
  setField("#ax25-slottime", config.slottime, 20);
  setField("#plugin-mtu", config.mtu, 500);
  populatePluginSelect(config.plugin, config.config || {});
  document.querySelector("#ax25-flow-control").checked = Boolean(config.flow_control);
  document.querySelector("#prefer-ipv6").checked = Boolean(config.prefer_ipv6);
  document.querySelector("#kiss-framing").checked = Boolean(config.kiss_framing);
  document.querySelector("#interface-enabled").checked = item ? item.enabled !== false : true;
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

async function refreshSettings() {
  const data = await apiFetch("/api/v1/settings");
  const fields = {
    "#setting-instance-name": data.instance_name,
    "#setting-shared-type": data.shared_instance_type,
    "#setting-shared-port": data.shared_instance_port,
    "#setting-control-port": data.instance_control_port,
    "#setting-api-port": data.api_port,
    "#setting-api-user": data.api_user,
    "#setting-force-bitrate": data.force_shared_instance_bitrate,
    "#setting-ar-target": data.default_ar_target,
    "#setting-ar-grace": data.default_ar_grace,
    "#setting-ar-penalty": data.default_ar_penalty,
    "#setting-autoconnect": data.autoconnect_discovered_interfaces,
    "#setting-discovery-value": data.required_discovery_value,
    "#setting-loglevel": data.loglevel,
  };
  for (const [selector, value] of Object.entries(fields)) setField(selector, value);
  for (const [selector, key] of [
    ["#setting-share-instance", "share_instance"],
    ["#setting-enable-transport", "enable_transport"],
    ["#setting-static-identity", "static_transport_identity"],
    ["#setting-local-hops-delta", "local_hops_delta"],
    ["#setting-probes", "respond_to_probes"],
    ["#setting-implicit-proof", "use_implicit_proof"],
    ["#setting-panic-interface", "panic_on_interface_error"],
    ["#setting-mtu-discovery", "link_mtu_discovery"],
    ["#setting-discovery", "discover_interfaces"],
    ["#setting-logtimestamps", "logtimestamps"],
  ]) document.querySelector(selector).checked = Boolean(data[key]);
  document.querySelector("#settings-restart").hidden = !data.restart_required;
  setField("#setting-api-password", "");
}

async function saveSettings(event) {
  event.preventDefault();
  const form = event.currentTarget;
  if (!form.reportValidity()) return;
  const number = (id) => Number(document.querySelector(id).value);
  const optional = (id) => optionalInteger(id);
  const checked = (id) => document.querySelector(id).checked;
  const payload = {
    share_instance: checked("#setting-share-instance"),
    instance_name: document.querySelector("#setting-instance-name").value.trim(),
    shared_instance_type: document.querySelector("#setting-shared-type").value,
    shared_instance_port: number("#setting-shared-port"),
    instance_control_port: number("#setting-control-port"),
    api_port: number("#setting-api-port"),
    api_user: document.querySelector("#setting-api-user").value.trim(),
    enable_transport: checked("#setting-enable-transport"),
    static_transport_identity: checked("#setting-static-identity"),
    local_hops_delta: checked("#setting-local-hops-delta"),
    respond_to_probes: checked("#setting-probes"),
    use_implicit_proof: checked("#setting-implicit-proof"),
    panic_on_interface_error: checked("#setting-panic-interface"),
    link_mtu_discovery: checked("#setting-mtu-discovery"),
    discover_interfaces: checked("#setting-discovery"),
    autoconnect_discovered_interfaces: number("#setting-autoconnect"),
    required_discovery_value: number("#setting-discovery-value"),
    loglevel: number("#setting-loglevel"),
    logtimestamps: checked("#setting-logtimestamps"),
    force_shared_instance_bitrate: optional("#setting-force-bitrate"),
    default_ar_target: optional("#setting-ar-target"),
    default_ar_grace: optional("#setting-ar-grace"),
    default_ar_penalty: optional("#setting-ar-penalty"),
  };
  const apiPassword = document.querySelector("#setting-api-password").value;
  if (apiPassword) payload.api_password = apiPassword;
  const button = document.querySelector("#save-settings");
  setBusy(button, true);
  document.querySelector("#settings-error").textContent = "";
  try {
    await apiFetch("/api/v1/settings", {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
    });
    document.querySelector("#settings-restart").hidden = false;
  } catch (error) {
    document.querySelector("#settings-error").textContent = error.message;
  } finally {
    setBusy(button, false);
  }
}

async function waitForSystemReturn(button, action) {
  let observedOffline = false;
  for (let attempt = 0; attempt < 120; attempt += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 1000));
    try {
      const response = await fetch("/health", { cache: "no-store" });
      if (observedOffline && response.ok) {
        button.textContent = action === "reboot" ? "Reboot system" : "Restart daemon";
        setBusy(button, false);
        if (window.location.hash === "#dashboard") {
          showView("dashboard", { focus: true });
        } else {
          window.location.hash = "dashboard";
        }
        return;
      }
      if (!response.ok) observedOffline = true;
    } catch (_) {
      observedOffline = true;
    }
  }
  button.textContent = action === "reboot" ? "Reboot system" : "Restart daemon";
  setBusy(button, false);
  showError(new ApiError("System did not return within two minutes"));
}

async function requestSystemAction(action) {
  const reboot = action === "reboot";
  const message = reboot
    ? "Reboot the entire system? All services on the device will be interrupted."
    : "Restart rnsd-rs? Dependent services will be restarted by the external launcher.";
  if (!window.confirm(message)) return;
  const button = document.querySelector(reboot ? "#reboot-system" : "#restart-daemon");
  setBusy(button, true);
  button.textContent = reboot ? "Rebooting…" : "Restarting…";
  try {
    await apiFetch(`/api/v1/system/${action}`, { method: "POST" });
    document.querySelector("#settings-error").textContent =
      reboot ? "Reboot accepted. Waiting for the system." : "";
    waitForSystemReturn(button, action);
  } catch (error) {
    button.textContent = reboot ? "Reboot system" : "Restart daemon";
    setBusy(button, false);
    showError(error);
  }
}

function addAdvancedOptions(payload) {
  payload.outgoing = document.querySelector("#advanced-outgoing").checked;
  payload.ingress_control =
    document.querySelector("#advanced-ingress-control").checked;
  payload.recursive_prs =
    document.querySelector("#advanced-recursive-prs").checked;
  payload.announces_from_internal =
    document.querySelector("#advanced-announces-internal").checked;
  for (const [selector, key] of [
    ["#advanced-bitrate", "bitrate"],
    ["#advanced-announce-cap", "announce_cap"],
    ["#advanced-rate-target", "announce_rate_target"],
    ["#advanced-rate-grace", "announce_rate_grace"],
    ["#advanced-rate-penalty", "announce_rate_penalty"],
    ["#advanced-ifac-size", "ifac_size"],
    ["#advanced-ic-burst-freq-new", "ic_burst_freq_new"],
    ["#advanced-ic-burst-freq", "ic_burst_freq"],
    ["#advanced-ic-pr-burst-freq-new", "ic_pr_burst_freq_new"],
    ["#advanced-ic-pr-burst-freq", "ic_pr_burst_freq"],
    ["#advanced-ic-new-time", "ic_new_time"],
    ["#advanced-ic-burst-hold", "ic_burst_hold"],
    ["#advanced-ic-burst-penalty", "ic_burst_penalty"],
    ["#advanced-ic-max-held", "ic_max_held_announces"],
    ["#advanced-ic-held-release", "ic_held_release_interval"],
    ["#advanced-ec-pr-freq", "ec_pr_freq"],
  ]) {
    const value = document.querySelector(selector).value.trim();
    if (value !== "") payload[key] = Number(value);
  }
  for (const [selector, key] of [
    ["#advanced-network-name", "network_name"],
    ["#advanced-passphrase", "passphrase"],
  ]) {
    const value = document.querySelector(selector).value.trim();
    if (value) payload[key] = value;
  }
  const egress = document.querySelector("#advanced-egress-control").value;
  if (egress) payload.egress_control = egress === "true";
}

function interfacePayload() {
  const type = document.querySelector("#interface-type").value;
  const payload = {
    name: document.querySelector("#interface-name").value.trim(),
    type,
    interface_mode: document.querySelector("#interface-mode").value,
    enabled: document.querySelector("#interface-enabled").checked,
  };

  if (type === "TCPClientInterface") {
    payload.kiss_framing = document.querySelector("#kiss-framing").checked;
    payload.target_host = document.querySelector("#target-host").value.trim();
    payload.target_port = Number(document.querySelector("#target-port").value);
    const connectTimeout = optionalInteger("#connect-timeout");
    const reconnectTries = optionalInteger("#max-reconnect-tries");
    const fixedMtu = optionalInteger("#fixed-mtu");
    if (connectTimeout !== undefined) payload.connect_timeout = connectTimeout;
    if (reconnectTries !== undefined) payload.max_reconnect_tries = reconnectTries;
    if (fixedMtu !== undefined) payload.fixed_mtu = fixedMtu;
  } else if (type === "TCPServerInterface") {
    payload.kiss_framing = document.querySelector("#kiss-framing").checked;
    payload.listen_ip = document.querySelector("#listen-ip").value.trim();
    payload.listen_port = Number(document.querySelector("#listen-port").value);
    payload.prefer_ipv6 = document.querySelector("#prefer-ipv6").checked;
    const device = document.querySelector("#interface-device").value.trim();
    if (device) payload.device = device;
  } else if (type === "UDPInterface") {
    const listenIp = document.querySelector("#udp-listen-ip").value.trim();
    const listenPort = optionalInteger("#udp-listen-port");
    const forwardIp = document.querySelector("#udp-forward-ip").value.trim();
    const forwardPort = optionalInteger("#udp-forward-port");
    const device = document.querySelector("#udp-device").value.trim();
    if (listenIp) payload.listen_ip = listenIp;
    if (listenPort !== undefined) payload.listen_port = listenPort;
    if (forwardIp) payload.forward_ip = forwardIp;
    if (forwardPort !== undefined) payload.forward_port = forwardPort;
    if (device) payload.device = device;
  } else if (type === "AutoInterface") {
    payload.group_id = document.querySelector("#auto-group-id").value.trim();
    payload.discovery_scope =
      document.querySelector("#auto-discovery-scope").value;
    payload.discovery_port =
      Number(document.querySelector("#auto-discovery-port").value);
    payload.data_port = Number(document.querySelector("#auto-data-port").value);
    payload.multicast_address_type =
      document.querySelector("#auto-multicast-address-type").value;
    const devices = document.querySelector("#auto-devices").value.trim();
    const ignoredDevices =
      document.querySelector("#auto-ignored-devices").value.trim();
    const configuredBitrate = optionalInteger("#auto-configured-bitrate");
    if (devices) payload.devices = devices;
    if (ignoredDevices) payload.ignored_devices = ignoredDevices;
    if (configuredBitrate !== undefined) {
      payload.configured_bitrate = configuredBitrate;
    }
  } else if (type === "BackboneInterface") {
    payload.target_port = Number(document.querySelector("#backbone-port").value);
    payload.prefer_ipv6 =
      document.querySelector("#backbone-prefer-ipv6").checked;
    payload.i2p_tunneled =
      document.querySelector("#backbone-i2p-tunneled").checked;
    if (document.querySelector("#backbone-role").value === "client") {
      payload.target_host =
        document.querySelector("#backbone-target-host").value.trim();
    } else {
      const listenOn = document.querySelector("#backbone-listen-on").value.trim();
      if (listenOn) payload.listen_on = listenOn;
    }
    const device = document.querySelector("#backbone-device").value.trim();
    const timeout = optionalInteger("#backbone-connect-timeout");
    const reconnect = optionalInteger("#backbone-reconnect-limit");
    if (device) payload.device = device;
    if (timeout !== undefined) payload.connect_timeout = timeout;
    if (reconnect !== undefined) payload.max_reconnect_tries = reconnect;
  } else if (type === "SerialInterface") {
    payload.port = document.querySelector("#serial-port").value.trim();
    payload.speed = Number(document.querySelector("#serial-speed").value);
    payload.databits = Number(document.querySelector("#serial-databits").value);
    payload.parity = document.querySelector("#serial-parity").value;
    payload.stopbits = Number(document.querySelector("#serial-stopbits").value);
  } else if (type === "KISSInterface") {
    payload.port = document.querySelector("#kiss-port").value.trim();
    payload.speed = Number(document.querySelector("#kiss-speed").value);
    payload.databits = Number(document.querySelector("#kiss-databits").value);
    payload.parity = document.querySelector("#kiss-parity").value;
    payload.stopbits = Number(document.querySelector("#kiss-stopbits").value);
    payload.preamble = Number(document.querySelector("#kiss-preamble").value);
    payload.txtail = Number(document.querySelector("#kiss-txtail").value);
    payload.persistence = Number(document.querySelector("#kiss-persistence").value);
    payload.slottime = Number(document.querySelector("#kiss-slottime").value);
    payload.flow_control = document.querySelector("#kiss-flow-control").checked;
  } else if (type === "RNodeInterface") {
    payload.port = document.querySelector("#rnode-port").value.trim();
    payload.frequency = Number(document.querySelector("#rnode-frequency").value);
    payload.bandwidth = Number(document.querySelector("#rnode-bandwidth").value);
    payload.spreadingfactor = Number(
      document.querySelector("#rnode-spreading-factor").value,
    );
    payload.codingrate = Number(document.querySelector("#rnode-coding-rate").value);
    payload.txpower = Number(document.querySelector("#rnode-tx-power").value);
    payload.flow_control = document.querySelector("#rnode-flow-control").checked;
    const airtimeShort = optionalInteger("#rnode-airtime-short");
    const airtimeLong = optionalInteger("#rnode-airtime-long");
    if (airtimeShort !== undefined) payload.airtime_limit_short = airtimeShort;
    if (airtimeLong !== undefined) payload.airtime_limit_long = airtimeLong;
  } else if (type === "AX25KISSInterface") {
    payload.port = document.querySelector("#ax25-port").value.trim();
    payload.callsign = document.querySelector("#ax25-callsign").value.trim();
    payload.ssid = Number(document.querySelector("#ax25-ssid").value);
    payload.speed = Number(document.querySelector("#ax25-speed").value);
    payload.databits = Number(document.querySelector("#ax25-databits").value);
    payload.parity = document.querySelector("#ax25-parity").value;
    payload.stopbits = Number(document.querySelector("#ax25-stopbits").value);
    payload.preamble = Number(document.querySelector("#ax25-preamble").value);
    payload.txtail = Number(document.querySelector("#ax25-txtail").value);
    payload.persistence = Number(document.querySelector("#ax25-persistence").value);
    payload.slottime = Number(document.querySelector("#ax25-slottime").value);
    payload.flow_control = document.querySelector("#ax25-flow-control").checked;
  } else if (type === "PluginInterface") {
    payload.plugin = document.querySelector("#plugin-name").value;
    payload.mtu = Number(document.querySelector("#plugin-mtu").value);
    payload.config = pluginConfigPayload();
  }

  addAdvancedOptions(payload);
  return payload;
}

async function saveInterface(event) {
  event.preventDefault();
  const form = event.currentTarget;
  if (!form.reportValidity()) return;

  const originalName = document.querySelector("#interface-original-name").value;
  const button = document.querySelector("#save-interface");
  const errorNode = document.querySelector("#interface-form-error");
  const statusNode = document.querySelector("#interface-form-status");
  const editing = originalName !== "";
  errorNode.textContent = "";
  statusNode.textContent = editing ? "Restarting interface…" : "Starting interface…";
  setBusy(button, true);

  try {
    const endpoint = editing
      ? `/api/v1/config/interfaces/${encodeURIComponent(originalName)}`
      : "/api/v1/interfaces";
    await apiFetch(endpoint, {
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
  dialog.dataset.interfaceName = item.name;
  document.querySelector("#delete-interface-name").textContent = item.name;
  document.querySelector("#delete-interface-error").textContent = "";
  dialog.showModal();
  document.querySelector("#confirm-delete-interface").focus();
}

function closeDeleteDialog() {
  const dialog = document.querySelector("#delete-interface-dialog");
  dialog.close();
  delete dialog.dataset.interfaceName;
}

async function deleteInterface(event) {
  event.preventDefault();
  const dialog = document.querySelector("#delete-interface-dialog");
  const name = dialog.dataset.interfaceName;
  if (!name) return;

  const button = document.querySelector("#confirm-delete-interface");
  const errorNode = document.querySelector("#delete-interface-error");
  errorNode.textContent = "";
  setBusy(button, true);
  try {
    await apiFetch(`/api/v1/config/interfaces/${encodeURIComponent(name)}`, { method: "DELETE" });
    closeDeleteDialog();
    interfaceState.expanded.delete(name);
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
  if (selected === "settings") refreshSettings().catch(showError);
  if (selected === "logs") startLogs().catch(showError);
  else stopLogs();
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
  if (currentView() === "settings") {
    await refreshSettings();
    return;
  }
  if (currentView() === "logs") {
    await startLogs();
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

function initialize() {
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
  document.querySelector("#plugin-name").addEventListener("change", (event) => {
    renderPluginSchema(event.target.value, {});
  });
  document.querySelector("#backbone-role").addEventListener("change", (event) => {
    setBackboneRole(event.target.value);
  });
  document.querySelector("#open-interface-advanced").addEventListener("click", () => {
    document.querySelector("#interface-advanced-dialog").showModal();
  });
  document.querySelector("#interface-form").addEventListener("submit", saveInterface);
  document.querySelector("#login-form").addEventListener("submit", submitLogin);
  document.querySelector("#logout").addEventListener("click", () => {
    logout().catch(showError);
  });
  document.querySelector("#settings-form").addEventListener("submit", saveSettings);
  document.querySelector("#reload-settings").addEventListener("click", () => {
    refreshSettings().catch(showError);
  });
  document.querySelector("#restart-daemon").addEventListener("click", () => {
    requestSystemAction("restart");
  });
  document.querySelector("#reboot-system").addEventListener("click", () => {
    requestSystemAction("reboot");
  });
  document.querySelector("#logs-pause").addEventListener("click", (event) => {
    logState.paused = !logState.paused;
    event.currentTarget.textContent = logState.paused ? "Resume" : "Pause";
    if (!logState.paused) renderLogs();
  });
  document.querySelector("#logs-clear").addEventListener("click", () => {
    logState.entries = [];
    logState.ids.clear();
    renderLogs();
  });
  document.querySelector("#logs-search").addEventListener("input", (event) => {
    logState.filter = event.target.value;
    renderLogs();
  });
  document.querySelector("#logs-level").addEventListener("change", (event) => {
    logState.level = event.target.value;
    renderLogs();
  });
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
}

if (typeof module !== "undefined" && module.exports) {
  module.exports = {
    formatBytes,
    formatDuration,
    formatFrequency,
    formatNumber,
    formatRate,
    interfaceEndpoint,
    matchesInterface,
    matchesPath,
    relativeTimestamp,
  };
}

if (typeof document !== "undefined") initialize();
