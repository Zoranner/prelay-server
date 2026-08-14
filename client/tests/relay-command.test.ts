import { expect, mock, test } from "bun:test";

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
};

const requests: Deferred<unknown>[] = [];

mock.module("@tauri-apps/api/core", () => ({
  invoke: () => {
    let resolve!: (value: unknown) => void;
    const promise = new Promise<unknown>((settle) => {
      resolve = settle;
    });
    requests.push({ promise, resolve });
    return promise;
  },
}));

mock.module("~/utils/errors", () => ({
  toRelayError: (error: unknown) => ({ code: "internal", message: String(error) }),
}));

const { useRelayCommand } = await import("../app/composables/useRelayCommand");

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
