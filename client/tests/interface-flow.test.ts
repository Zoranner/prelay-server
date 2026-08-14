import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const interfacePage = readFileSync(
  new URL("../app/pages/interfaces.vue", import.meta.url),
  "utf8",
);

test("接口页面提供模型映射、根 v1 地址和 Token 重置", () => {
  expect(interfacePage).toContain('"interfaces_save"');
  expect(interfacePage).toContain('"interfaces_regenerate_token"');
  expect(interfacePage).toContain("/v1/");
  expect(interfacePage).toContain("upstream_model");
  expect(interfacePage).not.toContain("/proxy");
});
