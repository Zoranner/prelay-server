import { ref } from 'vue';
import { configApi } from '../api/index';
import { DEFAULT_BASE_URLS, copyToClipboard } from '../utils/providers';

export function useCreateKey() {
  const form = ref({
    provider_type: 'openai',
    name: '',
    base_url: DEFAULT_BASE_URLS.openai,
    api_key: '',
  });

  const creating = ref(false);
  const error = ref('');
  const createdToken = ref('');
  const copied = ref(false);

  function onProviderChange() {
    form.value.base_url = DEFAULT_BASE_URLS[form.value.provider_type] ?? '';
  }

  async function submit() {
    error.value = '';
    if (!form.value.api_key.trim()) {
      error.value = '请填写上游 API Key';
      return;
    }
    if (!form.value.base_url.trim()) {
      error.value = '请填写上游 Base URL';
      return;
    }

    creating.value = true;
    try {
      const res = await configApi.create({
        name: form.value.name.trim() || form.value.provider_type,
        provider_type: form.value.provider_type,
        base_url: form.value.base_url.trim(),
        api_key: form.value.api_key.trim(),
      });
      createdToken.value = res.data.token;
      form.value = {
        provider_type: 'openai',
        name: '',
        base_url: DEFAULT_BASE_URLS.openai,
        api_key: '',
      };
    } catch {
      error.value = '创建失败，请检查服务是否正常运行';
    } finally {
      creating.value = false;
    }
  }

  async function copyToken() {
    const ok = await copyToClipboard(createdToken.value);
    if (ok) {
      copied.value = true;
      setTimeout(() => {
        copied.value = false;
      }, 2000);
    }
  }

  return { form, creating, error, createdToken, copied, onProviderChange, submit, copyToken };
}
