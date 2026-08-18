import type { Provider, UpstreamProtocol } from "~/stores/relay";

const protocolValues: UpstreamProtocol[] = ["responses", "openai", "anthropic"];

const responseProviderTypes = new Set(["openai", "responses_compatible", "qwen_responses", "minimax_responses"]);
const anthropicProviderTypes = new Set([
  "anthropic",
  "anthropic_compatible",
  "deepseek_anthropic",
  "qwen_anthropic",
  "zhipu_anthropic",
  "minimax_anthropic",
  "kimi_coding_anthropic",
  "zai_coding_anthropic",
  "zhipu_coding",
  "minimax_token",
  "bailian_coding_anthropic",
  "bailian_token_anthropic",
]);

export function providerProtocolOptions(provider: Provider): UpstreamProtocol[] {
  const configured = provider.capabilities.upstream_protocols
    ?.filter((protocol): protocol is UpstreamProtocol => protocolValues.includes(protocol as UpstreamProtocol));
  if (configured?.length) return [...new Set(configured)];
  if (responseProviderTypes.has(provider.provider_type)) return ["responses"];
  if (anthropicProviderTypes.has(provider.provider_type)) return ["anthropic"];
  return ["openai"];
}
