export const DEFAULT_BASE_URLS: Record<string, string> = {
  openai: 'https://api.openai.com',
  anthropic: 'https://api.anthropic.com',
  azure: 'https://YOUR-RESOURCE.openai.azure.com',
  custom: '',
};

export const PROVIDER_LABELS: Record<string, string> = {
  openai: 'OpenAI',
  anthropic: 'Anthropic',
  azure: 'Azure',
  custom: '自定义',
};

// Earthy, non-generic badge palette
export const PROVIDER_BADGE_CLASSES: Record<string, string> = {
  openai: 'bg-[#edf4ee] text-[#3d6b45]',
  anthropic: 'bg-[#f5ece0] text-[#8b5230]',
  azure: 'bg-[#e8edf5] text-[#3a5878]',
  custom: 'bg-stone-100 text-stone-600',
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
