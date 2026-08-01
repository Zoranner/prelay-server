import type { ProviderConfig, ProviderModelResponse } from '../api';

export interface SelectOption {
  label: string;
  value: string;
}

export function providerModelOptions(provider?: ProviderConfig | null): SelectOption[] {
  return providerModels(provider).map((model) => ({
    label: model.model_name,
    value: model.model_name,
  }));
}

export function providerOptionsForInterface(providers: ProviderConfig[]): SelectOption[] {
  return providers
    .filter((provider) => providerModels(provider).length > 0)
    .map((provider) => ({
      label: provider.name,
      value: provider.id,
    }));
}

export function providerModels(provider?: ProviderConfig | null): ProviderModelResponse[] {
  return provider?.models ?? [];
}

export function hasProviderModel(provider: ProviderConfig | undefined, modelName: string): boolean {
  return providerModels(provider).some((model) => model.model_name === modelName);
}

export function mergeDiscoveredProviderModels(
  currentModels: string[],
  discoveredModels: string[],
): string[] {
  return normalizeProviderModelNames([...currentModels, ...discoveredModels]);
}

export function normalizeProviderModelNames(modelNames: string[]): string[] {
  return Array.from(
    new Set(modelNames.map((modelName) => modelName.trim()).filter((modelName) => modelName)),
  ).sort((left, right) => left.localeCompare(right));
}
