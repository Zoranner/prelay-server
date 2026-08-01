export interface ProviderOption {
  value: string;
  label: string;
}

export interface ProviderGroup {
  label: string;
  options: ProviderOption[];
}

export type ProviderUpstreamProtocol = 'responses' | 'openai' | 'anthropic';

export const PROVIDER_UPSTREAM_PROTOCOL_ORDER: ProviderUpstreamProtocol[] = [
  'openai',
  'anthropic',
  'responses',
];

const PROVIDER_UPSTREAM_PROTOCOL_RANK = new Map(
  PROVIDER_UPSTREAM_PROTOCOL_ORDER.map((protocol, index) => [protocol, index]),
);

export interface ProviderProtocolVariant {
  protocol: ProviderUpstreamProtocol;
  label: string;
  providerType: string;
  baseUrl: string;
}

export interface ProviderTemplate {
  value: string;
  label: string;
  providerType: string;
  baseUrl: string;
  variants: ProviderProtocolVariant[];
  custom: boolean;
}

export interface ProviderTemplateGroup {
  label: string;
  options: ProviderTemplate[];
}

export const PROVIDER_GROUPS: ProviderGroup[] = [
  {
    label: '套餐服务',
    options: [
      { value: 'kimi_coding_anthropic', label: 'Kimi Code' },
      { value: 'zhipu_coding', label: 'GLM Coding Plan' },
      { value: 'minimax_token', label: 'MiniMax Token Plan' },
    ],
  },
  {
    label: 'API 服务',
    options: [
      { value: 'kimi', label: 'Kimi' },
      { value: 'deepseek', label: 'DeepSeek' },
      { value: 'qwen', label: '阿里云百炼' },
      { value: 'zhipu', label: '智谱AI开放平台' },
      { value: 'minimax', label: 'MiniMax' },
    ],
  },
  {
    label: '其他服务',
    options: [{ value: 'openai_compatible', label: '自定义' }],
  },
];

export const DEFAULT_BASE_URLS: Record<string, string> = {
  // 套餐服务
  kimi_coding: 'https://api.kimi.com/coding/v1',
  kimi_coding_anthropic: 'https://api.kimi.com/coding',
  zhipu_coding_openai: 'https://open.bigmodel.cn/api/coding/paas/v4',
  zhipu_coding: 'https://open.bigmodel.cn/api/anthropic',
  minimax_token_openai: 'https://api.minimax.io/v1',
  minimax_token: 'https://api.minimax.io/anthropic',
  bailian_coding_openai: 'https://coding.dashscope.aliyuncs.com/v1',
  bailian_coding_anthropic: 'https://coding.dashscope.aliyuncs.com/apps/anthropic',
  bailian_token_openai: 'https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1',
  bailian_token_anthropic: 'https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic',
  // 接口服务
  openai: 'https://api.openai.com/v1',
  anthropic: 'https://api.anthropic.com/v1',
  kimi: 'https://api.moonshot.ai/v1',
  deepseek: 'https://api.deepseek.com',
  deepseek_anthropic: 'https://api.deepseek.com/anthropic',
  qwen: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
  qwen_responses: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
  qwen_anthropic: 'https://dashscope.aliyuncs.com/apps/anthropic',
  zhipu: 'https://open.bigmodel.cn/api/paas/v4',
  zhipu_anthropic: 'https://open.bigmodel.cn/api/anthropic',
  minimax: 'https://api.minimaxi.com/v1',
  minimax_responses: 'https://api.minimaxi.com/v1',
  minimax_anthropic: 'https://api.minimaxi.com/anthropic',
  // 其他自定义
  responses_compatible: '',
  openai_compatible: '',
  anthropic_compatible: '',
};

export const PACKAGE_PROVIDER_TEMPLATE_GROUP: ProviderTemplateGroup = {
  label: '套餐服务',
  options: [
    template('kimi_code', 'Kimi Code', [
      variant('anthropic', 'Anthropic Messages', 'kimi_coding_anthropic'),
      variant('openai', 'Chat Completions', 'kimi_coding'),
    ]),
    template('bigmodel_coding_plan', 'GLM Coding Plan', [
      variant('openai', 'Chat Completions', 'zhipu_coding_openai'),
      variant('anthropic', 'Anthropic Messages', 'zhipu_coding'),
    ]),
    template('minimax_token_plan', 'MiniMax Token Plan', [
      variant('openai', 'Chat Completions', 'minimax_token_openai'),
      variant('anthropic', 'Anthropic Messages', 'minimax_token'),
    ]),
  ],
};

export const API_PROVIDER_TEMPLATE_GROUP: ProviderTemplateGroup = {
  label: 'API 服务',
  options: [
    template('kimi', 'Kimi', [variant('openai', 'Chat Completions', 'kimi')]),
    template('deepseek', 'DeepSeek', [
      variant('openai', 'Chat Completions', 'deepseek'),
      variant('anthropic', 'Anthropic Messages', 'deepseek_anthropic'),
    ]),
    template(
      'bailian',
      '阿里云百炼',
      [
        variant('responses', 'Responses', 'qwen_responses'),
        variant('openai', 'Chat Completions', 'qwen'),
        variant('anthropic', 'Anthropic Messages', 'qwen_anthropic'),
      ],
      { providerType: 'qwen', baseUrl: DEFAULT_BASE_URLS.qwen },
    ),
    template('bigmodel', '智谱AI开放平台', [
      variant('openai', 'Chat Completions', 'zhipu'),
      variant('anthropic', 'Anthropic Messages', 'zhipu_anthropic'),
    ]),
    template(
      'minimax',
      'MiniMax',
      [
        variant('responses', 'Responses', 'minimax_responses'),
        variant('openai', 'Chat Completions', 'minimax'),
        variant('anthropic', 'Anthropic Messages', 'minimax_anthropic'),
      ],
      { providerType: 'minimax', baseUrl: DEFAULT_BASE_URLS.minimax },
    ),
  ],
};

export const CUSTOM_PROVIDER_TEMPLATE_GROUP: ProviderTemplateGroup = {
  label: '其他服务',
  options: [
    template(
      'custom',
      '自定义',
      [
        variant('responses', 'Responses', 'responses_compatible'),
        variant('openai', 'Chat Completions', 'openai_compatible'),
        variant('anthropic', 'Anthropic Messages', 'anthropic_compatible'),
      ],
      {
        custom: true,
        providerType: 'openai_compatible',
        baseUrl: DEFAULT_BASE_URLS.openai_compatible,
      },
    ),
  ],
};

export const PROVIDER_TEMPLATE_GROUPS: ProviderTemplateGroup[] = [
  PACKAGE_PROVIDER_TEMPLATE_GROUP,
  API_PROVIDER_TEMPLATE_GROUP,
  CUSTOM_PROVIDER_TEMPLATE_GROUP,
];

export const PROVIDER_LABELS: Record<string, string> = {
  kimi: 'Kimi',
  kimi_coding: 'Kimi Code',
  kimi_coding_anthropic: 'Kimi Code',
  deepseek: 'DeepSeek',
  deepseek_anthropic: 'DeepSeek',
  qwen: '阿里云百炼',
  qwen_responses: '阿里云百炼',
  qwen_anthropic: '阿里云百炼',
  bailian_coding_openai: '百炼 Coding Plan',
  bailian_coding_anthropic: '百炼 Coding Plan',
  bailian_token_openai: '百炼 Token Plan',
  bailian_token_anthropic: '百炼 Token Plan',
  zhipu: '智谱AI开放平台',
  zhipu_anthropic: '智谱AI开放平台',
  zhipu_coding_openai: 'GLM Coding Plan',
  zhipu_coding: 'GLM Coding Plan',
  minimax: 'MiniMax',
  minimax_responses: 'MiniMax',
  minimax_anthropic: 'MiniMax',
  minimax_token_openai: 'MiniMax Token Plan',
  minimax_token: 'MiniMax Token Plan',
};

export const PROVIDER_DOT_CLASSES: Record<string, string> = {
  kimi_coding: 'bg-[#2f8f7c]',
  kimi_coding_anthropic: 'bg-[#176b5d]',
  zhipu_coding_openai: 'bg-[#5c82e6]',
  zhipu_coding: 'bg-[#4a7de8]',
  minimax_token_openai: 'bg-[#8a69cf]',
  minimax_token: 'bg-[#7c5cbf]',
  bailian_coding_openai: 'bg-[#2c8fbf]',
  bailian_coding_anthropic: 'bg-[#247a9e]',
  bailian_token_openai: 'bg-[#3f9b8f]',
  bailian_token_anthropic: 'bg-[#2f7f75]',
  openai: 'bg-[#4a9e5c]',
  anthropic: 'bg-[#c47830]',
  kimi: 'bg-[#1f8a70]',
  deepseek: 'bg-[#2f63d7]',
  deepseek_anthropic: 'bg-[#214fa8]',
  qwen: 'bg-[#2c8fbf]',
  qwen_responses: 'bg-[#3a9fd1]',
  qwen_anthropic: 'bg-[#247a9e]',
  zhipu: 'bg-[#3d6fd4]',
  zhipu_anthropic: 'bg-[#2f5eb8]',
  minimax: 'bg-[#8b5fc4]',
  minimax_responses: 'bg-[#9670cf]',
  minimax_anthropic: 'bg-[#7750af]',
  responses_compatible: 'bg-stone-500',
  openai_compatible: 'bg-stone-400',
  anthropic_compatible: 'bg-stone-300',
};

export const PROVIDER_BADGE_CLASSES: Record<string, string> = {
  kimi_coding: 'bg-[#e4f4ef] text-[#176b5d]',
  kimi_coding_anthropic: 'bg-[#e4f4ef] text-[#0f5a4c]',
  zhipu_coding_openai: 'bg-[#ddeafc] text-[#173f9c]',
  zhipu_coding: 'bg-[#ddeafc] text-[#133d9e]',
  minimax_token_openai: 'bg-[#ede8f5] text-[#56348a]',
  minimax_token: 'bg-[#ede8f5] text-[#4a2d7a]',
  bailian_coding_openai: 'bg-[#e7f3f8] text-[#236b8f]',
  bailian_coding_anthropic: 'bg-[#e7f3f8] text-[#1d617d]',
  bailian_token_openai: 'bg-[#e7f4ec] text-[#28764b]',
  bailian_token_anthropic: 'bg-[#e7f4ec] text-[#1f665d]',
  openai: 'bg-[#edf4ee] text-[#3d6b45]',
  anthropic: 'bg-[#f5ece0] text-[#8b5230]',
  kimi: 'bg-[#e4f4ef] text-[#176b5d]',
  deepseek: 'bg-[#e9effc] text-[#2f63d7]',
  deepseek_anthropic: 'bg-[#e9effc] text-[#214fa8]',
  qwen: 'bg-[#e7f3f8] text-[#236b8f]',
  qwen_responses: 'bg-[#e7f3f8] text-[#236b8f]',
  qwen_anthropic: 'bg-[#e7f3f8] text-[#1d617d]',
  zhipu: 'bg-[#e8f0fd] text-[#1a4db5]',
  zhipu_anthropic: 'bg-[#e8f0fd] text-[#17449e]',
  minimax: 'bg-[#f0edf8] text-[#5c3d8b]',
  minimax_responses: 'bg-[#f0edf8] text-[#5c3d8b]',
  minimax_anthropic: 'bg-[#f0edf8] text-[#50347d]',
  responses_compatible: 'bg-stone-100 text-stone-700',
  openai_compatible: 'bg-stone-100 text-stone-600',
  anthropic_compatible: 'bg-stone-100 text-stone-500',
};

export function providerLabel(type: string): string {
  return PROVIDER_LABELS[type] ?? type;
}

export function providerTemplateForProviderType(
  providerType: string,
): ProviderTemplate | undefined {
  return PROVIDER_TEMPLATE_GROUPS.flatMap((group) => group.options).find((provider) =>
    provider.variants.some((variant) => variant.providerType === providerType),
  );
}

export function providerTemplateByValue(templateValue: string): ProviderTemplate | undefined {
  return PROVIDER_TEMPLATE_GROUPS.flatMap((group) => group.options).find(
    (provider) => provider.value === templateValue,
  );
}

export function providerProtocolValuesForTemplate(
  templateValue: string,
): ProviderUpstreamProtocol[] {
  return sortProviderProtocolValues(
    providerTemplateByValue(templateValue)?.variants.map((variant) => variant.protocol) ?? [],
  );
}

export function sortProviderProtocolValues(
  protocols: ProviderUpstreamProtocol[],
): ProviderUpstreamProtocol[] {
  return [...new Set(protocols)].sort(
    (left, right) =>
      (PROVIDER_UPSTREAM_PROTOCOL_RANK.get(left) ?? Number.MAX_SAFE_INTEGER) -
      (PROVIDER_UPSTREAM_PROTOCOL_RANK.get(right) ?? Number.MAX_SAFE_INTEGER),
  );
}

export function sortProviderProtocolVariants<T extends { protocol: ProviderUpstreamProtocol }>(
  variants: readonly T[],
): T[] {
  return [...variants].sort(
    (left, right) =>
      (PROVIDER_UPSTREAM_PROTOCOL_RANK.get(left.protocol) ?? Number.MAX_SAFE_INTEGER) -
      (PROVIDER_UPSTREAM_PROTOCOL_RANK.get(right.protocol) ?? Number.MAX_SAFE_INTEGER),
  );
}

export function providerVariantForProviderType(
  providerType: string,
): ProviderProtocolVariant | undefined {
  return PROVIDER_TEMPLATE_GROUPS.flatMap((group) => group.options)
    .flatMap((provider) => provider.variants)
    .find((variant) => variant.providerType === providerType);
}

export function providerVariantForTemplateProtocol(
  templateValue: string,
  protocol: ProviderUpstreamProtocol,
): ProviderProtocolVariant | undefined {
  return PROVIDER_TEMPLATE_GROUPS.flatMap((group) => group.options)
    .find((provider) => provider.value === templateValue)
    ?.variants.find((variant) => variant.protocol === protocol);
}

export function providerTemplateLabel(templateValue: string): string {
  return (
    PROVIDER_TEMPLATE_GROUPS.flatMap((group) => group.options).find(
      (provider) => provider.value === templateValue,
    )?.label ?? templateValue
  );
}

function template(
  value: string,
  label: string,
  variants: ProviderProtocolVariant[],
  options: { custom?: boolean; providerType?: string; baseUrl?: string } = {},
): ProviderTemplate {
  return {
    value,
    label,
    providerType: options.providerType ?? variants[0]?.providerType ?? value,
    baseUrl: options.baseUrl ?? variants[0]?.baseUrl ?? '',
    variants,
    custom: options.custom ?? false,
  };
}

function variant(
  protocol: ProviderUpstreamProtocol,
  label: string,
  providerType: string,
): ProviderProtocolVariant {
  return {
    protocol,
    label,
    providerType,
    baseUrl: DEFAULT_BASE_URLS[providerType] ?? '',
  };
}

export function providerDotClass(type: string): string {
  return PROVIDER_DOT_CLASSES[type] ?? 'bg-stone-300';
}

export function providerBadgeClass(type: string): string {
  return PROVIDER_BADGE_CLASSES[type] ?? 'bg-stone-100 text-stone-600';
}

export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

export function clearStoredProviderTokens(
  storage: Pick<Storage, 'removeItem'> = localStorage,
): void {
  storage.removeItem('provider-relay-tokens');
}
