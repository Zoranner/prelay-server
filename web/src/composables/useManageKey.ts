import { ref } from 'vue';
import { configApi, type ProviderConfig } from '../api/index';
import {
  DEFAULT_BASE_URLS,
  copyToClipboard,
  removeStoredToken,
  updateStoredToken,
  replaceStoredToken,
} from '../utils/providers';
import {
  capabilityOverridesFromForm,
  createCapabilityOverrideForm,
} from '../utils/capabilityOverrides';

export function useManageKey() {
  const lookupToken = ref('');
  const looking = ref(false);
  const lookupError = ref('');
  const config = ref<ProviderConfig | null>(null);

  const editing = ref(false);
  const editForm = ref({ provider_type: '', name: '', base_url: '', api_key: '' });
  const editCapabilityForm = ref(createCapabilityOverrideForm());
  const editError = ref('');
  const saving = ref(false);

  const regenerating = ref(false);
  const deleting = ref(false);
  const actionMsg = ref<{ type: 'success' | 'error'; text: string } | null>(null);

  async function lookup() {
    const token = lookupToken.value.trim();
    if (!token) return;

    lookupError.value = '';
    config.value = null;
    editing.value = false;
    actionMsg.value = null;
    looking.value = true;
    try {
      const res = await configApi.getByToken(token);
      config.value = res.data;
    } catch {
      lookupError.value = '未找到对应配置，请确认密钥是否正确';
    } finally {
      looking.value = false;
    }
  }

  function startEdit() {
    if (!config.value) return;
    editForm.value = {
      provider_type: config.value.provider_type,
      name: config.value.name,
      base_url: config.value.base_url,
      api_key: '',
    };
    editCapabilityForm.value = createCapabilityOverrideForm(config.value.capabilities);
    editError.value = '';
    editing.value = true;
  }

  function cancelEdit() {
    editing.value = false;
  }

  function onEditProviderChange() {
    editForm.value.base_url = DEFAULT_BASE_URLS[editForm.value.provider_type] ?? '';
  }

  async function saveEdit() {
    if (!config.value) return;
    editError.value = '';
    saving.value = true;
    try {
      const payload: Record<string, unknown> = {
        name: editForm.value.name,
        provider_type: editForm.value.provider_type,
        base_url: editForm.value.base_url,
        capabilities: capabilityOverridesFromForm(editCapabilityForm.value),
      };
      if (editForm.value.api_key.trim()) payload.api_key = editForm.value.api_key;
      const res = await configApi.update(config.value.id, payload);
      config.value = res.data;
      updateStoredToken(lookupToken.value, {
        name: res.data.name,
        providerType: res.data.provider_type,
      });
      editing.value = false;
      actionMsg.value = { type: 'success', text: '密钥配置已更新' };
    } catch {
      editError.value = '密钥配置更新失败，请稍后重试';
    } finally {
      saving.value = false;
    }
  }

  async function regenerate() {
    if (!config.value) return;

    regenerating.value = true;
    actionMsg.value = null;
    try {
      const res = await configApi.regenerateToken(config.value.id);
      const oldToken = lookupToken.value;
      const newToken = res.data.token;
      replaceStoredToken(oldToken, {
        token: newToken,
        name: config.value.name,
        providerType: config.value.provider_type,
        createdAt: Date.now(),
      });
      config.value = { ...config.value, token: newToken };
      lookupToken.value = newToken;
      await copyToClipboard(newToken);
      actionMsg.value = {
        type: 'success',
        text: '密钥已刷新并复制到剪贴板',
      };
    } catch {
      actionMsg.value = { type: 'error', text: '密钥刷新失败，请稍后重试' };
    } finally {
      regenerating.value = false;
    }
  }

  async function remove() {
    if (!config.value) return;

    deleting.value = true;
    actionMsg.value = null;
    try {
      const tokenToDelete = config.value.token;
      await configApi.delete(config.value.id);
      removeStoredToken(tokenToDelete);
      config.value = null;
      lookupToken.value = '';
      actionMsg.value = { type: 'success', text: '密钥配置已删除' };
    } catch {
      actionMsg.value = { type: 'error', text: '密钥配置删除失败，请稍后重试' };
    } finally {
      deleting.value = false;
    }
  }

  return {
    lookupToken,
    looking,
    lookupError,
    config,
    editing,
    editForm,
    editCapabilityForm,
    editError,
    saving,
    regenerating,
    deleting,
    actionMsg,
    lookup,
    startEdit,
    cancelEdit,
    onEditProviderChange,
    saveEdit,
    regenerate,
    remove,
  };
}
