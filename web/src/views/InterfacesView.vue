<template>
  <PageShell>
    <PageHeader title="接口" description="每个接口对应一个 API Token，接口内维护可用模型。">
      <template #actions>
        <Button variant="secondary" size="sm" :disabled="loading" @click="loadData">
          {{ loading ? '刷新中...' : '刷新' }}
        </Button>
        <Button size="sm" :disabled="saving" @click="openCreateDrawer"> 新建接口 </Button>
      </template>
    </PageHeader>

    <div
      v-if="actionError"
      role="alert"
      class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700"
    >
      {{ actionError }}
    </div>

    <SurfacePanel v-if="loading">
      <div class="flex min-h-40 items-center justify-center gap-2 text-sm text-stone-500">
        <LoaderCircle class="h-4 w-4 animate-spin" aria-hidden="true" />
        正在加载接口...
      </div>
    </SurfacePanel>

    <SurfacePanel v-else-if="loadError">
      <div
        role="alert"
        class="flex min-h-40 flex-col items-center justify-center gap-4 px-6 py-10 text-center"
      >
        <p class="text-sm text-red-600">{{ loadError }}</p>
        <Button variant="secondary" size="sm" @click="loadData">
          <RefreshCw class="h-4 w-4" aria-hidden="true" />
          重试
        </Button>
      </div>
    </SurfacePanel>

    <SurfacePanel v-else-if="filteredInterfaces.length === 0">
      <div class="flex min-h-40 items-center justify-center px-6 py-10 text-sm text-stone-400">
        暂无接口
      </div>
    </SurfacePanel>

    <SurfacePanel v-else>
      <DataTableShell table-class="min-w-[48rem]">
        <colgroup>
          <!-- 接口名称列自适应 -->
          <col />
          <col class="w-48" />
          <col class="w-20" />
          <col class="w-24" />
        </colgroup>
        <thead class="sticky top-0 z-10 bg-stone-50 text-xs font-semibold text-stone-500">
          <tr>
            <th class="px-5 py-3 text-left">接口</th>
            <th class="px-5 py-3 text-left">API Token</th>
            <th class="px-5 py-3 text-left">模型</th>
            <th class="px-5 py-3 text-right">操作</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-stone-100">
          <template v-for="item in filteredInterfaces" :key="item.id">
            <tr
              class="selectable-row"
              :class="expandedInterfaceId === item.id ? 'selected-row' : ''"
              tabindex="0"
              :aria-selected="expandedInterfaceId === item.id"
              @click="selectInterface(item.id)"
              @keydown.enter.prevent="selectInterface(item.id)"
              @keydown.space.prevent="selectInterface(item.id)"
            >
              <td class="px-5 py-4">
                <div class="font-semibold text-stone-800">
                  {{ item.name }}
                </div>
                <div class="mt-1 text-xs text-stone-400">{{ item.id }}</div>
              </td>
              <td class="px-5 py-4 whitespace-nowrap">
                <div class="flex items-center gap-2 font-mono text-xs text-stone-700">
                  <span>{{ maskToken(item.token) }}</span>
                  <CopyButton
                    label="复制 API Token"
                    size="sm"
                    @click.stop="copyInterfaceToken(item.token)"
                  />
                </div>
              </td>
              <td class="px-5 py-4 text-stone-600">{{ item.models.length }} 个</td>
              <td class="px-5 py-4">
                <div class="flex justify-end gap-2">
                  <IconButton
                    label="编辑接口"
                    :disabled="saving"
                    @click.stop="openEditDrawer(item)"
                  >
                    <Pencil class="h-4 w-4" aria-hidden="true" />
                  </IconButton>
                  <IconButton
                    label="删除接口"
                    variant="danger"
                    :disabled="isInterfaceDeleteBusy(item.id)"
                    @click.stop="deleteInterface(item)"
                  >
                    <Trash2 class="h-4 w-4" aria-hidden="true" />
                  </IconButton>
                </div>
              </td>
            </tr>

            <tr v-if="expandedInterfaceId === item.id" class="expanded-row">
              <td colspan="4" class="px-5 py-4">
                <div class="flex flex-wrap gap-2">
                  <button
                    v-for="model in item.models"
                    :key="model.id"
                    type="button"
                    class="copy-tag"
                    aria-label="复制模型名"
                    :title="`复制模型名：${model.model_name}`"
                    @click="copyInterfaceModelName(model.model_name)"
                  >
                    <span class="max-w-[12rem] truncate font-mono text-xs text-stone-800">
                      {{ model.model_name }}
                    </span>
                    <Copy class="h-3.5 w-3.5 shrink-0 text-stone-400" aria-hidden="true" />
                  </button>
                  <div v-if="item.models.length === 0" class="text-sm text-stone-400">
                    暂无模型。
                  </div>
                </div>
              </td>
            </tr>
          </template>
        </tbody>
      </DataTableShell>
    </SurfacePanel>

    <DrawerPanel
      :open="drawerOpen"
      :title="editingInterfaceId ? '编辑接口' : '新建接口'"
      :description="
        editingInterfaceId ? '维护接口名称和模型列表。' : '创建接口后会生成一个 API Token。'
      "
      label="接口配置"
      @close="drawerOpen = false"
    >
      <section class="rounded-lg border border-stone-200 p-4">
        <h4 class="font-semibold text-stone-800">接口配置</h4>
        <div class="mt-4 grid gap-4">
          <label class="grid gap-1.5 text-xs font-medium uppercase tracking-wide text-stone-500">
            名称
            <input
              v-model="form.name"
              class="rounded-lg border border-stone-200 px-3 py-2 text-sm normal-case tracking-normal text-stone-800"
              placeholder="Codex 主接口"
            />
          </label>
        </div>
      </section>

      <section class="rounded-lg border border-stone-200 p-4">
        <div class="flex items-center justify-between gap-3">
          <h4 class="font-semibold text-stone-800">模型列表</h4>
          <span class="font-mono text-xs text-stone-400"> {{ form.models.length }} 个 </span>
        </div>

        <div class="mt-4 grid gap-2">
          <div
            v-for="model in form.models"
            :key="model.form_id"
            class="flex items-center justify-between gap-3 rounded-lg border border-stone-200 px-3 py-2"
          >
            <div>
              <strong class="font-mono text-sm text-stone-800">{{ model.model_name }}</strong>
              <div class="mt-1 text-xs text-stone-400">
                {{ providerForModel(model)?.name ?? '已删除供应商' }} /
                {{ model.upstream_model }}
              </div>
            </div>
            <button class="text-sm text-red-600" @click="removeModelFromForm(model)">删除</button>
          </div>
          <div v-if="form.models.length === 0" class="text-sm text-stone-400">暂无模型。</div>
        </div>

        <div class="mt-4 grid gap-4 border-t border-stone-100 pt-4 sm:grid-cols-2">
          <label class="grid gap-1.5 text-xs font-medium uppercase tracking-wide text-stone-500">
            供应商
            <select
              v-model="modelForm.provider_id"
              class="rounded-lg border border-stone-200 bg-white px-3 py-2 text-sm normal-case tracking-normal text-stone-800"
              @change="modelForm.upstream_model = firstProviderModelName(modelForm.provider_id)"
            >
              <option value="">选择供应商</option>
              <option
                v-for="provider in selectableProviders"
                :key="provider.value"
                :value="provider.value"
              >
                {{ provider.label }}
              </option>
            </select>
          </label>
          <label class="grid gap-1.5 text-xs font-medium uppercase tracking-wide text-stone-500">
            上游模型
            <select
              v-model="modelForm.upstream_model"
              class="rounded-lg border border-stone-200 bg-white px-3 py-2 font-mono text-sm normal-case tracking-normal text-stone-800 disabled:bg-stone-50 disabled:text-stone-400"
              :disabled="providerModelOptionsForForm.length === 0"
            >
              <option value="">
                {{ modelForm.provider_id ? '选择上游模型' : '先选择供应商' }}
              </option>
              <option
                v-for="model in providerModelOptionsForForm"
                :key="model.value"
                :value="model.value"
              >
                {{ model.label }}
              </option>
            </select>
          </label>
          <label
            class="grid gap-1.5 text-xs font-medium uppercase tracking-wide text-stone-500 sm:col-span-2"
          >
            对外模型名
            <input
              v-model="modelForm.model_name"
              class="rounded-lg border border-stone-200 px-3 py-2 font-mono text-sm normal-case tracking-normal text-stone-800"
              placeholder="不填则使用上游模型名"
            />
          </label>
          <div class="flex justify-end sm:col-span-2">
            <Button variant="teal" :disabled="saving" @click="addModelToForm"> 添加模型 </Button>
          </div>
        </div>
      </section>

      <template #footer>
        <p class="text-sm" :class="message?.type === 'error' ? 'text-red-600' : 'text-[#176b5d]'">
          {{ message?.text }}
        </p>
        <div class="flex gap-2">
          <Button variant="secondary" :disabled="saving" @click="drawerOpen = false"> 取消 </Button>
          <Button :disabled="saving" @click="saveInterface">
            {{ saving ? '保存中...' : editingInterfaceId ? '保存接口' : '创建接口' }}
          </Button>
        </div>
      </template>
    </DrawerPanel>
  </PageShell>
</template>

<script setup lang="ts">
import { Copy, LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-vue-next';
import { computed, onMounted, ref, watch } from 'vue';
import {
  configApi,
  type InterfaceModelInput,
  type InterfaceModelResponse,
  type InterfaceResponse,
  type ProviderConfig,
} from '../api';
import {
  CopyButton,
  DataTableShell,
  DrawerPanel,
  IconButton,
  Button,
  PageHeader,
  PageShell,
  SurfacePanel,
} from '../components/base';
import { useManagementList } from '../composables/useManagementList';
import { managementErrorMessage } from '../utils/apiErrors';
import { persistInterface, type InterfacePersistenceCommand } from '../utils/interfacePersistence';
import { createKeyedRequestCoordinator } from '../utils/providerCoordination';
import {
  hasProviderModel,
  providerModelOptions,
  providerOptionsForInterface,
} from '../utils/providerModels';
import { copyToClipboard } from '../utils/providers';

type InterfaceModelFormItem = {
  form_id: string;
  id?: string;
  interface_id?: string;
  model_name: string;
  provider_id: string;
  upstream_model: string;
  created_at?: string;
};

const props = withDefaults(
  defineProps<{
    searchQuery?: string;
  }>(),
  {
    searchQuery: '',
  },
);

const {
  items: providers,
  loading: providersLoading,
  loadError: providerLoadError,
  load: loadProviders,
} = useManagementList<ProviderConfig>(
  async () => (await configApi.list()).data,
  managementErrorMessage,
);
const {
  items: interfaces,
  loading: interfacesLoading,
  loadError: interfaceLoadError,
  load: loadInterfaces,
} = useManagementList<InterfaceResponse>(
  async () => (await configApi.listInterfaces()).data,
  managementErrorMessage,
);
const expandedInterfaceId = ref<string | null>(null);
const drawerOpen = ref(false);
const editingInterfaceId = ref<string | null>(null);
const saving = ref(false);
const actionError = ref<string | null>(null);
const deleteRequests = createKeyedRequestCoordinator<string>();
const deletingInterfaceIds = ref<Set<string>>(new Set());
const message = ref<{ type: 'success' | 'error'; text: string } | null>(null);
const form = ref(defaultForm());
const modelForm = ref(defaultModelForm());

const loading = computed(() => providersLoading.value || interfacesLoading.value);
const loadError = computed(() => interfaceLoadError.value ?? providerLoadError.value);
const filteredInterfaces = computed(() =>
  interfaces.value.filter((item) => interfaceMatchesSearch(item)),
);
const selectedProvider = computed(
  () => providers.value.find((provider) => provider.id === modelForm.value.provider_id) ?? null,
);
const selectableProviders = computed(() => providerOptionsForInterface(providers.value));
const providerModelOptionsForForm = computed(() => providerModelOptions(selectedProvider.value));

onMounted(() => {
  void loadData();
});

watch(filteredInterfaces, (items) => {
  if (items[0] && !items.some((item) => item.id === expandedInterfaceId.value)) {
    expandedInterfaceId.value = items[0].id;
  }
});

async function loadData() {
  await Promise.all([loadProviders(), loadInterfaces()]);
}

function openCreateDrawer() {
  editingInterfaceId.value = null;
  form.value = defaultForm();
  modelForm.value = defaultModelForm();
  message.value = null;
  drawerOpen.value = true;
}

function openEditDrawer(item: InterfaceResponse) {
  editingInterfaceId.value = item.id;
  form.value = {
    name: item.name,
    models: item.models.map(interfaceModelToFormItem),
  };
  modelForm.value = defaultModelForm();
  message.value = null;
  drawerOpen.value = true;
}

function defaultForm() {
  return {
    name: '',
    models: [] as InterfaceModelFormItem[],
  };
}

function defaultModelForm() {
  return {
    provider_id: '',
    upstream_model: '',
    model_name: '',
  };
}

async function saveInterface() {
  if (saving.value) {
    return;
  }
  message.value = null;
  if (!form.value.name.trim()) {
    message.value = { type: 'error', text: '请填写接口名称。' };
    return;
  }
  if (form.value.models.length === 0) {
    message.value = { type: 'error', text: '请至少添加一个模型。' };
    return;
  }
  if (!form.value.models.every(modelIsSelectableForInterface)) {
    message.value = { type: 'error', text: '模型必须来自供应商模型清单。' };
    return;
  }

  const command: InterfacePersistenceCommand = editingInterfaceId.value
    ? {
        type: 'update',
        id: editingInterfaceId.value,
        payload: { name: form.value.name.trim(), models: interfaceModelsFromForm() },
      }
    : {
        type: 'create',
        payload: { name: form.value.name.trim(), models: interfaceModelsFromForm() },
      };

  saving.value = true;
  try {
    await persistInterface(configApi, command);
  } catch (error) {
    message.value = { type: 'error', text: `保存失败：${managementErrorMessage(error)}` };
    return;
  } finally {
    saving.value = false;
  }

  drawerOpen.value = false;
  editingInterfaceId.value = null;
  await loadData();
}

function addModelToForm() {
  message.value = null;
  if (!modelForm.value.provider_id || !modelForm.value.upstream_model.trim()) {
    message.value = { type: 'error', text: '请选择供应商和上游模型。' };
    return;
  }
  const provider = selectedProvider.value;
  if (!provider || !hasProviderModel(provider, modelForm.value.upstream_model)) {
    message.value = { type: 'error', text: '上游模型必须来自所选供应商的模型清单。' };
    return;
  }
  const upstreamModel = modelForm.value.upstream_model.trim();
  const modelName = modelForm.value.model_name.trim() || upstreamModel;
  if (form.value.models.some((model) => model.model_name === modelName)) {
    message.value = { type: 'error', text: '接口内模型名不能重复。' };
    return;
  }
  form.value.models.push({
    form_id: crypto.randomUUID(),
    provider_id: modelForm.value.provider_id,
    upstream_model: upstreamModel,
    model_name: modelName,
  });
  modelForm.value = defaultModelForm();
}

async function deleteInterface(item: InterfaceResponse) {
  if (isInterfaceDeleteBusy(item.id)) {
    return;
  }
  if (!window.confirm(`确定删除接口「${item.name}」吗？`)) {
    return;
  }
  const request = deleteRequests.begin(item.id);
  if (!request) {
    return;
  }
  deletingInterfaceIds.value = new Set([...deletingInterfaceIds.value, item.id]);
  actionError.value = null;
  try {
    await configApi.deleteInterface(item.id);
    if (expandedInterfaceId.value === item.id) {
      expandedInterfaceId.value = null;
    }
    if (editingInterfaceId.value === item.id) {
      drawerOpen.value = false;
      editingInterfaceId.value = null;
    }
    await loadData();
  } catch (error) {
    actionError.value = managementErrorMessage(error);
  } finally {
    deleteRequests.finish(request);
    const nextDeletingIds = new Set(deletingInterfaceIds.value);
    nextDeletingIds.delete(item.id);
    deletingInterfaceIds.value = nextDeletingIds;
  }
}

function isInterfaceDeleteBusy(interfaceId: string) {
  return deletingInterfaceIds.value.has(interfaceId);
}

function providerForModel(model: Pick<InterfaceModelFormItem, 'provider_id'>) {
  return providers.value.find((provider) => provider.id === model.provider_id) ?? null;
}

function selectInterface(interfaceId: string) {
  expandedInterfaceId.value = interfaceId;
}

function maskToken(token: string) {
  if (token.length <= 10) {
    return '*'.repeat(token.length);
  }
  return `${token.slice(0, 6)}...${token.slice(-4)}`;
}

async function copyInterfaceToken(token: string) {
  message.value = (await copyToClipboard(token))
    ? { type: 'success', text: 'API Token 已复制。' }
    : { type: 'error', text: '复制失败，请手动复制。' };
}

async function copyInterfaceModelName(modelName: string) {
  message.value = (await copyToClipboard(modelName))
    ? { type: 'success', text: '模型名已复制。' }
    : { type: 'error', text: '复制失败，请手动复制。' };
}

function interfaceMatchesSearch(item: InterfaceResponse) {
  const query = props.searchQuery.trim().toLowerCase();
  if (!query) {
    return true;
  }

  const modelText = item.models
    .flatMap((model) => [
      model.model_name,
      model.upstream_model,
      providerForModel(model)?.name ?? '',
      providerForModel(model)?.provider_type ?? '',
    ])
    .join(' ');

  return [item.name, item.token, item.id, modelText].join(' ').toLowerCase().includes(query);
}

function interfaceModelToFormItem(model: InterfaceModelResponse): InterfaceModelFormItem {
  return {
    form_id: model.id,
    id: model.id,
    interface_id: model.interface_id,
    model_name: model.model_name,
    provider_id: model.provider_id,
    upstream_model: model.upstream_model,
    created_at: model.created_at,
  };
}

function modelIsSelectableForInterface(model: InterfaceModelFormItem) {
  const provider = providerForModel(model) ?? undefined;
  return hasProviderModel(provider, model.upstream_model);
}

function removeModelFromForm(model: InterfaceModelFormItem) {
  form.value.models = form.value.models.filter((item) => item.form_id !== model.form_id);
}

function firstProviderModelName(providerId: string) {
  const provider = providers.value.find((item) => item.id === providerId);
  return providerModelOptions(provider)[0]?.value ?? '';
}

function interfaceModelsFromForm(): InterfaceModelInput[] {
  return form.value.models.map((model) => ({
    provider_id: model.provider_id,
    upstream_model: model.upstream_model,
    model_name: model.model_name,
  }));
}
</script>
