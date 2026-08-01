<template>
  <PageShell>
    <PageHeader title="供应商" description="添加上游服务，管理可用模型，做连接测试。">
      <template #actions>
        <Button variant="secondary" size="sm" :disabled="loading" @click="loadData">
          {{ loading ? '刷新中...' : '刷新' }}
        </Button>
        <Button size="sm" @click="openDrawer"> 添加供应商 </Button>
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
        正在加载供应商...
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

    <SurfacePanel v-else-if="!loadError && filteredProviders.length === 0">
      <div class="flex min-h-40 items-center justify-center px-6 py-10 text-sm text-stone-400">
        暂无供应商
      </div>
    </SurfacePanel>

    <SurfacePanel v-else>
      <DataTableShell table-class="min-w-[60rem]">
        <colgroup>
          <!-- 名称列和协议列自适应 -->
          <col />
          <col class="w-32" />
          <col />
          <col class="w-20" />
          <col class="w-24" />
          <col class="w-24" />
        </colgroup>
        <thead class="sticky top-0 z-10 bg-stone-50 text-xs font-semibold text-stone-500">
          <tr>
            <th class="px-5 py-3 text-left">名称</th>
            <th class="px-5 py-3 text-left">类型</th>
            <th class="px-5 py-3 text-left">协议</th>
            <th class="px-5 py-3 text-left">模型</th>
            <th class="px-5 py-3 text-left">状态</th>
            <th class="px-5 py-3 text-right">操作</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-stone-100">
          <template v-for="provider in filteredProviders" :key="provider.id">
            <tr
              class="selectable-row"
              :class="expandedProviderId === provider.id ? 'selected-row' : ''"
              tabindex="0"
              :aria-selected="expandedProviderId === provider.id"
              @click="selectProvider(provider.id)"
              @keydown.enter.prevent="selectProvider(provider.id)"
              @keydown.space.prevent="selectProvider(provider.id)"
            >
              <td class="px-5 py-4">
                <div class="flex max-w-full items-center gap-2 font-semibold text-stone-800">
                  <span
                    class="h-2 w-2 rounded-full"
                    :class="providerDotClass(provider.provider_type)"
                  ></span>
                  {{ provider.name || providerLabel(provider.provider_type) }}
                </div>
                <div class="mt-1 max-w-[320px] truncate font-mono text-xs text-stone-400">
                  {{ provider.base_url }}
                </div>
              </td>
              <td class="px-5 py-4">
                <span
                  class="rounded-full px-2 py-0.5 text-xs font-medium transition-opacity hover:opacity-80 focus:outline-none focus:ring-2 focus:ring-[#176b5d]/20"
                  :class="categoryClass(provider.provider_type)"
                >
                  {{ categoryLabel(provider.provider_type) }}
                </span>
              </td>
              <td class="px-5 py-4 whitespace-nowrap text-stone-600">
                <div class="flex flex-nowrap gap-1.5">
                  <span
                    v-for="protocol in upstreamProtocolOptionsForProvider(
                      provider.provider_type,
                      provider.capabilities,
                    )"
                    :key="protocol.value"
                    class="rounded-full border px-2 py-0.5 text-xs font-medium"
                    :class="protocolTagClass(protocol.value)"
                  >
                    {{ protocol.label }}
                  </span>
                </div>
              </td>
              <td class="px-5 py-4 text-stone-600">
                {{ provider.models.length || '待添加' }}
              </td>
              <td class="px-5 py-4">
                <button
                  type="button"
                  class="rounded-full px-2 py-0.5 text-xs font-medium"
                  :class="pingClass(provider.id)"
                  :aria-label="providerPingAriaLabel(provider)"
                  :disabled="isProviderPingBusy(provider.id)"
                  @click.stop="pingProvider(provider)"
                >
                  {{ pingLabel(provider.id) }}
                </button>
              </td>
              <td class="px-5 py-4">
                <div class="flex justify-end gap-2">
                  <IconButton label="编辑供应商" @click.stop="openEditDrawer(provider)">
                    <Pencil class="h-4 w-4" aria-hidden="true" />
                  </IconButton>
                  <IconButton
                    label="删除供应商"
                    variant="danger"
                    :disabled="isProviderDeleteBusy(provider.id)"
                    @click.stop="deleteProvider(provider)"
                  >
                    <Trash2 class="h-4 w-4" aria-hidden="true" />
                  </IconButton>
                </div>
              </td>
            </tr>

            <tr v-if="expandedProviderId === provider.id" class="expanded-row">
              <td colspan="6" class="px-5 py-4">
                <div class="flex flex-wrap gap-2">
                  <span
                    v-for="model in provider.models"
                    :key="model.id"
                    class="rounded-full border border-stone-200 bg-white px-3 py-1 font-mono text-xs text-stone-700"
                  >
                    {{ model.model_name }}
                  </span>
                  <div v-if="provider.models.length === 0" class="text-sm text-stone-400">
                    暂无模型。编辑供应商后添加模型。
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
      :title="editingProviderId ? '编辑供应商' : '添加供应商'"
      :description="
        editingProviderId
          ? '修改供应商连接信息；API Key 留空表示不更换。'
          : '选择服务，填写连接信息，并维护模型清单。'
      "
      label="添加供应商"
      size="lg"
      @close="drawerOpen = false"
    >
      <section class="rounded-lg border border-stone-200 p-4">
        <h4 class="font-semibold text-stone-800">连接配置</h4>
        <div class="mt-4 grid gap-4 sm:grid-cols-2">
          <label
            class="grid gap-1.5 text-xs font-medium uppercase tracking-wide text-stone-500 sm:col-span-2"
          >
            供应商
            <select
              v-model="form.provider_template"
              class="rounded-lg border border-stone-200 bg-white px-3 py-2 text-sm normal-case tracking-normal text-stone-800"
              @change="selectProviderTemplate(form.provider_template)"
            >
              <optgroup
                v-for="group in PROVIDER_TEMPLATE_GROUPS"
                :key="group.label"
                :label="group.label"
              >
                <option v-for="option in group.options" :key="option.value" :value="option.value">
                  {{ option.label }}
                </option>
              </optgroup>
            </select>
          </label>
          <label
            class="grid gap-1.5 text-xs font-medium uppercase tracking-wide text-stone-500 sm:col-span-2"
          >
            名称
            <input
              v-model="form.name"
              class="rounded-lg border border-stone-200 px-3 py-2 text-sm normal-case tracking-normal text-stone-800"
            />
          </label>
          <label
            class="grid gap-1.5 text-xs font-medium uppercase tracking-wide text-stone-500 sm:col-span-2"
          >
            API Key
            <input
              v-model="form.api_key"
              type="password"
              class="rounded-lg border border-stone-200 px-3 py-2 font-mono text-sm normal-case tracking-normal text-stone-800"
              :placeholder="editingProviderId ? '留空则保持原 Key' : '填写上游 API Key'"
            />
          </label>
          <label
            class="grid gap-1.5 text-xs font-medium uppercase tracking-wide text-stone-500 sm:col-span-2"
          >
            默认 Base URL
            <input
              v-model="form.base_url"
              class="rounded-lg border border-stone-200 px-3 py-2 font-mono text-sm normal-case tracking-normal text-stone-800"
            />
          </label>
          <div
            v-if="selectedProviderTemplate"
            class="grid gap-2 text-xs font-medium uppercase tracking-wide text-stone-500 sm:col-span-2"
          >
            支持协议
            <div ref="customProtocolSelectRef" class="relative">
              <button
                v-if="selectedProviderTemplate.custom"
                type="button"
                class="flex h-10 w-full items-center justify-between gap-3 overflow-hidden rounded-lg border border-stone-200 bg-white px-3 py-2 text-left text-sm normal-case tracking-normal text-stone-800 hover:border-[#b7d8cf] focus:outline-none focus:ring-2 focus:ring-[#176b5d]/15"
                aria-controls="provider-protocol-menu"
                :aria-expanded="customProtocolMenuOpen"
                @click="customProtocolMenuOpen = !customProtocolMenuOpen"
              >
                <span class="flex min-w-0 flex-1 flex-nowrap gap-1.5 overflow-x-auto">
                  <span
                    v-for="protocol in selectedCustomProtocolOptions"
                    :key="protocol.protocol"
                    class="shrink-0 rounded-full border px-2 py-0.5 text-xs font-medium"
                    :class="protocolTagClass(protocol.protocol)"
                  >
                    {{ protocol.label }}
                  </span>
                </span>
                <span class="shrink-0 text-stone-400">选择</span>
              </button>
              <div
                v-else
                class="flex h-10 w-full items-center gap-1.5 overflow-hidden rounded-lg border border-stone-200 bg-stone-50 px-3 py-2 text-sm normal-case tracking-normal text-stone-800"
              >
                <span class="flex min-w-0 flex-1 flex-nowrap gap-1.5 overflow-x-auto">
                  <span
                    v-for="protocol in selectedProviderProtocolOptions"
                    :key="protocol.protocol"
                    class="shrink-0 rounded-full border px-2 py-0.5 text-xs font-medium"
                    :class="protocolTagClass(protocol.protocol)"
                  >
                    {{ protocol.label }}
                  </span>
                </span>
              </div>
              <div
                v-if="selectedProviderTemplate.custom && customProtocolMenuOpen"
                id="provider-protocol-menu"
                role="group"
                aria-label="支持协议"
                class="absolute left-0 top-[calc(100%+6px)] z-20 grid w-full gap-1 rounded-lg border border-stone-200 bg-white p-1.5 shadow-sm"
              >
                <label
                  v-for="protocol in customProtocolOptions"
                  :key="protocol.protocol"
                  class="flex cursor-pointer items-center gap-2 rounded-md border border-transparent px-2 py-1.5 text-sm normal-case tracking-normal text-stone-700 hover:border-[#b7d8cf] hover:bg-[#f4faf8]"
                  :class="
                    form.supported_protocols.includes(protocol.protocol)
                      ? 'bg-[#f4faf8]'
                      : 'bg-white'
                  "
                >
                  <input
                    type="checkbox"
                    class="h-4 w-4 rounded border-stone-300 text-[#176b5d]"
                    :checked="form.supported_protocols.includes(protocol.protocol)"
                    @change="toggleCustomProtocol(protocol.protocol)"
                  />
                  {{ protocol.label }}
                </label>
              </div>
            </div>
            <div class="grid gap-2 normal-case tracking-normal">
              <div
                v-for="protocol in protocolBaseUrlRows"
                :key="protocol.protocol"
                class="grid gap-2 rounded-lg border border-stone-200 bg-white p-2 sm:grid-cols-[9.5rem_minmax(0,1fr)_auto] sm:items-center"
              >
                <span class="text-sm font-medium text-stone-600">{{ protocol.label }}</span>
                <input
                  v-model="form.protocol_base_urls[protocol.protocol]"
                  class="h-9 min-w-0 rounded-lg border border-stone-200 px-3 font-mono text-sm text-stone-800"
                  :placeholder="protocol.baseUrl || '留空使用默认 Base URL'"
                />
                <button
                  type="button"
                  class="flex h-9 w-9 items-center justify-center rounded-lg border border-stone-200 text-sm font-semibold text-stone-500 hover:border-[#b7d8cf] hover:text-[#176b5d] disabled:cursor-not-allowed disabled:opacity-60"
                  aria-label="测试协议连接"
                  title="测试协议连接"
                  :disabled="protocolTestState[protocol.protocol]?.type === 'loading'"
                  @click="testProviderProtocol(protocol.protocol)"
                >
                  <span
                    v-if="protocolTestState[protocol.protocol]?.type === 'loading'"
                    aria-hidden="true"
                  >
                    <LoaderCircle class="h-4 w-4 animate-spin" aria-hidden="true" />
                  </span>
                  <Gauge v-else class="h-4 w-4" aria-hidden="true" />
                </button>
                <span
                  v-if="protocolTestState[protocol.protocol]?.text"
                  class="text-xs sm:col-start-2"
                  :class="
                    protocolTestState[protocol.protocol]?.type === 'error'
                      ? 'text-red-600'
                      : 'text-[#176b5d]'
                  "
                >
                  {{ protocolTestState[protocol.protocol]?.text }}
                </span>
              </div>
            </div>
          </div>
        </div>
      </section>

      <section class="rounded-lg border border-stone-200 p-4">
        <div class="flex items-start justify-between gap-3">
          <div>
            <h4 class="font-semibold text-stone-800">模型清单</h4>
            <p class="mt-1 text-xs text-stone-500">接口页只能选择这里已经配置的上游模型。</p>
          </div>
          <div class="flex items-center gap-3">
            <span class="font-mono text-xs text-stone-400">{{ form.models.length }} 个</span>
            <Button
              variant="secondary"
              size="sm"
              :disabled="discoveringModels"
              @click="discoverModels()"
            >
              {{ discoveringModels ? '获取中...' : '获取模型' }}
            </Button>
          </div>
        </div>
        <div class="mt-4 grid gap-3">
          <div
            v-for="model in form.models"
            :key="model"
            class="flex items-center justify-between gap-3 rounded-lg border border-stone-200 px-3 py-2"
          >
            <span class="font-mono text-sm text-stone-800">{{ model }}</span>
            <button class="text-sm text-red-600" @click="removeModelFromForm(model)">删除</button>
          </div>
          <div v-if="form.models.length === 0" class="text-sm text-stone-400">暂无模型。</div>
        </div>
        <div class="mt-4 grid gap-4 sm:grid-cols-[minmax(0,1fr)_auto]">
          <label class="grid gap-1.5 text-xs font-medium uppercase tracking-wide text-stone-500">
            上游模型
            <input
              v-model="modelDraft"
              class="rounded-lg border border-stone-200 px-3 py-2 font-mono text-sm normal-case tracking-normal text-stone-800"
              placeholder="kimi-k2-0711-preview"
              @keydown.enter.prevent="addModelToForm"
            />
          </label>
          <Button class="self-end" variant="teal" @click="addModelToForm"> 添加模型 </Button>
        </div>
      </section>

      <template #footer>
        <p
          class="text-sm"
          :class="formMessage?.type === 'error' ? 'text-red-600' : 'text-[#176b5d]'"
        >
          {{ formMessage?.text }}
        </p>
        <div class="flex gap-2">
          <Button variant="secondary" @click="drawerOpen = false"> 取消 </Button>
          <Button :disabled="saving" @click="saveProvider">
            {{ saving ? '保存中...' : editingProviderId ? '保存修改' : '保存供应商' }}
          </Button>
        </div>
      </template>
    </DrawerPanel>
  </PageShell>
</template>

<script setup lang="ts">
import { Gauge, LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-vue-next';
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import {
  configApi,
  type ModelCatalogCapabilities,
  type ProviderConfig,
  type ProviderProtocolBaseUrls,
} from '../api';
import {
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
import {
  createDrawerSessionCoordinator,
  createKeyedRequestCoordinator,
  fingerprintProviderConfiguration,
  selectProvidersForPing,
  type DrawerSession,
  type ProviderPingSnapshot,
} from '../utils/providerCoordination';
import {
  DEFAULT_BASE_URLS,
  PROVIDER_GROUPS,
  PROVIDER_TEMPLATE_GROUPS,
  providerProtocolValuesForTemplate,
  providerTemplateByValue,
  providerTemplateForProviderType,
  providerDotClass,
  providerLabel,
  clearStoredProviderTokens,
  sortProviderProtocolVariants,
  type ProviderTemplate,
  type ProviderUpstreamProtocol,
} from '../utils/providers';
import {
  normalizeProviderProtocolBaseUrls,
  protocolTagClass,
  upstreamProtocolOptionsForProvider,
  upstreamProtocolValuesForProvider,
} from '../utils/providerProtocols';
import {
  mergeDiscoveredProviderModels,
  normalizeProviderModelNames,
} from '../utils/providerModels';
import {
  persistProviderConfig,
  type ProviderPersistenceCommand,
} from '../utils/providerPersistence';

type ProviderCategory = 'subscription' | 'api' | 'custom';
type ProviderForm = {
  provider_template: string;
  provider_type: string;
  supported_protocols: ProviderUpstreamProtocol[];
  name: string;
  base_url: string;
  protocol_base_urls: Record<ProviderUpstreamProtocol, string>;
  api_key: string;
  models: string[];
};

type ProviderProtocolCapabilities = ModelCatalogCapabilities & {
  upstream_protocols?: ProviderUpstreamProtocol[];
  protocol_base_urls?: ProviderProtocolBaseUrls;
};

const {
  items: providers,
  loading,
  loadError,
  load,
} = useManagementList<ProviderConfig>(async () => (await configApi.list()).data);
const saving = ref(false);
const discoveringModels = ref(false);
const actionError = ref<string | null>(null);
const drawerOpen = ref(false);
const expandedProviderId = ref<string | null>(null);
const editingProviderId = ref<string | null>(null);
const drawerSessions = createDrawerSessionCoordinator();
const pingRequests = createKeyedRequestCoordinator<string>();
const deleteRequests = createKeyedRequestCoordinator<string>();
const deletingProviderIds = ref<Set<string>>(new Set());
const pingSnapshots = ref<Record<string, ProviderPingSnapshot>>({});
const pingState = ref<
  Record<string, { type: 'idle' | 'available' | 'unavailable' | 'loading'; text: string }>
>({});
const protocolTestState = ref<
  Partial<Record<ProviderUpstreamProtocol, { type: 'ok' | 'error' | 'loading'; text: string }>>
>({});
const formMessage = ref<{ type: 'success' | 'error'; text: string } | null>(null);
const form = ref(defaultForm());
const modelDraft = ref('');
const customProtocolMenuOpen = ref(false);
const customProtocolSelectRef = ref<HTMLElement | null>(null);
const selectedProviderTemplate = computed(() =>
  providerTemplateByValue(form.value.provider_template),
);
const customProtocolOptions = computed(() =>
  selectedProviderTemplate.value?.custom
    ? sortProviderProtocolVariants(selectedProviderTemplate.value.variants)
    : [],
);
const selectedCustomProtocolOptions = computed(() =>
  customProtocolOptions.value.filter((protocol) =>
    form.value.supported_protocols.includes(protocol.protocol),
  ),
);
const selectedProviderProtocolOptions = computed(() =>
  sortProviderProtocolVariants(selectedProviderTemplate.value?.variants ?? []).filter((protocol) =>
    form.value.supported_protocols.includes(protocol.protocol),
  ),
);
const protocolBaseUrlRows = computed(() =>
  sortProviderProtocolVariants(selectedProviderTemplate.value?.variants ?? []).filter((protocol) =>
    form.value.supported_protocols.includes(protocol.protocol),
  ),
);

const filteredProviders = computed(() => providers.value);

onMounted(() => {
  clearLegacyProviderTokens();
  void loadData();
});

watch(filteredProviders, (items) => {
  if (items[0] && !items.some((item) => item.id === expandedProviderId.value)) {
    expandedProviderId.value = items[0].id;
  }
});

watch(customProtocolMenuOpen, (open) => {
  if (open) {
    document.addEventListener('pointerdown', handleCustomProtocolOutsideClick);
    return;
  }
  document.removeEventListener('pointerdown', handleCustomProtocolOutsideClick);
});

onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', handleCustomProtocolOutsideClick);
});

function clearLegacyProviderTokens() {
  try {
    if (typeof localStorage === 'undefined') {
      return;
    }
    clearStoredProviderTokens(localStorage);
  } catch {
    // Storage can be blocked by browser privacy policy.
  }
}

async function loadData() {
  const loaded = await load();
  if (loaded) {
    pingProviders(providersNeedingPing(providers.value));
  }
}

function openDrawer() {
  beginDrawerSession(null);
  form.value = defaultForm();
  modelDraft.value = '';
  customProtocolMenuOpen.value = false;
  protocolTestState.value = {};
  editingProviderId.value = null;
  formMessage.value = null;
  drawerOpen.value = true;
}

function openEditDrawer(provider: ProviderConfig) {
  beginDrawerSession(provider.id);
  const providerTemplate = providerTemplateForProviderType(provider.provider_type);
  const supportedProtocols = providerTemplate?.custom
    ? upstreamProtocolValuesForProvider(provider.provider_type, provider.capabilities)
    : providerTemplate
      ? providerProtocolValuesForTemplate(providerTemplate.value)
      : upstreamProtocolValuesForProvider(provider.provider_type, provider.capabilities);
  editingProviderId.value = provider.id;
  form.value = {
    provider_template: providerTemplate?.value ?? provider.provider_type,
    provider_type: provider.provider_type,
    supported_protocols: supportedProtocols,
    name: provider.name,
    base_url: provider.base_url,
    protocol_base_urls: {
      ...emptyProtocolBaseUrls(),
      ...protocolBaseUrlsFromTemplate(providerTemplate),
      ...normalizeProviderProtocolBaseUrls(protocolBaseUrlsFromCapabilities(provider.capabilities)),
    },
    api_key: '',
    models: provider.models.map((model) => model.model_name),
  };
  modelDraft.value = '';
  customProtocolMenuOpen.value = false;
  protocolTestState.value = {};
  formMessage.value = null;
  drawerOpen.value = true;
}

function beginDrawerSession(providerId: string | null) {
  drawerSessions.begin(providerId);
  discoveringModels.value = false;
}

function captureDrawerSession() {
  return drawerSessions.capture();
}

function isCurrentDrawerSession(session: DrawerSession | null): session is DrawerSession {
  return session !== null && drawerSessions.isCurrent(session);
}

function defaultForm(): ProviderForm {
  const template = providerTemplateByValue('kimi_code');
  if (template) {
    return formFromTemplate(template);
  }
  return {
    provider_template: 'kimi_code',
    provider_type: 'kimi_coding_anthropic',
    supported_protocols: ['anthropic', 'openai'],
    name: 'Kimi Code',
    base_url: DEFAULT_BASE_URLS.kimi_coding_anthropic,
    protocol_base_urls: {
      responses: '',
      openai: DEFAULT_BASE_URLS.kimi_coding,
      anthropic: DEFAULT_BASE_URLS.kimi_coding_anthropic,
    },
    api_key: '',
    models: [],
  };
}

function selectProviderTemplate(templateValue: string) {
  const providerTemplate = providerTemplateByValue(templateValue);
  if (!providerTemplate) {
    return;
  }
  form.value = {
    ...form.value,
    ...formFromTemplate(providerTemplate),
  };
  customProtocolMenuOpen.value = false;
  if (!editingProviderId.value) {
    resetProviderDraftFields();
  }
}

function formFromTemplate(providerTemplate: ProviderTemplate): ProviderForm {
  const supportedProtocols = providerProtocolValuesForTemplate(providerTemplate.value);
  return {
    provider_template: providerTemplate.value,
    provider_type: providerTemplate.providerType,
    supported_protocols: providerTemplate.custom ? ['openai'] : supportedProtocols,
    name: providerTemplate.label,
    base_url: providerTemplate.baseUrl,
    protocol_base_urls: protocolBaseUrlsFromTemplate(providerTemplate),
    api_key: '',
    models: [],
  };
}

function resetProviderDraftFields() {
  form.value.api_key = '';
  form.value.models = [];
  protocolTestState.value = {};
  modelDraft.value = '';
  formMessage.value = null;
}

function toggleCustomProtocol(protocol: ProviderUpstreamProtocol) {
  const selected = form.value.supported_protocols.includes(protocol)
    ? form.value.supported_protocols.filter((item) => item !== protocol)
    : [...form.value.supported_protocols, protocol];
  if (selected.length === 0) {
    return;
  }
  const protocolOrder = customProtocolOptions.value.map((item) => item.protocol);
  form.value.supported_protocols = selected.sort(
    (left, right) => protocolOrder.indexOf(left) - protocolOrder.indexOf(right),
  );
  form.value.provider_type = customProviderTypeForProtocols(form.value.supported_protocols);
  protocolTestState.value = {};
}

function handleCustomProtocolOutsideClick(event: PointerEvent) {
  const target = event.target;
  if (
    target instanceof Node &&
    customProtocolSelectRef.value &&
    !customProtocolSelectRef.value.contains(target)
  ) {
    customProtocolMenuOpen.value = false;
  }
}

function customProviderTypeForProtocols(protocols: ProviderUpstreamProtocol[]) {
  if (protocols.includes('openai')) {
    return 'openai_compatible';
  }
  if (protocols.includes('responses')) {
    return 'responses_compatible';
  }
  return 'anthropic_compatible';
}

async function saveProvider() {
  if (drawerSessions.isSaving()) {
    return;
  }
  const session = captureDrawerSession();
  if (!isCurrentDrawerSession(session)) {
    return;
  }
  formMessage.value = null;
  form.value.models = normalizeProviderModelNames(form.value.models);

  if (!form.value.base_url.trim() || (!editingProviderId.value && !form.value.api_key.trim())) {
    formMessage.value = {
      type: 'error',
      text: editingProviderId.value ? '请填写 Base URL。' : '请填写 Base URL 和 API Key。',
    };
    return;
  }
  if (selectedProviderTemplate.value?.custom && form.value.supported_protocols.length === 0) {
    formMessage.value = { type: 'error', text: '请至少选择一个支持协议。' };
    return;
  }
  if (form.value.models.length === 0) {
    formMessage.value = { type: 'error', text: '请至少添加一个上游模型。' };
    return;
  }

  const saveRequest = drawerSessions.beginSave();
  if (!saveRequest) {
    return;
  }
  saving.value = drawerSessions.isSaving();
  const saveSession = saveRequest.session;
  const payload = {
    name: form.value.name.trim() || providerLabel(form.value.provider_type),
    provider_type: form.value.provider_type,
    base_url: form.value.base_url.trim(),
    capabilities: capabilitiesFromForm(),
    models: form.value.models,
  };
  const command: ProviderPersistenceCommand = saveSession.providerId
    ? {
        type: 'update',
        id: saveSession.providerId,
        payload: {
          ...payload,
          ...(form.value.api_key.trim() ? { api_key: form.value.api_key.trim() } : {}),
        },
      }
    : {
        type: 'create',
        payload: {
          ...payload,
          api_key: form.value.api_key.trim(),
        },
      };

  try {
    await persistProviderConfig(configApi, command);
  } catch (error) {
    if (!isCurrentDrawerSession(saveSession)) {
      return;
    }
    formMessage.value = {
      type: 'error',
      text: `保存失败：${apiErrorMessage(error)}`,
    };
    return;
  } finally {
    drawerSessions.finishSave(saveRequest);
    saving.value = drawerSessions.isSaving();
  }

  if (!isCurrentDrawerSession(saveSession)) {
    await loadData();
    return;
  }
  formMessage.value = {
    type: 'success',
    text: command.type === 'update' ? '供应商已更新。' : '供应商已保存。',
  };
  drawerOpen.value = false;
  if (command.type === 'create') {
    editingProviderId.value = null;
  }
  await loadData();
}

function apiErrorMessage(error: unknown): string {
  if (
    error &&
    typeof error === 'object' &&
    'response' in error &&
    error.response &&
    typeof error.response === 'object' &&
    'data' in error.response &&
    error.response.data &&
    typeof error.response.data === 'object' &&
    'error' in error.response.data &&
    typeof error.response.data.error === 'string'
  ) {
    return error.response.data.error;
  }
  if (error instanceof Error && error.message) {
    return error.message;
  }
  return '请检查供应商配置和模型清单。';
}

function capabilitiesFromForm(): ModelCatalogCapabilities {
  const capabilities: ProviderProtocolCapabilities = {
    protocol_base_urls: protocolBaseUrlsFromForm(),
  };
  if (selectedProviderTemplate.value?.custom) {
    capabilities.upstream_protocols = form.value.supported_protocols;
  }
  return capabilities;
}

function protocolBaseUrlsFromForm(): ProviderProtocolBaseUrls {
  const urls: ProviderProtocolBaseUrls = {};
  for (const protocol of form.value.supported_protocols) {
    const baseUrl = form.value.protocol_base_urls[protocol].trim();
    if (baseUrl) {
      urls[protocol] = baseUrl;
    }
  }
  return urls;
}

function protocolBaseUrlsFromTemplate(
  providerTemplate: ProviderTemplate | undefined,
): Record<ProviderUpstreamProtocol, string> {
  const urls = emptyProtocolBaseUrls();
  if (!providerTemplate || providerTemplate.custom) {
    return urls;
  }
  for (const variant of providerTemplate.variants) {
    if (variant.baseUrl) {
      urls[variant.protocol] = variant.baseUrl;
    }
  }
  return urls;
}

function protocolBaseUrlsFromCapabilities(
  capabilities?: ModelCatalogCapabilities | null,
): ProviderProtocolBaseUrls {
  return (
    (capabilities as ProviderProtocolCapabilities | null | undefined)?.protocol_base_urls ?? {}
  );
}

function emptyProtocolBaseUrls(): Record<ProviderUpstreamProtocol, string> {
  return {
    responses: '',
    openai: '',
    anthropic: '',
  };
}

async function discoverModels(options: { silent?: boolean } = {}) {
  const session = captureDrawerSession();
  if (!isCurrentDrawerSession(session)) {
    return false;
  }
  formMessage.value = null;
  if (!form.value.base_url.trim()) {
    formMessage.value = { type: 'error', text: '请先填写 Base URL。' };
    return false;
  }
  if (!editingProviderId.value && !form.value.api_key.trim()) {
    formMessage.value = { type: 'error', text: '请先填写上游 API Key。' };
    return false;
  }

  discoveringModels.value = true;
  try {
    const response = session.providerId
      ? await configApi.discoverSavedModels(session.providerId)
      : await configApi.discoverModels({
          provider_type: form.value.provider_type,
          base_url: form.value.base_url.trim(),
          api_key: form.value.api_key.trim(),
        });
    if (!isCurrentDrawerSession(session)) {
      return false;
    }
    const beforeCount = form.value.models.length;
    form.value.models = mergeDiscoveredProviderModels(form.value.models, response.data.models);
    const addedCount = form.value.models.length - beforeCount;
    if (!options.silent) {
      formMessage.value = {
        type: 'success',
        text: addedCount > 0 ? `已获取 ${addedCount} 个新模型。` : '模型清单已是最新。',
      };
    }
    if (session.providerId) {
      await loadData();
    }
    return true;
  } catch {
    if (!isCurrentDrawerSession(session)) {
      return false;
    }
    if (!options.silent) {
      formMessage.value = { type: 'error', text: '模型获取失败，请检查连接信息和 API Key。' };
    }
    return false;
  } finally {
    if (isCurrentDrawerSession(session)) {
      discoveringModels.value = false;
    }
  }
}

async function testProviderProtocol(protocol: ProviderUpstreamProtocol) {
  const session = captureDrawerSession();
  if (!isCurrentDrawerSession(session)) {
    return;
  }
  formMessage.value = null;
  protocolTestState.value = {
    ...protocolTestState.value,
    [protocol]: { type: 'loading', text: '测试中' },
  };

  const baseUrl = effectiveProtocolBaseUrl(protocol);
  if (!baseUrl) {
    protocolTestState.value = {
      ...protocolTestState.value,
      [protocol]: { type: 'error', text: '缺少地址' },
    };
    return;
  }
  if (!editingProviderId.value && !form.value.api_key.trim()) {
    protocolTestState.value = {
      ...protocolTestState.value,
      [protocol]: { type: 'error', text: '缺少 Key' },
    };
    return;
  }

  try {
    const payload = {
      provider_type: providerTypeForProtocol(protocol),
      protocol,
      base_url: baseUrl,
      ...(form.value.api_key.trim() ? { api_key: form.value.api_key.trim() } : {}),
      ...(form.value.models[0] ? { model: form.value.models[0] } : {}),
    };
    const response = session.providerId
      ? await configApi.testSavedProtocol(session.providerId, payload)
      : await configApi.testProtocol(payload);
    if (!isCurrentDrawerSession(session)) {
      return;
    }
    if (!response.data.ok) {
      protocolTestState.value = {
        ...protocolTestState.value,
        [protocol]: {
          type: 'error',
          text: response.data.error ?? '测试失败',
        },
      };
      return;
    }
    protocolTestState.value = {
      ...protocolTestState.value,
      [protocol]: {
        type: 'ok',
        text: response.data.first_token_ms
          ? `${response.data.first_token_ms} ms 首字`
          : `${response.data.latency_ms} ms`,
      },
    };
  } catch {
    if (!isCurrentDrawerSession(session)) {
      return;
    }
    protocolTestState.value = {
      ...protocolTestState.value,
      [protocol]: { type: 'error', text: '失败' },
    };
  }
}

function effectiveProtocolBaseUrl(protocol: ProviderUpstreamProtocol) {
  return form.value.protocol_base_urls[protocol].trim() || form.value.base_url.trim();
}

function providerTypeForProtocol(protocol: ProviderUpstreamProtocol) {
  return (
    selectedProviderTemplate.value?.variants.find((variant) => variant.protocol === protocol)
      ?.providerType ?? customProviderTypeForProtocols([protocol])
  );
}

async function pingProvider(provider: ProviderConfig, options: { supersede?: boolean } = {}) {
  const request = pingRequests.begin(provider.id, options);
  if (!request) {
    return;
  }
  const configurationFingerprint = providerConfigurationFingerprint(provider);
  pingState.value[provider.id] = { type: 'loading', text: '检查中' };
  try {
    const response = await configApi.ping(provider.id);
    if (!pingRequests.isCurrent(request)) {
      return;
    }
    if (!response.data.ok) {
      pingState.value[provider.id] = {
        type: 'unavailable',
        text: '不可用',
      };
      recordProviderPing(provider.id, configurationFingerprint);
      return;
    }
    pingState.value[provider.id] = {
      type: 'available',
      text: '可用',
    };
    recordProviderPing(provider.id, configurationFingerprint);
  } catch {
    if (!pingRequests.isCurrent(request)) {
      return;
    }
    pingState.value[provider.id] = { type: 'unavailable', text: '不可用' };
    recordProviderPing(provider.id, configurationFingerprint);
  } finally {
    pingRequests.finish(request);
  }
}

function pingProviders(providersToPing: ProviderConfig[]) {
  for (const provider of providersToPing) {
    void pingProvider(provider, { supersede: true });
  }
}

function providersNeedingPing(providersToCheck: ProviderConfig[]) {
  const candidates = providersToCheck.map((provider) => ({
    id: provider.id,
    configurationFingerprint: providerConfigurationFingerprint(provider),
  }));
  const selectedIds = new Set(
    selectProvidersForPing(candidates, pingSnapshots.value, Date.now(), 5 * 60_000).map(
      (provider) => provider.id,
    ),
  );
  return providersToCheck.filter((provider) => selectedIds.has(provider.id));
}

function providerConfigurationFingerprint(provider: ProviderConfig) {
  return fingerprintProviderConfiguration({
    provider_type: provider.provider_type,
    base_url: provider.base_url,
    api_key_masked: provider.api_key_masked,
    capabilities: provider.capabilities,
  });
}

function recordProviderPing(providerId: string, configurationFingerprint: string) {
  pingSnapshots.value = {
    ...pingSnapshots.value,
    [providerId]: { configurationFingerprint, checkedAt: Date.now() },
  };
}

function isProviderPingBusy(providerId: string) {
  return pingRequests.isBusy(providerId);
}

function providerPingAriaLabel(provider: ProviderConfig) {
  const name = provider.name || providerLabel(provider.provider_type);
  return `${name}连接状态：${pingLabel(provider.id)}。点击重新检查。`;
}

function pingLabel(providerId: string) {
  return pingState.value[providerId]?.text ?? '未检查';
}

function pingClass(providerId: string) {
  const type = pingState.value[providerId]?.type ?? 'idle';
  if (type === 'available') {
    return 'bg-[#e7f4ec] text-[#28764b]';
  }
  if (type === 'unavailable') {
    return 'bg-red-50 text-red-600';
  }
  if (type === 'loading') {
    return 'bg-amber-50 text-amber-700';
  }
  return 'bg-stone-100 text-stone-500';
}

function selectProvider(providerId: string) {
  expandedProviderId.value = providerId;
}

async function deleteProvider(provider: ProviderConfig) {
  if (isProviderDeleteBusy(provider.id)) {
    return;
  }
  if (!window.confirm(`确定要删除供应商「${provider.name}」吗？`)) {
    return;
  }
  const request = deleteRequests.begin(provider.id);
  if (!request) {
    return;
  }
  deletingProviderIds.value = new Set([...deletingProviderIds.value, provider.id]);
  actionError.value = null;
  try {
    await configApi.delete(provider.id);
    await loadData();
  } catch (error) {
    actionError.value = managementErrorMessage(error);
  } finally {
    deleteRequests.finish(request);
    const nextDeletingIds = new Set(deletingProviderIds.value);
    nextDeletingIds.delete(provider.id);
    deletingProviderIds.value = nextDeletingIds;
  }
}

function isProviderDeleteBusy(providerId: string) {
  return deletingProviderIds.value.has(providerId);
}

function providerCategory(providerType: string): ProviderCategory {
  const subscriptionValues = new Set([
    ...PROVIDER_GROUPS[0].options.map((option) => option.value),
    'kimi_coding',
    'bailian_coding_openai',
    'bailian_token_openai',
    'zhipu_coding_openai',
    'minimax_token_openai',
  ]);
  const apiValues = new Set([
    ...PROVIDER_GROUPS[1].options.map((option) => option.value),
    'deepseek_anthropic',
    'qwen_responses',
    'qwen_anthropic',
    'zhipu_anthropic',
    'minimax_responses',
    'minimax_anthropic',
  ]);
  const customValues = new Set([
    ...PROVIDER_GROUPS[2].options.map((option) => option.value),
    'responses_compatible',
    'anthropic_compatible',
  ]);

  if (subscriptionValues.has(providerType)) {
    return 'subscription';
  }
  if (apiValues.has(providerType)) {
    return 'api';
  }
  if (customValues.has(providerType)) {
    return 'custom';
  }
  return 'custom';
}

function categoryLabel(providerType: string) {
  const labels: Record<ProviderCategory, string> = {
    subscription: '套餐',
    api: 'API',
    custom: '自定义',
  };
  return labels[providerCategory(providerType)];
}

function categoryClass(providerType: string) {
  const category = providerCategory(providerType);
  if (category === 'subscription') {
    return 'bg-[#e9effc] text-[#2f63d7]';
  }
  if (category === 'api') {
    return 'bg-stone-100 text-stone-600';
  }
  return 'bg-[#f3f0ea] text-stone-600';
}

function addModelToForm() {
  const modelName = modelDraft.value.trim();
  if (!modelName || form.value.models.includes(modelName)) {
    return;
  }
  form.value.models.push(modelName);
  modelDraft.value = '';
}

function removeModelFromForm(modelName: string) {
  form.value.models = form.value.models.filter((model) => model !== modelName);
}
</script>
