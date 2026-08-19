import { expect, mock, test } from "bun:test";

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
};

const requests: Deferred<unknown>[] = [];

mock.module("@tauri-apps/api/core", () => ({
  invoke: () => {
    let resolve!: (value: unknown) => void;
    let reject!: (reason: unknown) => void;
    const promise = new Promise<unknown>((settle, fail) => {
      resolve = settle;
      reject = fail;
    });
    requests.push({ promise, resolve, reject });
    return promise;
  },
}));

mock.module("~/utils/errors", () => ({
  toRelayError: (error: unknown) =>
    typeof error === "object" && error !== null
      ? (error as { code: string; message: string })
      : { code: "internal", message: String(error) },
}));

const { useRelayCommand, useRelayManagementApiStatus } =
  await import("../app/composables/useRelayCommand");

test("并行 command 在最后一个请求结束前保持 pending", async () => {
  requests.length = 0;
  const relay = useRelayCommand();
  const first = relay.invokeCommand("stats_overview");
  const second = relay.invokeCommand("stats_models");

  expect(relay.pending.value).toBe(true);
  requests[0]?.resolve({});
  await first;

  expect(relay.pending.value).toBe(true);
  requests[1]?.resolve([]);
  await second;
  expect(relay.pending.value).toBe(false);
});

test("管理 API 不可达时公开全局阻断状态", async () => {
  requests.length = 0;
  const relay = useRelayCommand();
  const managementApi = useRelayManagementApiStatus();
  const request = relay.invokeCommand("stats_overview");

  requests[0]?.reject({
    code: "network_error",
    message: "unable to reach the relay management API",
  });

  await expect(request).rejects.toEqual({
    code: "network_error",
    message: "unable to reach the relay management API",
  });
  expect(managementApi.error.value?.code).toBe("network_error");
});
