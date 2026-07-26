"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");
const {
  formatBytes,
  formatDuration,
  formatFrequency,
  formatNumber,
  formatRate,
  matchesInterface,
  matchesPath,
} = require("./app.js");

test("formats runtime metrics", () => {
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
