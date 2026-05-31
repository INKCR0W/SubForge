import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { deletePlugin } from "./plugins";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

describe("plugins api", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("deletePlugin 会编码动态 plugin id 路径段", async () => {
    invokeMock.mockResolvedValueOnce({
      status: 200,
      headers: {},
      body: JSON.stringify({
        id: "plugin-record",
        plugin_id: "a/b",
        name: "插件",
        version: "1.0.0",
        spec_version: "1",
        plugin_type: "source",
        status: "enabled",
        installed_at: "2026-05-31T00:00:00Z",
        updated_at: "2026-05-31T00:00:00Z",
      }),
    });

    await deletePlugin("a/b");

    expect(invokeMock).toHaveBeenCalledWith("core_api_call", {
      request: {
        method: "DELETE",
        path: "/api/plugins/a%2Fb",
        body: null,
      },
    });
  });
});
