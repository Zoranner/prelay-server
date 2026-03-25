import { ref } from 'vue';
import { configApi, type ProviderConfig } from '../api/index';
import { DEFAULT_BASE_URLS, copyToClipboard } from '../utils/providers';

export function useManageKey() {
  const lookupToken = ref('');
  const looking = ref(false);
  const lookupError = ref('');
  const config = ref<ProviderConfig | null>(null);

  const editing = ref(false);
  const editForm = ref({ provider_type: '', name: '', base_url: '', api_key: '' });
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
      const payload: Record<string, string> = {
        name: editForm.value.name,
        provider_type: editForm.value.provider_type,
        base_url: editForm.value.base_url,
      };
      if (editForm.value.api_key.trim()) payload.api_key = editForm.value.api_key;
      const res = await configApi.update(config.value.id, payload);
      config.value = res.data;
      editing.value = false;
      actionMsg.value = { type: 'success', text: '配置已更新' };
    } catch {
      editError.value = '更新失败，请稍后重试';
    } finally {
      saving.value = false;
    }
  }

  async function regenerate() {
    if (!config.value) return;
    if (!confirm('刷新后，使用旧密钥的连接将立即失效。确认刷新？')) return;

    regenerating.value = true;
    actionMsg.value = null;
    try {
      const res = await configApi.regenerateToken(config.value.id);
      const newToken = res.data.token;
      config.value = { ...config.value, token: newToken };
      lookupToken.value = newToken;
      await copyToClipboard(newToken);
      actionMsg.value = {
        type: 'success',
        text: '密钥已刷新并复制到剪贴板，请更新你的 AI 工具配置',
      };
    } catch {
      actionMsg.value = { type: 'error', text: '刷新失败，请稍后重试' };
    } finally {
      regenerating.value = false;
    }
  }

  async function remove() {
    if (!config.value) return;
    if (!confirm(`确定要删除「${config.value.name}」的配置吗？此操作不可撤销。`)) return;

    deleting.value = true;
    actionMsg.value = null;
    try {
      await configApi.delete(config.value.id);
      config.value = null;
      lookupToken.value = '';
      actionMsg.value = { type: 'success', text: '配置已删除' };
    } catch {
      actionMsg.value = { type: 'error', text: '删除失败，请稍后重试' };
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
