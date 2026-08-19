import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = (path: string) =>
  readFileSync(new URL(`../app/${path}`, import.meta.url), "utf8");

test("Nuxt 管理页面只通过固定的 Tauri command 调用服务端", () => {
  const commands = source("composables/useRelayCommand.ts");

  for (const command of [
    "bootstrap",
    "providers_list",
    "providers_save",
    "providers_delete",
    "providers_ping",
    "providers_discover_models",
    "providers_test_protocol",
    "interfaces_list",
    "interfaces_save",
    "interfaces_delete",
    "interfaces_regenerate_token",
    "stats_overview",
    "stats_requests",
    "stats_models",
    "stats_providers",
    "credential_rotate",
  ]) {
    expect(commands).toContain(`"${command}"`);
  }

  expect(commands).toContain("@tauri-apps/api/core");
  expect(commands).toContain("invoke");
});

test("Nuxt 页面不直连服务端或读取认证凭据", () => {
  for (const page of [
    "pages/index.vue",
    "pages/providers.vue",
    "pages/interfaces.vue",
    "pages/stats.vue",
  ]) {
    const content = source(page);
    expect(content).not.toContain("fetch(");
    expect(content).not.toContain("Authorization");
    expect(content).not.toContain("device-credential");
    expect(content).not.toContain("localStorage");
  }
});
