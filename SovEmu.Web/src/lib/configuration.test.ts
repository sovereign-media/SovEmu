import { describe, expect, it } from "vitest";
import type { ConfigSetting } from "./api";
import { codeSpans, environmentLines, filterSettings, groupSettings } from "./configuration";

const setting = (over: Partial<ConfigSetting>): ConfigSetting => ({
  group: "STREAMING",
  name: "SovEmu__Http__Listen",
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
    setting({
      group: "ACCOUNTS",
      name: "SovEmu__Authentication__AdminPassword",
      value: "(set)",
      secret: true,
      source: "ENVIRONMENT",
    }),
    setting({ group: "ACCOUNTS", name: "SovEmu__Authentication__SessionDays", value: "30", source: "ENVIRONMENT" }),
    setting({ description: "Address for the web client and API." }),
    setting({
      name: "SovEmu__Streaming__Udp__Ports",
      value: "6000-6009",
      source: "COMMAND_LINE",
      description: "UDP ports",
    }),
  ];

  it("finds settings by name, value or description", () => {
    expect(filterSettings(all, "").length).toBe(4);
    expect(filterSettings(all, "udp").map((s) => s.name)).toEqual(["SovEmu__Streaming__Udp__Ports"]);
    expect(filterSettings(all, "web client").map((s) => s.name)).toEqual(["SovEmu__Http__Listen"]);
    expect(filterSettings(all, "6000").map((s) => s.name)).toEqual(["SovEmu__Streaming__Udp__Ports"]);
  });

  it("groups settings in the server's order", () => {
    expect(groupSettings(all).map((g) => [g.group, g.settings.length])).toEqual([
      ["ACCOUNTS", 2],
      ["STREAMING", 2],
    ]);
  });

  it("copies what was given, never a secret", () => {
    expect(environmentLines(all)).toEqual([
      "SovEmu__Authentication__SessionDays=30",
      "SovEmu__Streaming__Udp__Ports=6000-6009",
    ]);
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
