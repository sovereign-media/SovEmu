import { describe, expect, it } from "vitest";
import type { ConfigSetting } from "./api";
import { codeSpans, environmentLines, filterSettings, groupSettings } from "./configuration";

const setting = (over: Partial<ConfigSetting>): ConfigSetting => ({
  group: "STREAMING",
  name: "SOVEMU_HTTP",
  value: "0.0.0.0:8080",
  effective: null,
  source: "DEFAULT",
  secret: false,
  default: "0.0.0.0:8080",
  description: "",
  ...over,
});

describe("the configuration page", () => {
  const all = [
    setting({ group: "ACCOUNTS", name: "SOVEMU_ADMIN_PASSWORD", value: "(set)", secret: true, source: "FILE" }),
    setting({ group: "ACCOUNTS", name: "SOVEMU_SESSION_DAYS", value: "30", source: "ENVIRONMENT" }),
    setting({ description: "Address for the web client and API." }),
    setting({ name: "SOVEMU_UDP_PORTS", value: "6000-6009", source: "COMMAND_LINE", description: "UDP ports" }),
  ];

  it("finds settings by name, value or description", () => {
    expect(filterSettings(all, "").length).toBe(4);
    expect(filterSettings(all, "udp").map((s) => s.name)).toEqual(["SOVEMU_UDP_PORTS"]);
    expect(filterSettings(all, "web client").map((s) => s.name)).toEqual(["SOVEMU_HTTP"]);
    expect(filterSettings(all, "6000").map((s) => s.name)).toEqual(["SOVEMU_UDP_PORTS"]);
  });

  it("groups settings in the server's order", () => {
    expect(groupSettings(all).map((g) => [g.group, g.settings.length])).toEqual([
      ["ACCOUNTS", 2],
      ["STREAMING", 2],
    ]);
  });

  it("copies what was given, never a secret", () => {
    expect(environmentLines(all)).toEqual(["SOVEMU_SESSION_DAYS=30", "SOVEMU_UDP_PORTS=6000-6009"]);
  });
});

describe("descriptions", () => {
  it("show their code spans as code", () => {
    expect(codeSpans("Unset: `localhost` and `127.0.0.1`.")).toEqual([
      { code: false, text: "Unset: " },
      { code: true, text: "localhost" },
      { code: false, text: " and " },
      { code: true, text: "127.0.0.1" },
      { code: false, text: "." },
    ]);
    expect(codeSpans("plain")).toEqual([{ code: false, text: "plain" }]);
  });
});
