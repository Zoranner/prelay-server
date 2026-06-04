import axios from 'axios';

const api = axios.create({ baseURL: '/api' });

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
  input_tokens: number | null;
  output_tokens: number | null;
  latency_ms: number | null;
}

export const configApi = {
  getByToken: (token: string) => api.get<ProviderConfig>(`/configs/by-token/${token}`),
  create: (data: CreateConfigRequest) => api.post<ProviderConfig>('/configs', data),
  update: (id: string, data: UpdateConfigRequest) =>
    api.put<ProviderConfig>(`/configs/${id}`, data),
  delete: (id: string) => api.delete(`/configs/${id}`),
  regenerateToken: (id: string) => api.post<{ token: string }>(`/configs/${id}/regenerate-token`),
};

export const statsApi = {
  getOverview: () => api.get<StatsOverview>('/stats/overview'),
  listRequests: (limit = 50) =>
    api.get<RequestLogSummary[]>('/stats/requests', { params: { limit } }),
};
