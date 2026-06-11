import type { ModelCatalogCapabilities } from '../api';
import { BOOLEAN_CAPABILITIES } from './modelCapabilities';

export type BooleanOverride = 'default' | 'enabled' | 'disabled';

export interface CapabilityOverrideForm {
  boolean: Record<(typeof BOOLEAN_CAPABILITIES)[number]['key'], BooleanOverride>;
  max_context_tokens: string;
  max_output_tokens: string;
}

export function createCapabilityOverrideForm(
  capabilities?: ModelCatalogCapabilities | null,
): CapabilityOverrideForm {
  const boolean = BOOLEAN_CAPABILITIES.reduce(
    (acc, item) => {
      const value = capabilities?.[item.key];
      acc[item.key] = typeof value === 'boolean' ? (value ? 'enabled' : 'disabled') : 'default';
      return acc;
    },
    {} as CapabilityOverrideForm['boolean'],
  );

  return {
    boolean,
    max_context_tokens: tokenLimitToInput(capabilities?.max_context_tokens),
    max_output_tokens: tokenLimitToInput(capabilities?.max_output_tokens),
  };
}

export function capabilityOverridesFromForm(
  form: CapabilityOverrideForm,
): ModelCatalogCapabilities {
  const capabilities: ModelCatalogCapabilities = {};

  for (const item of BOOLEAN_CAPABILITIES) {
    const value = form.boolean[item.key];
    if (value === 'enabled') {
      capabilities[item.key] = true;
    } else if (value === 'disabled') {
      capabilities[item.key] = false;
    }
  }

  const maxContextTokens = tokenLimitFromInput(form.max_context_tokens);
  const maxOutputTokens = tokenLimitFromInput(form.max_output_tokens);
  if (maxContextTokens !== null) {
    capabilities.max_context_tokens = maxContextTokens;
  }
  if (maxOutputTokens !== null) {
    capabilities.max_output_tokens = maxOutputTokens;
  }

  return capabilities;
}

function tokenLimitToInput(value: number | null | undefined) {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? String(value) : '';
}

function tokenLimitFromInput(value: string) {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }

  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return null;
  }

  return Math.floor(parsed);
}
