import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { refreshSource } from "./sources";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

describe("sources api", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("refreshSource 会编码动态 source id 路径段", async () => {
    invokeMock.mockResolvedValueOnce({
      status: 200,
      headers: {},
      body: JSON.stringify({ source_id: "a/b", node_count: 1 }),
    });

    await refreshSource("a/b");

    expect(invokeMock).toHaveBeenCalledWith("core_api_call", {
      request: {
        method: "POST",
        path: "/api/sources/a%2Fb/refresh",
        body: null,
      },
    });
  });
});
