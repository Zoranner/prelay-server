import axios from 'axios';

const api = axios.create({ baseURL: '/api' });
const protocolApi = axios.create();

api.interceptors.request.use((config) => {
  const token = readAdminToken();
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

export function readAdminToken(
  storage: Pick<Storage, 'getItem'> | null = availableLocalStorage(),
): string | null {
  try {
    return storage?.getItem('provider-relay-admin-token')?.trim() || null;
  } catch {
    return null;
  }
}

function availableLocalStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

export function withProtocolToken(token: string) {
  return {
    headers: {
      Authorization: `Bearer ${token}`,
    },
  };
}

export interface ProviderConfig {
  id: string;
  name: string;
  provider_type: string;
  base_url: string;
  api_key_masked: string;
  token: string;
  capabilities: ModelCatalogCapabilities;
  models: ProviderModelResponse[];
  created_at: string;
}

export interface ProviderModelResponse {
  id: string;
  provider_id: string;
  model_name: string;
  created_at: string;
}

export interface CreateConfigRequest {
  name: string;
  provider_type: string;
  base_url: string;
  api_key: string;
  capabilities?: ModelCatalogCapabilities;
  models: string[];
}

export interface UpdateConfigRequest {
  name?: string;
  provider_type?: string;
  base_url?: string;
  api_key?: string;
  capabilities?: ModelCatalogCapabilities;
  models?: string[];
}

export interface CreateInterfaceRequest {
  name: string;
  protocol?: string;
  models: InterfaceModelInput[];
}

export interface UpdateInterfaceRequest {
  name?: string;
  models?: InterfaceModelInput[];
}

export interface InterfaceModelInput {
  provider_id: string;
  upstream_model: string;
  model_name?: string;
}

export type CreateInterfaceModelRequest = InterfaceModelInput;

export interface CreateProviderModelRequest {
  model_name: string;
}

export interface DiscoverProviderModelsRequest {
  provider_type: string;
  base_url: string;
  api_key: string;
}

export interface DiscoverProviderModelsResponse {
  models: string[];
}

export type ProviderUpstreamProtocol = 'responses' | 'openai' | 'anthropic';

export interface ProviderProtocolBaseUrls {
  responses?: string | null;
  openai?: string | null;
  anthropic?: string | null;
}

export interface TestProviderProtocolRequest {
  provider_type: string;
  protocol: ProviderUpstreamProtocol;
  base_url: string;
  api_key?: string;
  model?: string;
}

export interface TestProviderProtocolResponse {
  ok: boolean;
  protocol: ProviderUpstreamProtocol;
  latency_ms: number;
  first_token_ms?: number | null;
  error?: string | null;
}

export interface PingProviderResponse {
  ok: boolean;
  latency_ms: number;
  error?: string | null;
}

export interface InterfaceModelResponse {
  id: string;
  interface_id: string;
  model_name: string;
  provider_id: string;
  upstream_model: string;
  created_at: string;
}

export interface InterfaceResponse {
  id: string;
  name: string;
  protocol: string;
  token: string;
  models: InterfaceModelResponse[];
  created_at: string;
}

export interface ModelCatalogEntry {
  id: string;
  object: 'model';
  entry_type: 'provider' | 'alias';
  owned_by: string;
  provider_id: string;
  provider_name: string;
  upstream_protocol: string;
  upstream_model: string;
  downstream_protocols: string[];
  capabilities: ModelCatalogCapabilities;
}

export interface ModelCatalogCapabilities {
  upstream_protocols?: ProviderUpstreamProtocol[];
  protocol_base_urls?: ProviderProtocolBaseUrls;
  tool_calls?: boolean;
  reasoning?: boolean;
  tool_choice?: boolean;
  parallel_tool_calls?: boolean;
  system_messages?: boolean;
  structured_outputs?: boolean;
  streaming_usage?: boolean;
  max_context_tokens?: number | null;
  max_output_tokens?: number | null;
}

export interface ModelCatalogResponse {
  object: 'list';
  data: ModelCatalogEntry[];
}

export interface StatsOverview {
  total_requests: number;
  successful_requests: number;
  failed_requests: number;
  input_tokens: number;
  output_tokens: number;
}

export interface RequestLogSummary {
  id: string;
  created_at: string;
  protocol_in: string | null;
  protocol_upstream: string | null;
  provider_name: string | null;
  model_requested: string | null;
  status: string;
  http_status: number | null;
  error_code: string | null;
  error_message: string | null;
  input_tokens: number | null;
  output_tokens: number | null;
  latency_ms: number | null;
  upstream_request_id: string | null;
  metadata_json: string | null;
}

export interface RequestMetadata {
  schema?: string | null;
  bridge?: RequestMetadataBridge | null;
  diagnostics?: BridgeDiagnostic[] | null;
  stream?: RequestStreamMetadata | null;
  upstream?: RequestUpstreamMetadata | null;
}

export interface RequestMetadataBridge {
  protocol_in?: string | null;
  protocol_out?: string | null;
  protocol_upstream?: string | null;
  model_requested?: string | null;
  model_upstream?: string | null;
}

export interface BridgeDiagnostic {
  phase?: string | null;
  protocol?: string | null;
  path?: string | null;
  action?: string | null;
  severity?: string | null;
  code?: string | null;
  message?: string | null;
  original_kind?: string | null;
}

export interface RequestStreamMetadata {
  empty?: boolean | null;
  completed?: boolean | null;
  final_usage_seen?: boolean | null;
  stream_error?: string | null;
}

export interface RequestUpstreamMetadata {
  request_id?: string | null;
  error_body_excerpt?: string | null;
}

export interface ModelStatsSummary {
  model_requested: string | null;
  total_requests: number;
  successful_requests: number;
  failed_requests: number;
  input_tokens: number;
  output_tokens: number;
  estimated_cost: number | null;
  average_latency_ms: number | null;
}

export interface ProviderStatsSummary {
  provider_id: string | null;
  provider_name: string | null;
  total_requests: number;
  successful_requests: number;
  failed_requests: number;
  input_tokens: number;
  output_tokens: number;
  estimated_cost: number | null;
  average_latency_ms: number | null;
  average_first_token_ms: number | null;
}

export const configApi = {
  list: () => api.get<ProviderConfig[]>('/configs'),
  getByToken: (token: string) => api.get<ProviderConfig>(`/configs/by-token/${token}`),
  create: (data: CreateConfigRequest) => api.post<ProviderConfig>('/configs', data),
  update: (id: string, data: UpdateConfigRequest) =>
    api.put<ProviderConfig>(`/configs/${id}`, data),
  delete: (id: string) => api.delete(`/configs/${id}`),
  regenerateToken: (id: string) => api.post<{ token: string }>(`/configs/${id}/regenerate-token`),
  discoverModels: (data: DiscoverProviderModelsRequest) =>
    api.post<DiscoverProviderModelsResponse>('/configs/discover-models', data),
  discoverSavedModels: (providerId: string) =>
    api.post<DiscoverProviderModelsResponse>(`/configs/${providerId}/discover-models`),
  ping: (providerId: string) => api.post<PingProviderResponse>(`/configs/${providerId}/ping`),
  testProtocol: (data: TestProviderProtocolRequest) =>
    api.post<TestProviderProtocolResponse>('/configs/test-protocol', data),
  testSavedProtocol: (providerId: string, data: TestProviderProtocolRequest) =>
    api.post<TestProviderProtocolResponse>(`/configs/${providerId}/test-protocol`, data),
  createProviderModel: (providerId: string, data: CreateProviderModelRequest) =>
    api.post<ProviderModelResponse>(`/configs/${providerId}/models`, data),
  deleteProviderModel: (providerId: string, modelId: string) =>
    api.delete(`/configs/${providerId}/models/${modelId}`),
  listInterfaces: () => api.get<InterfaceResponse[]>('/interfaces'),
  createInterface: (data: CreateInterfaceRequest) =>
    api.post<InterfaceResponse>('/interfaces', data),
  updateInterface: (id: string, data: UpdateInterfaceRequest) =>
    api.put<InterfaceResponse>(`/interfaces/${id}`, data),
  deleteInterface: (id: string) => api.delete(`/interfaces/${id}`),
  regenerateInterfaceToken: (id: string) =>
    api.post<{ token: string }>(`/interfaces/${id}/regenerate-token`),
  createInterfaceModel: (interfaceId: string, data: CreateInterfaceModelRequest) =>
    api.post<InterfaceModelResponse>(`/interfaces/${interfaceId}/models`, data),
  deleteInterfaceModel: (interfaceId: string, modelId: string) =>
    api.delete(`/interfaces/${interfaceId}/models/${modelId}`),
};

export const modelsApi = {
  list: (token: string) =>
    protocolApi.get<ModelCatalogResponse>('/v1/models', withProtocolToken(token)),
};

export const statsApi = {
  getOverview: () => api.get<StatsOverview>('/stats/overview'),
  listRequests: (limit = 50) =>
    api.get<RequestLogSummary[]>('/stats/requests', { params: { limit } }),
  listModels: () => api.get<ModelStatsSummary[]>('/stats/models'),
  listProviders: () => api.get<ProviderStatsSummary[]>('/stats/providers'),
};
