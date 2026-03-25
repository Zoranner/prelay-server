export const DEFAULT_BASE_URLS: Record<string, string> = {
  // 订阅服务（Anthropic 兼容）
  zhipu_coding: 'https://open.bigmodel.cn/api/anthropic',
  minimax_token: 'https://api.minimaxi.com/anthropic',
  // 接口服务
  openai: 'https://api.openai.com',
  anthropic: 'https://api.anthropic.com',
  zhipu: 'https://open.bigmodel.cn/api/paas/v4',
  minimax: 'https://api.minimaxi.com/v1',
  // 其他自定义
  openai_compatible: '',
  anthropic_compatible: '',
};

export const PROVIDER_LABELS: Record<string, string> = {
  zhipu_coding: '智谱 Coding Plan',
  minimax_token: 'MiniMax Token Plan',
  openai: 'OpenAI',
  anthropic: 'Anthropic Claude',
  zhipu: '智谱 AI',
  minimax: 'MiniMax',
  openai_compatible: '自定义 OpenAI 兼容',
  anthropic_compatible: '自定义 Anthropic 兼容',
};

export const PROVIDER_BADGE_CLASSES: Record<string, string> = {
  zhipu_coding: 'bg-[#ddeafc] text-[#133d9e]',
  minimax_token: 'bg-[#ede8f5] text-[#4a2d7a]',
  openai: 'bg-[#edf4ee] text-[#3d6b45]',
  anthropic: 'bg-[#f5ece0] text-[#8b5230]',
  zhipu: 'bg-[#e8f0fd] text-[#1a4db5]',
  minimax: 'bg-[#f0edf8] text-[#5c3d8b]',
  openai_compatible: 'bg-stone-100 text-stone-600',
  anthropic_compatible: 'bg-stone-100 text-stone-500',
};

export function providerLabel(type: string): string {
  return PROVIDER_LABELS[type] ?? type;
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
