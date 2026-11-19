import { describe, expect, it } from "vitest";

import {
  APPS_EXTENSION_ID,
  APPS_RESOURCE_MIME_TYPE,
  OBSERVATION_APP_HTML,
  OBSERVATION_APP_URI,
  OBSERVATION_RESOURCE_META,
  OBSERVATION_TOOL_META,
  resolveAppsSupport,
} from "../src/apps.js";

const appsCapability = {
  extensions: {
    [APPS_EXTENSION_ID]: { mimeTypes: [APPS_RESOURCE_MIME_TYPE] },
  },
};

describe("MCP Apps negotiation", () => {
  it("negotiates only when the client declares the UI extension and MIME type", () => {
    expect(resolveAppsSupport({ clientCapabilities: appsCapability })).toEqual({
      mode: "apps",
      reason: "apps_negotiated",
      extensionAdvertised: true,
      mimeType: APPS_RESOURCE_MIME_TYPE,
    });
    expect(
      resolveAppsSupport({
        clientCapabilities: {
          extensions: {
            [APPS_EXTENSION_ID]: {
              mimeTypes: [APPS_RESOURCE_MIME_TYPE.toUpperCase()],
            },
          },
        },
      }).mode,
    ).toBe("apps");
  });

  it("falls back without the extension or with a different MIME type", () => {
    expect(resolveAppsSupport({ clientCapabilities: null })).toEqual({
      mode: "fallback",
      reason: "client_did_not_declare_apps",
      extensionAdvertised: false,
      mimeType: APPS_RESOURCE_MIME_TYPE,
    });
    expect(
      resolveAppsSupport({ clientCapabilities: { roots: {} } }).reason,
    ).toBe("client_did_not_declare_apps");
    expect(
      resolveAppsSupport({
        clientCapabilities: {
          extensions: { [APPS_EXTENSION_ID]: { mimeTypes: ["text/html"] } },
        },
      }).reason,
    ).toBe("client_mime_type_missing");
    expect(
      resolveAppsSupport({
        clientCapabilities: { extensions: { [APPS_EXTENSION_ID]: {} } },
      }).reason,
    ).toBe("client_mime_type_missing");
  });

  it("never claims negotiation when the bridge does not implement Apps", () => {
    expect(
      resolveAppsSupport({
        clientCapabilities: appsCapability,
        bridgeImplementsApps: false,
      }),
    ).toMatchObject({ mode: "fallback", reason: "apps_not_implemented" });
  });
});

describe("observation app resource", () => {
  it("is a self-contained HTML5 document with no external references", () => {
    expect(OBSERVATION_APP_HTML.startsWith("<!DOCTYPE html>")).toBe(true);
    expect(OBSERVATION_APP_HTML).toContain("ui/initialize");
    expect(OBSERVATION_APP_HTML).toContain("ui/notifications/initialized");
    expect(OBSERVATION_APP_HTML).toContain("ui/notifications/tool-result");
    expect(OBSERVATION_APP_HTML).toContain("ui/resource-teardown");
    expect(OBSERVATION_APP_HTML).not.toMatch(/https?:\/\//);
    expect(OBSERVATION_APP_HTML).not.toContain("<script src");
    expect(OBSERVATION_APP_HTML).not.toContain("<link");
    expect(OBSERVATION_APP_HTML).not.toContain("eval(");
    expect(OBSERVATION_APP_HTML).not.toContain("new Function");
    expect(OBSERVATION_APP_HTML).not.toContain("document.write");
    expect(OBSERVATION_APP_HTML).not.toContain("innerHTML");
  });

  it("declares a restrictive resource policy and links the observe tool", () => {
    expect(OBSERVATION_APP_URI.startsWith("ui://")).toBe(true);
    expect(APPS_RESOURCE_MIME_TYPE).toBe("text/html;profile=mcp-app");
    expect(OBSERVATION_RESOURCE_META).toEqual({
      ui: {
        csp: {
          connectDomains: [],
          resourceDomains: [],
          frameDomains: [],
          baseUriDomains: [],
        },
        prefersBorder: true,
      },
    });
    expect(OBSERVATION_TOOL_META).toEqual({
      ui: {
        resourceUri: OBSERVATION_APP_URI,
        visibility: ["model", "app"],
      },
    });
  });

  it("validates image payloads instead of trusting arbitrary content", () => {
    expect(OBSERVATION_APP_HTML).toContain('"data:image/png;base64,"');
    expect(OBSERVATION_APP_HTML).toContain("MAX_BASE64_LENGTH");
    expect(OBSERVATION_APP_HTML).toContain("BASE64.test");
    expect(OBSERVATION_APP_HTML).toContain('entry.mimeType === "image/png"');
    expect(OBSERVATION_APP_HTML).toContain("metadata.warnings");
  });
});
