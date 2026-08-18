import type { Provider, UpstreamProtocol } from "~/stores/relay";

const protocolValues: UpstreamProtocol[] = ["responses", "openai", "anthropic"];

export function providerProtocolOptions(provider: Provider): UpstreamProtocol[] {
  return [...new Set(provider.upstream_protocols)].filter((protocol): protocol is UpstreamProtocol =>
    protocolValues.includes(protocol as UpstreamProtocol),
  );
}
