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
  ollama_native: 'http://localhost:11434/api',
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
  ollama_native: 'Ollama 本地模型',
};

export const PROVIDER_DOT_CLASSES: Record<string, string> = {
  zhipu_coding: 'bg-[#4a7de8]',
  minimax_token: 'bg-[#7c5cbf]',
  openai: 'bg-[#4a9e5c]',
  anthropic: 'bg-[#c47830]',
  zhipu: 'bg-[#3d6fd4]',
  minimax: 'bg-[#8b5fc4]',
  openai_compatible: 'bg-stone-400',
  anthropic_compatible: 'bg-stone-300',
  ollama_native: 'bg-[#4f8f74]',
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
  ollama_native: 'bg-[#e4f2ec] text-[#2d6b50]',
};

export function providerLabel(type: string): string {
  return PROVIDER_LABELS[type] ?? type;
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

const STORAGE_KEY = 'provider-relay-tokens';

export interface StoredToken {
  token: string;
  name: string;
  providerType: string;
  createdAt: number;
}

export function getStoredTokens(): StoredToken[] {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) || '[]');
  } catch {
    return [];
  }
}

export function saveToken(token: StoredToken): void {
  const tokens = getStoredTokens();
  const existing = tokens.findIndex((t) => t.token === token.token);
  if (existing >= 0) {
    tokens.splice(existing, 1);
  }
  tokens.unshift(token);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(tokens.slice(0, 20)));
}

export function removeStoredToken(token: string): void {
  const tokens = getStoredTokens().filter((t) => t.token !== token);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(tokens));
}

export function updateStoredToken(
  token: string,
  updates: Partial<Pick<StoredToken, 'name' | 'providerType'>>,
): void {
  const tokens = getStoredTokens();
  const idx = tokens.findIndex((t) => t.token === token);
  if (idx >= 0) {
    tokens[idx] = { ...tokens[idx], ...updates };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(tokens));
  }
}

export function replaceStoredToken(oldToken: string, newEntry: StoredToken): void {
  const tokens = getStoredTokens().filter((t) => t.token !== oldToken);
  tokens.unshift(newEntry);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(tokens.slice(0, 20)));
}
