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

export const configApi = {
  getByToken: (token: string) => api.get<ProviderConfig>(`/configs/by-token/${token}`),
  create: (data: CreateConfigRequest) => api.post<ProviderConfig>('/configs', data),
  update: (id: string, data: UpdateConfigRequest) =>
    api.put<ProviderConfig>(`/configs/${id}`, data),
  delete: (id: string) => api.delete(`/configs/${id}`),
  regenerateToken: (id: string) => api.post<{ token: string }>(`/configs/${id}/regenerate-token`),
};
