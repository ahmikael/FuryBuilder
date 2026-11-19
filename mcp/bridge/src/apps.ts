import type { McpServer } from "@modelcontextprotocol/server";

import { BRIDGE_IMPLEMENTS_APPS } from "./capabilities.js";

/**
 * MCP Apps (SEP-1865, extension `io.modelcontextprotocol/ui`).
 *
 * The bridge only uses this extension as an optional presentation layer. Every
 * `observe` call still returns an ordinary PNG image and JSON metadata, so a
 * host that does not declare the extension keeps the same behavior.
 */
export const APPS_EXTENSION_ID = "io.modelcontextprotocol/ui";
export const APPS_RESOURCE_MIME_TYPE = "text/html;profile=mcp-app";
export const OBSERVATION_APP_URI = "ui://furybuilder/observation";
export const OBSERVATION_APP_NAME = "furybuilder-observation";
export const OBSERVATION_APP_VERSION = "0.1.0";
export const OBSERVATION_TOOL_NAME = "observe";

export type AppsFallbackReason =
  | "client_did_not_declare_apps"
  | "client_mime_type_missing"
  | "apps_not_implemented";

export interface AppsSupportDecision {
  mode: "apps" | "fallback";
  reason: "apps_negotiated" | AppsFallbackReason;
  extensionAdvertised: boolean;
  mimeType: string;
}

function appsSettings(
  clientCapabilities: unknown,
): Record<string, unknown> | null {
  if (typeof clientCapabilities !== "object" || clientCapabilities === null) {
    return null;
  }
  const extensions = (clientCapabilities as Record<string, unknown>).extensions;
  if (typeof extensions !== "object" || extensions === null) {
    return null;
  }
  const settings = (extensions as Record<string, unknown>)[APPS_EXTENSION_ID];
  if (typeof settings !== "object" || settings === null) {
    return null;
  }
  return settings as Record<string, unknown>;
}

/**
 * Negotiages MCP Apps per connection. A missing extension or MIME type is a
 * normal fallback, never an error: the ordinary observation result is complete.
 */
export function resolveAppsSupport(input: {
  clientCapabilities: unknown;
  bridgeImplementsApps?: boolean;
}): AppsSupportDecision {
  const implementsApps = input.bridgeImplementsApps ?? BRIDGE_IMPLEMENTS_APPS;
  const settings = appsSettings(input.clientCapabilities);
  if (!settings) {
    return {
      mode: "fallback",
      reason: "client_did_not_declare_apps",
      extensionAdvertised: false,
      mimeType: APPS_RESOURCE_MIME_TYPE,
    };
  }
  const mimeTypes = settings.mimeTypes;
  const supportsMimeType =
    Array.isArray(mimeTypes) &&
    mimeTypes.some(
      (value) =>
        typeof value === "string" &&
        value.toLowerCase() === APPS_RESOURCE_MIME_TYPE,
    );
  if (!supportsMimeType) {
    return {
      mode: "fallback",
      reason: "client_mime_type_missing",
      extensionAdvertised: false,
      mimeType: APPS_RESOURCE_MIME_TYPE,
    };
  }
  if (!implementsApps) {
    return {
      mode: "fallback",
      reason: "apps_not_implemented",
      extensionAdvertised: false,
      mimeType: APPS_RESOURCE_MIME_TYPE,
    };
  }
  return {
    mode: "apps",
    reason: "apps_negotiated",
    extensionAdvertised: true,
    mimeType: APPS_RESOURCE_MIME_TYPE,
  };
}

/**
 * Restrictive metadata for the static observation view. The view needs no
 * network, no nested frames and no external assets; inline styles and scripts
 * are enough to render the PNG passed in the tool result.
 */
export const OBSERVATION_RESOURCE_META = {
  ui: {
    csp: {
      connectDomains: [],
      resourceDomains: [],
      frameDomains: [],
      baseUriDomains: [],
    },
    prefersBorder: true,
  },
} as const;

export const OBSERVATION_TOOL_META = {
  ui: {
    resourceUri: OBSERVATION_APP_URI,
    visibility: ["model", "app"],
  },
} as const;

/**
 * The view is deliberately static: it contains no model-supplied text and no
 * external references. It renders only a validated PNG data URL from the tool
 * result; all other fields are written through textContent.
 */
export const OBSERVATION_APP_HTML = `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>FuryBuilder observation</title>
<style>
  :root {
    color-scheme: light dark;
    --bg: #101318;
    --panel: #171b22;
    --text: #e7eaf0;
    --muted: #9aa4b2;
    --border: #2a303a;
    --accent: #6ea8fe;
  }
  @media (prefers-color-scheme: light) {
    :root {
      --bg: #f7f8fa;
      --panel: #ffffff;
      --text: #1b1f24;
      --muted: #5b6573;
      --border: #d9dee6;
      --accent: #1d5fd1;
    }
  }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    background: var(--bg);
    color: var(--text);
    font: 13px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif;
  }
  main { display: grid; gap: 10px; padding: 12px; }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }
  h1 { font-size: 14px; margin: 0; letter-spacing: 0.01em; }
  #status { color: var(--muted); }
  figure {
    margin: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: var(--panel);
  }
  #image {
    display: block;
    width: 100%;
    height: auto;
    image-rendering: pixelated;
  }
  figcaption {
    padding: 8px 10px;
    border-top: 1px solid var(--border);
    color: var(--muted);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 2px 12px;
    margin: 0;
  }
  dt { color: var(--muted); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .error { color: #ff8f8f; }
</style>
</head>
<body>
<main>
  <header>
    <h1>FuryBuilder observation</h1>
    <span id="status">Connecting…</span>
  </header>
  <figure id="preview" hidden>
    <img id="image" alt="FuryBuilder observation preview">
    <figcaption id="caption"></figcaption>
  </figure>
  <dl id="meta"></dl>
</main>
<script>
(function () {
  "use strict";

  var PROTOCOL_VERSION = "2026-01-26";
  var PNG_PREFIX = "data:image/png;base64,";
  var MAX_BASE64_LENGTH = 24 * 1024 * 1024;
  var BASE64 = /^[A-Za-z0-9+/]+={0,2}$/;
  var nextId = 1;
  var pending = new Map();
  var initialized = false;
  var queuedResult = null;
  var statusNode = document.getElementById("status");
  var figure = document.getElementById("preview");
  var image = document.getElementById("image");
  var caption = document.getElementById("caption");
  var meta = document.getElementById("meta");

  function post(message) { window.parent.postMessage(message, "*"); }

  function notify(method, params) {
    post({ jsonrpc: "2.0", method: method, params: params || {} });
  }

  function request(method, params) {
    var id = nextId++;
    return new Promise(function (resolve, reject) {
      var timer = setTimeout(function () {
        pending.delete(id);
        reject(new Error("host did not answer " + method));
      }, 10000);
      pending.set(id, { resolve: resolve, reject: reject, timer: timer });
      post({ jsonrpc: "2.0", id: id, method: method, params: params });
    });
  }

  function setStatus(text, isError) {
    statusNode.textContent = text;
    statusNode.className = isError ? "error" : "";
  }

  function clearMeta() {
    while (meta.firstChild) meta.removeChild(meta.firstChild);
  }

  function addMeta(label, value) {
    if (value === undefined || value === null || value === "") return;
    var term = document.createElement("dt");
    term.textContent = label;
    var description = document.createElement("dd");
    description.textContent = String(value);
    meta.appendChild(term);
    meta.appendChild(description);
  }

  function describeBounds(bounds) {
    if (!bounds || typeof bounds !== "object") return "";
    var min = bounds.min;
    var max = bounds.max;
    if (!min || !max) return "";
    return "(" + min.x + ", " + min.y + ", " + min.z + ") to (" +
      max.x + ", " + max.y + ", " + max.z + ")";
  }

  function render(result) {
    var content = result && Array.isArray(result.content) ? result.content : [];
    var imageBlock = null;
    var text = "";
    for (var index = 0; index < content.length; index += 1) {
      var entry = content[index];
      if (!imageBlock && entry && entry.type === "image" &&
          entry.mimeType === "image/png" && typeof entry.data === "string") {
        imageBlock = entry;
      }
      if (!text && entry && entry.type === "text" && typeof entry.text === "string") {
        text = entry.text;
      }
    }
    if (!imageBlock) {
      figure.hidden = true;
      setStatus("This result has no PNG preview; use the image content returned by observe.", true);
      return;
    }
    if (imageBlock.data.length > MAX_BASE64_LENGTH || !BASE64.test(imageBlock.data)) {
      figure.hidden = true;
      setStatus("Preview rejected: invalid PNG payload.", true);
      return;
    }
    var metadata = null;
    if (text) {
      try { metadata = JSON.parse(text); } catch (ignored) { metadata = null; }
    }
    clearMeta();
    if (metadata && typeof metadata === "object") {
      addMeta("Mode", metadata.mode);
      addMeta("Dimension", metadata.dimension);
      addMeta("Bounds", describeBounds(metadata.bounds));
      addMeta("Revision", metadata.worldRevision);
      addMeta("Scale", metadata.scale);
      addMeta("Legend", Array.isArray(metadata.legend) ? metadata.legend.length + " entries" : undefined);
      addMeta("Warnings", Array.isArray(metadata.warnings) ? metadata.warnings.join("; ") : metadata.warnings);
    }
    image.onload = function () {
      figure.hidden = false;
      setStatus("Observation ready.");
      notify("ui/notifications/size-changed", {
        width: Math.ceil(document.documentElement.scrollWidth),
        height: Math.ceil(document.documentElement.scrollHeight)
      });
    };
    image.onerror = function () {
      figure.hidden = true;
      setStatus("Preview failed to decode the PNG image.", true);
    };
    image.src = PNG_PREFIX + imageBlock.data;
    caption.textContent = metadata && metadata.mode
      ? "Mode: " + metadata.mode
      : "Observation preview";
  }

  window.addEventListener("message", function (event) {
    var data = event.data;
    if (!data || data.jsonrpc !== "2.0") return;
    if (Object.prototype.hasOwnProperty.call(data, "id") &&
        (Object.prototype.hasOwnProperty.call(data, "result") ||
         Object.prototype.hasOwnProperty.call(data, "error"))) {
      var slot = pending.get(data.id);
      if (!slot) return;
      clearTimeout(slot.timer);
      pending.delete(data.id);
      if (data.error) {
        slot.reject(new Error(typeof data.error.message === "string" ? data.error.message : "host error"));
      } else {
        slot.resolve(data.result);
      }
      return;
    }
    if (data.method === "ui/notifications/tool-result") {
      queuedResult = data.params;
      if (initialized) render(data.params);
      return;
    }
    if (data.method === "ui/notifications/tool-cancelled") {
      setStatus("Observation cancelled.", false);
      return;
    }
    if (data.method === "ping") {
      post({ jsonrpc: "2.0", id: data.id, result: {} });
      return;
    }
    if (data.method === "ui/resource-teardown") {
      post({ jsonrpc: "2.0", id: data.id, result: {} });
    }
  });

  request("ui/initialize", {
    protocolVersion: PROTOCOL_VERSION,
    appInfo: { name: "furybuilder-observation", version: "0.1.0" },
    appCapabilities: { availableDisplayModes: ["inline", "fullscreen"] }
  }).then(function () {
    initialized = true;
    notify("ui/notifications/initialized", {});
    setStatus("Waiting for an observe result…");
    if (queuedResult) render(queuedResult);
  }).catch(function () {
    setStatus("The host did not complete the MCP Apps handshake; use the PNG returned by observe.", true);
  });
})();
</script>
</body>
</html>
`;

/** Registers the static UI resource referenced by the observe tool. */
export function registerObservationApp(server: McpServer): void {
  server.registerResource(
    OBSERVATION_APP_NAME,
    OBSERVATION_APP_URI,
    {
      title: "FuryBuilder observation preview",
      description:
        "Interactive MCP Apps view for observe results. Hosts without MCP Apps keep the PNG image and JSON metadata returned by the observe tool.",
      mimeType: APPS_RESOURCE_MIME_TYPE,
      annotations: { audience: ["user"], priority: 0.5 },
      _meta: OBSERVATION_RESOURCE_META,
    },
    async (uri) => ({
      contents: [
        {
          uri: uri.href,
          mimeType: APPS_RESOURCE_MIME_TYPE,
          text: OBSERVATION_APP_HTML,
          _meta: OBSERVATION_RESOURCE_META,
        },
      ],
    }),
  );
}
