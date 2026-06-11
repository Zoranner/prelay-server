import axios from 'axios';

const api = axios.create({ baseURL: '/api' });
const protocolApi = axios.create();

api.interceptors.request.use((config) => {
  const token = localStorage.getItem('provider-relay-admin-token')?.trim();
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

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
  created_at: string;
}

export interface CreateConfigRequest {
  name: string;
  provider_type: string;
  base_url: string;
  api_key: string;
}

export interface UpdateConfigRequest {
  name?: string;
  provider_type?: string;
  base_url?: string;
  api_key?: string;
}

export interface CreateModelAliasRequest {
  alias: string;
  provider_id: string;
  upstream_model: string;
  downstream_protocols: string[];
}

export interface ModelAliasResponse {
  alias: string;
  provider_id: string;
  upstream_model: string;
  downstream_protocols: string[];
}

export interface ModelCatalogEntry {
  id: string;
  object: 'model';
  owned_by: string;
  provider_id: string;
  provider_name: string;
  upstream_protocol: string;
  upstream_model: string;
  downstream_protocols: string[];
  capabilities: {
    tool_calls: boolean;
  };
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
  listModelAliases: () => api.get<ModelAliasResponse[]>('/model-aliases'),
  createModelAlias: (data: CreateModelAliasRequest) =>
    api.post<ModelAliasResponse>('/model-aliases', data),
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
