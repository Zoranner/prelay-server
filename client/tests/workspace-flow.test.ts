import { expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";

const source = (path: string) =>
  readFileSync(new URL(`../app/${path}`, import.meta.url), "utf8");

test("桌面壳层按工作流组织客户端入口", () => {
  const app = source("app.vue");

  expect(app).toContain("workspace-nav");
  expect(app).toContain('label: "工作台"');
  expect(app).toContain('label: "供应商"');
  expect(app).toContain('label: "接入"');
  expect(app).toContain('label: "活动"');
  expect(app).toContain('label: "设置"');
  expect(app).not.toContain("app-nav");
});

test("首次启动和服务配置拥有独立页面", () => {
  expect(existsSync(new URL("../app/pages/setup.vue", import.meta.url))).toBe(
    true,
  );
  expect(
    existsSync(new URL("../app/pages/settings.vue", import.meta.url)),
  ).toBe(true);
});
