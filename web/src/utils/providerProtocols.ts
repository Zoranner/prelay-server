import type { ModelCatalogCapabilities, ProviderProtocolBaseUrls } from '../api';
import {
  PROVIDER_UPSTREAM_PROTOCOL_ORDER,
  providerTemplateForProviderType,
  sortProviderProtocolValues,
  type ProviderUpstreamProtocol,
} from './providers';

export interface ProtocolOption {
  value: string;
  label: string;
}

type ProviderProtocolCapabilities = ModelCatalogCapabilities & {
  upstream_protocols?: ProviderUpstreamProtocol[];
};

const responsesProtocolOptions: ProtocolOption[] = [
  { value: 'responses', label: 'Responses' },
  { value: 'anthropic_messages', label: 'Anthropic Messages' },
];

const openAiCompatibleProtocolOptions: ProtocolOption[] = [
  { value: 'responses', label: 'Responses' },
  { value: 'chat_completions', label: 'Chat Completions' },
  { value: 'anthropic_messages', label: 'Anthropic Messages' },
];

const anthropicProviderTypes = new Set([
  'anthropic',
  'anthropic_compatible',
  'deepseek_anthropic',
  'qwen_anthropic',
  'zhipu_anthropic',
  'minimax_anthropic',
  'kimi_coding_anthropic',
  'zhipu_coding',
  'minimax_token',
  'bailian_coding_anthropic',
  'bailian_token_anthropic',
]);

export function isResponsesProvider(providerType: string): boolean {
  return ['openai', 'responses_compatible', 'qwen_responses', 'minimax_responses'].includes(
    providerType,
  );
}

export function isAnthropicProvider(providerType: string): boolean {
  return anthropicProviderTypes.has(providerType);
}

export function protocolLabel(providerType: string): string {
  if (isResponsesProvider(providerType)) {
    return 'Responses';
  }
  if (isAnthropicProvider(providerType)) {
    return 'Anthropic Messages';
  }
  return 'Chat Completions';
}

export function protocolOptionsForProvider(
  providerType: string,
  capabilities?: ModelCatalogCapabilities | null,
): ProtocolOption[] {
  const upstreamProtocols = upstreamProtocolValuesForProvider(providerType, capabilities);
  if (upstreamProtocols.length > 0) {
    return protocolOptionsForUpstreamProtocols(upstreamProtocols);
  }
  if (isResponsesProvider(providerType) || isAnthropicProvider(providerType)) {
    return responsesProtocolOptions;
  }
  return openAiCompatibleProtocolOptions;
}

export function defaultProtocolValuesForProvider(
  providerType: string,
  capabilities?: ModelCatalogCapabilities | null,
): string[] {
  return protocolOptionsForProvider(providerType, capabilities).map((option) => option.value);
}

export function upstreamProtocolValuesForProvider(
  providerType: string,
  capabilities?: ModelCatalogCapabilities | null,
): ProviderUpstreamProtocol[] {
  const customProtocols = customProtocolValuesFromCapabilities(capabilities);
  if (customProtocols.length > 0) {
    return customProtocols;
  }

  const template = providerTemplateForProviderType(providerType);
  if (template && !template.custom) {
    return sortProviderProtocolValues(template.variants.map((variant) => variant.protocol));
  }

  if (isResponsesProvider(providerType)) {
    return ['responses'];
  }
  if (isAnthropicProvider(providerType)) {
    return ['anthropic'];
  }
  return ['openai'];
}

export function upstreamProtocolOptionsForProvider(
  providerType: string,
  capabilities?: ModelCatalogCapabilities | null,
): ProtocolOption[] {
  return upstreamProtocolValuesForProvider(providerType, capabilities).map((protocol) => ({
    value: protocol,
    label: upstreamProtocolLabel(protocol),
  }));
}

export function protocolTagClass(protocol: string): string {
  if (protocol === 'responses') {
    return 'border-[#b7d8cf] bg-[#e8f4f0] text-[#176b5d]';
  }
  if (protocol === 'openai' || protocol === 'chat_completions') {
    return 'border-[#c7d6f4] bg-[#edf3ff] text-[#2f63d7]';
  }
  if (protocol === 'anthropic' || protocol === 'anthropic_messages') {
    return 'border-[#e6c8aa] bg-[#f5ece0] text-[#8b5230]';
  }
  return 'border-stone-200 bg-stone-100 text-stone-600';
}

export function customProtocolValuesFromCapabilities(
  capabilities?: ModelCatalogCapabilities | null,
): ProviderUpstreamProtocol[] {
  const protocols = (capabilities as ProviderProtocolCapabilities | null | undefined)
    ?.upstream_protocols;
  if (!Array.isArray(protocols)) {
    return [];
  }
  return sortProviderProtocolValues(protocols.filter(isProviderUpstreamProtocol));
}

export function normalizeProviderProtocolBaseUrls(
  baseUrls?: ProviderProtocolBaseUrls | null,
): Record<ProviderUpstreamProtocol, string> {
  return Object.fromEntries(
    PROVIDER_UPSTREAM_PROTOCOL_ORDER.map((protocol) => [
      protocol,
      (baseUrls?.[protocol] ?? '').trim(),
    ]),
  ) as Record<ProviderUpstreamProtocol, string>;
}

function upstreamProtocolLabel(protocol: ProviderUpstreamProtocol): string {
  if (protocol === 'responses') {
    return 'Responses';
  }
  if (protocol === 'openai') {
    return 'Chat Completions';
  }
  return 'Anthropic Messages';
}

function protocolOptionsForUpstreamProtocols(
  upstreamProtocols: ProviderUpstreamProtocol[],
): ProtocolOption[] {
  const values = new Set<string>();
  for (const protocol of upstreamProtocols) {
    for (const option of downstreamProtocolOptionsForUpstream(protocol)) {
      values.add(option.value);
    }
  }
  return openAiCompatibleProtocolOptions.filter((option) => values.has(option.value));
}

function downstreamProtocolOptionsForUpstream(
  protocol: ProviderUpstreamProtocol,
): ProtocolOption[] {
  if (protocol === 'openai') {
    return openAiCompatibleProtocolOptions;
  }
  return responsesProtocolOptions;
}

function isProviderUpstreamProtocol(value: string): value is ProviderUpstreamProtocol {
  return value === 'responses' || value === 'openai' || value === 'anthropic';
}
