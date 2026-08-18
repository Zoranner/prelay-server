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

test("编辑供应商时仍向原生必填输入传递空密钥，由原生层保留旧密钥", () => {
  const page = readFileSync(new URL("../app/pages/providers.vue", import.meta.url), "utf8");
  expect(page).toMatch(/api_key:\s*payload\.api_key,\s*\n/);
});

test("供应商管理覆盖模型发现、协议测试和连通性状态", () => {
  const page = readFileSync(new URL("../app/pages/providers.vue", import.meta.url), "utf8");
  const list = readFileSync(new URL("../app/components/providers/ProviderList.vue", import.meta.url), "utf8");
  expect(page).toContain('"providers_discover_models"');
  expect(page).toContain('"providers_test_protocol"');
  expect(page).toContain('"providers_ping"');
  expect(page).toContain("providerProtocolOptions(provider)");
  expect(page).toContain("await loadProviders()");
  expect(list).toContain("api_key_masked");
});

test("供应商表单回显并保存全部能力覆盖", () => {
  expect(providerSource).toContain("capabilities: ProviderCapabilities");
  expect(providerSource).toContain("protocolBaseUrls");
  expect(providerSource).toContain("tool_calls");
  expect(providerSource).toContain("max_context_tokens");
  expect(providerSource).toContain("ref<boolean | null>(null)");
  expect(pageSource()).toContain("capabilities: payload.capabilities");
});

function pageSource() {
  return readFileSync(new URL("../app/pages/providers.vue", import.meta.url), "utf8");
}
