import type { ModelCatalogCapabilities, ModelCatalogEntry } from '../api';

export interface BooleanCapabilityItem {
  key: keyof Pick<
    ModelCatalogCapabilities,
    | 'tool_calls'
    | 'reasoning'
    | 'tool_choice'
    | 'parallel_tool_calls'
    | 'system_messages'
    | 'structured_outputs'
    | 'streaming_usage'
  >;
  label: string;
}

export interface CapabilityChip extends BooleanCapabilityItem {
  enabled: boolean;
}

export const BOOLEAN_CAPABILITIES: BooleanCapabilityItem[] = [
  { key: 'tool_calls', label: '工具调用' },
  { key: 'reasoning', label: 'Reasoning' },
  { key: 'tool_choice', label: 'Tool Choice' },
  { key: 'parallel_tool_calls', label: '并行工具' },
  { key: 'system_messages', label: '系统消息' },
  { key: 'structured_outputs', label: '结构化输出' },
  { key: 'streaming_usage', label: '流式 Usage' },
];

export function capabilityChips(model: ModelCatalogEntry): CapabilityChip[] {
  return BOOLEAN_CAPABILITIES.map((item) => ({
    ...item,
    enabled: Boolean(model.capabilities?.[item.key]),
  }));
}

export function enabledCapabilityChips(model: ModelCatalogEntry): CapabilityChip[] {
  return capabilityChips(model).filter((item) => item.enabled);
}

export function enabledCapabilitySummary(model: ModelCatalogEntry): string {
  const labels = enabledCapabilityChips(model).map((item) => item.label);
  return labels.length > 0 ? labels.join('、') : '无能力声明';
}

export function formatTokenLimit(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0) {
    return '未声明';
  }

  if (value >= 1000) {
    return `${new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 1 }).format(value / 1000)}k`;
  }

  return new Intl.NumberFormat('zh-CN').format(value);
}

export function supportsCapability(
  model: ModelCatalogEntry,
  key: BooleanCapabilityItem['key'],
): boolean {
  return Boolean(model.capabilities?.[key]);
}
