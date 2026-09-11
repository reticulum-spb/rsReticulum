"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const {
  inboundQueueRows,
  formatBytes,
  formatControlTraffic,
  formatBitRate,
  formatDuration,
  formatFrequency,
  formatNumber,
  formatRate,
  interfaceEndpoint,
  interfaceConfigEditable,
  matchesInterface,
  matchesPath,
} = require("./app.js");

test("renders independent queue occupancy and drops, with old-server fallback", () => {
  const rows = inboundQueueRows({ capacities: [2, 4, 8, 16], heights: [1, 4, 0, 8], dropped: [0, 3, 2, 1] });
  assert.deepEqual(rows.map((row) => row[1]), ["1 / 2 (50.0%)", "4 / 4 (100.0%)", "0 / 8 (0.0%)", "8 / 16 (50.0%)"]);
  assert.deepEqual(rows.map((row) => row[2]), ["0", "3", "2", "1"]);
  for (const missing of [undefined, null, {}, { capacities: [1] }]) {
    assert.ok(inboundQueueRows(missing).every((row) => row[1] === "Unavailable" && row[2] === "—"));
  }
  const invalid = inboundQueueRows({ capacities: [0, 2, 2, 2], heights: [0, -1, 3, 0], dropped: [null, -1, Number.MAX_SAFE_INTEGER + 1, 0] });
  assert.ok(invalid.slice(0, 3).every((row) => row[1] === "Unavailable" && row[2] === "—"));
});

test("allows editing schema-backed plugin interfaces", () => {
  assert.equal(interfaceConfigEditable("PluginInterface"), true);
});

test("formats runtime metrics", () => {
  assert.equal(formatBitRate(0), "0 bit/s");
  assert.equal(formatBitRate(12.5), `${(12.5).toLocaleString()} bit/s`);
  for (const unavailable of [null, undefined, -1, Infinity, NaN, "12"]) {
    assert.equal(formatBitRate(unavailable), "—");
  }
  assert.equal(formatControlTraffic(2, 1500), "2 / 1.5 kB");
  assert.equal(formatControlTraffic(0, 0), "0 / 0 B");
  for (const unavailable of [null, undefined, -1, Number.MAX_SAFE_INTEGER + 1]) {
    assert.equal(formatControlTraffic(unavailable, unavailable), "— / —");
  }
  assert.equal(formatBytes(0), "0 B");
  assert.equal(formatBytes(1500), "1.5 kB");
  assert.equal(formatRate(2500), "2.5 kB/s");
  assert.equal(formatNumber(null), "—");
  assert.equal(formatFrequency(0.992), "0.99/s");
  assert.equal(formatDuration(3720), "1h 2m");
});

test("filters interfaces by name and type without case sensitivity", () => {
  const item = {
    name: "Border SPB",
    config: { type: "TCPClientInterface" },
  };
  assert.equal(matchesInterface(item, "border spb"), true);
  assert.equal(matchesInterface(item, "BORDER"), true);
  assert.equal(matchesInterface(item, "tcpclient"), true);
  assert.equal(matchesInterface(item, "local server"), false);
});

test("formats UDP endpoints", () => {
  assert.equal(
    interfaceEndpoint({
      config: {
        type: "UDPInterface",
        listen_ip: "0.0.0.0",
        listen_port: 4242,
        forward_ip: "255.255.255.255",
        forward_port: 4242,
      },
    }),
    "0.0.0.0:4242 → 255.255.255.255:4242",
  );
});

test("formats Auto interface group", () => {
  assert.equal(
    interfaceEndpoint({
      config: { type: "AutoInterface", group_id: "field-network" },
    }),
    "field-network",
  );
});

test("formats Backbone endpoint", () => {
  assert.equal(
    interfaceEndpoint({
      config: {
        type: "BackboneInterface",
        target_host: "backbone.example",
        port: 4242,
      },
    }),
    "backbone.example:4242",
  );
});

test("formats serial endpoints", () => {
  assert.equal(
    interfaceEndpoint({
      config: { type: "SerialInterface", port: "/dev/ttyUSB0" },
    }),
    "/dev/ttyUSB0",
  );
  assert.equal(
    interfaceEndpoint({
      config: { type: "KISSInterface", port: "tcp://tnc.local:8001" },
    }),
    "tcp://tnc.local:8001",
  );
  assert.equal(
    interfaceEndpoint({
      config: { type: "RNodeInterface", port: "tcp://rnode.local:7633" },
    }),
    "tcp://rnode.local:7633",
  );
  assert.equal(
    interfaceEndpoint({
      config: { type: "AX25KISSInterface", port: "/dev/ttyUSB2" },
    }),
    "/dev/ttyUSB2",
  );
});

test("uses standard baud rates in every serial form", () => {
  const html = fs.readFileSync(require.resolve("./index.html"), "utf8");
  const expected = [
    "1200", "2400", "4800", "9600", "19200", "38400",
    "57600", "115200", "230400", "460800", "921600",
  ];
  for (const id of ["serial-speed", "kiss-speed", "ax25-speed"]) {
    const options = html
      .match(new RegExp(`id="${id}"[^>]*>([\\s\\S]*?)</select>`))[1]
      .matchAll(/<option value="(\d+)"/g);
    assert.deepEqual(Array.from(options, (match) => match[1]), expected);
  }
});

test("filters paths by destination, next hop, and interface name", () => {
  const path = {
    hash: "AABB001122334455",
    via: "6100799F1C81062F",
    interface: "Border SPB",
  };
  assert.equal(matchesPath(path, "aabb00"), true);
  assert.equal(matchesPath(path, "6100799f"), true);
  assert.equal(matchesPath(path, "border spb"), true);
  assert.equal(matchesPath(path, "BORDER SPB"), true);
  assert.equal(matchesPath(path, "Local server"), false);
});
