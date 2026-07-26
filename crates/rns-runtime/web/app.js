"use strict";

const panel = document.querySelector(".panel");
const label = document.querySelector("#status-label");
const detail = document.querySelector("#status-detail");

async function checkHealth() {
  try {
    const response = await fetch("/health", {
      headers: { accept: "application/json" },
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);

    const health = await response.json();
    if (!health.ok) throw new Error("Unexpected health response");

    panel.dataset.state = "online";
    label.textContent = "rnsd-rs is online";
    detail.textContent =
      "The embedded Web UI is connected. Dashboard and configuration tools " +
      "will be added in the next implementation block.";
  } catch (error) {
    panel.dataset.state = "failed";
    label.textContent = "rnsd-rs is unavailable";
    detail.textContent = `Health check failed: ${error.message}`;
  }
}

checkHealth();
