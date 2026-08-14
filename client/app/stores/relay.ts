export interface BootstrapState {
  relay_url?: string;
  machine_id: string;
  account_sid: string;
  username: string;
  has_device_credential: boolean;
}

export interface ProviderModel {
  id: string;
  provider_id: string;
  model_name: string;
  created_at: string;
}

export interface Provider {
  id: string;
  name: string;
  provider_type: string;
  base_url: string;
  api_key_masked: string;
  models: ProviderModel[];
  created_at: string;
}

export interface InterfaceModel {
  id?: string;
  interface_id?: string;
  model_name: string;
  provider_id: string;
  upstream_model: string;
  created_at?: string;
}

export interface RelayInterface {
  id: string;
  name: string;
  protocol: string;
  token: string;
  models: InterfaceModel[];
  created_at: string;
}

export interface StatsOverview {
  total_requests: number;
  successful_requests: number;
  failed_requests: number;
  input_tokens: number;
  output_tokens: number;
}

export interface ModelStats {
  model_requested: string | null;
  total_requests: number;
  successful_requests: number;
  failed_requests: number;
  input_tokens: number;
  output_tokens: number;
  estimated_cost: number | null;
  average_latency_ms: number | null;
}

export interface ProviderStats extends ModelStats {
  provider_id: string | null;
  provider_name: string | null;
  average_first_token_ms: number | null;
}

export interface RequestLog {
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

export function useRelayStore() {
  const bootstrap = useState<BootstrapState | null>("relay-bootstrap", () => null);

  function setBootstrap(value: BootstrapState) {
    bootstrap.value = value;
  }

  return { bootstrap, setBootstrap };
}
