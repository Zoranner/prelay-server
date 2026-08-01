import axios from 'axios';

const DEFAULT_MANAGEMENT_ERROR = '加载失败，请稍后重试。';

export function managementErrorMessage(error: unknown): string {
  if (!axios.isAxiosError(error)) {
    return DEFAULT_MANAGEMENT_ERROR;
  }
  if (error.response?.status === 401) {
    return '管理凭据无效或缺失，请检查 ADMIN_TOKEN。';
  }
  if (!error.response) {
    return '无法连接管理服务，请检查服务状态后重试。';
  }

  const data = error.response.data;
  if (
    data &&
    typeof data === 'object' &&
    'error' in data &&
    typeof data.error === 'string' &&
    data.error.trim()
  ) {
    return data.error;
  }
  return DEFAULT_MANAGEMENT_ERROR;
}
