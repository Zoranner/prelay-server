import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const providerSource = readFileSync(
  new URL("../app/components/providers/ProviderForm.vue", import.meta.url),
  "utf8",
);

test("保存供应商后立即清除只用于请求的密钥输入", () => {
  expect(providerSource).toContain("apiKey.value = \"\"");
  expect(providerSource).toContain('emit("save"');
});

test("供应商管理覆盖模型发现、协议测试和连通性状态", () => {
  const page = readFileSync(new URL("../app/pages/providers.vue", import.meta.url), "utf8");
  const list = readFileSync(new URL("../app/components/providers/ProviderList.vue", import.meta.url), "utf8");
  expect(page).toContain('"providers_discover_models"');
  expect(page).toContain('"providers_test_protocol"');
  expect(page).toContain('"providers_ping"');
  expect(list).toContain("api_key_masked");
});
