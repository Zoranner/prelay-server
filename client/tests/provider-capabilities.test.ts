import { expect, test } from "bun:test";

import type { Provider } from "../app/stores/relay";
import { providerProtocolOptions } from "../app/utils/providerCapabilities";

const provider = (overrides: Partial<Provider> = {}): Provider => ({
  id: "provider-1",
  name: "测试供应商",
  provider_type: "openai_compatible",
  base_url: "https://api.example.com/v1",
  api_key_masked: "sk-***",
  capabilities: {
    upstream_protocols: ["responses", "anthropic"],
    protocol_base_urls: {
      responses: "https://responses.example.com/v1",
      openai: null,
      anthropic: "https://anthropic.example.com/v1",
    },
    tool_calls: true,
    reasoning: false,
    tool_choice: true,
    parallel_tool_calls: false,
    system_messages: true,
    structured_outputs: true,
    streaming_usage: false,
    max_context_tokens: 128000,
    max_output_tokens: 8192,
  },
  models: [],
  created_at: "2026-08-18T00:00:00Z",
  ...overrides,
});

test("协议测试优先使用供应商保存的上游协议能力", () => {
  expect(providerProtocolOptions(provider())).toEqual(["responses", "anthropic"]);
});

test("未覆盖协议时按供应商类型选择默认上游协议", () => {
  expect(providerProtocolOptions(provider({ provider_type: "anthropic", capabilities: {} }))).toEqual(["anthropic"]);
  expect(providerProtocolOptions(provider({ capabilities: {} }))).toEqual(["openai"]);
});
