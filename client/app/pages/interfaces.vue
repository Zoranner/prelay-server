<script setup lang="ts">
import {
  useRelayStore,
  type BootstrapState,
  type InterfaceModel,
  type Provider,
  type RelayInterface,
} from "~/stores/relay";

type InterfaceFormPayload = { id?: string; name: string; protocol: string; models: InterfaceModel[] };

const { error, pending, invokeCommand } = useRelayCommand();
const { bootstrap, setBootstrap } = useRelayStore();
const providers = ref<Provider[]>([]);
const interfaces = ref<RelayInterface[]>([]);
const editingInterface = ref<RelayInterface | null>(null);
const showForm = ref(false);
const operationMessage = ref<string | null>(null);
const endpoint = computed(() => `${(bootstrap.value?.relay_url ?? "https://relay.rd.kim").replace(/\/$/, "")}/v1/`);

async function load() {
  try {
    const [providerList, interfaceList, bootstrapState] = await Promise.all([
      invokeCommand<Provider[]>("providers_list"),
      invokeCommand<RelayInterface[]>("interfaces_list"),
      bootstrap.value ? Promise.resolve(bootstrap.value) : invokeCommand<BootstrapState>("bootstrap"),
    ]);
    providers.value = providerList;
    interfaces.value = interfaceList;
    setBootstrap(bootstrapState);
  } catch {
    // The command composable exposes the error to this view.
  }
}

async function saveInterface(payload: InterfaceFormPayload) {
  operationMessage.value = null;
  try {
    await invokeCommand("interfaces_save", {
      ...(payload.id ? { interfaceId: payload.id } : {}),
      input: {
        name: payload.name,
        protocol: payload.protocol,
        models: payload.models.map(({ provider_id, upstream_model, model_name }) => ({
          provider_id,
          upstream_model,
          model_name: model_name || null,
        })),
      },
    });
    editingInterface.value = null;
    showForm.value = false;
    await load();
    operationMessage.value = "接口已保存。";
  } catch {
    // The command composable exposes the error to this view.
  }
}

async function deleteInterface(item: RelayInterface) {
  if (!confirm(`删除接口“${item.name}”？`)) return;
  try {
    await invokeCommand("interfaces_delete", { interfaceId: item.id });
    await load();
    operationMessage.value = "接口已删除。";
  } catch {
    // The command composable exposes the error to this view.
  }
}

async function regenerateToken(item: RelayInterface) {
  if (!confirm(`重置“${item.name}”的 Interface Token？现有工具将立即失效。`)) return;
  try {
    await invokeCommand("interfaces_regenerate_token", { interfaceId: item.id });
    await load();
    operationMessage.value = "Interface Token 已重置。";
  } catch {
    // The command composable exposes the error to this view.
  }
}

async function copy(value: string) {
  try {
    await navigator.clipboard.writeText(value);
    operationMessage.value = "已复制到剪贴板。";
  } catch {
    operationMessage.value = "无法访问剪贴板，请手动复制。";
  }
}

function edit(item: RelayInterface) {
  editingInterface.value = item;
  showForm.value = true;
}

function createInterface() {
  editingInterface.value = null;
  showForm.value = true;
}

onMounted(load);
</script>

<template>
  <main class="page">
    <div class="flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 class="page-heading">接口</h1>
        <p class="page-subheading">AI 工具使用接口的根 <code>/v1/</code> 地址和 Interface Token。</p>
      </div>
      <button class="button-primary" :disabled="!providers.length" @click="createInterface">添加接口</button>
    </div>

    <p v-if="!providers.length" class="mt-5 border border-amber-900 bg-amber-950/30 p-3 text-sm text-amber-200">
      请先在供应商页面配置至少一个模型。
    </p>
    <section v-if="showForm" class="panel mt-6">
      <h2 class="mb-5 font-medium text-white">{{ editingInterface ? "编辑接口" : "添加接口" }}</h2>
      <InterfacesInterfaceForm
        :interface="editingInterface"
        :providers="providers"
        :pending="pending"
        @save="saveInterface"
        @cancel="showForm = false"
      />
    </section>
    <p v-if="error" class="mt-5 border border-rose-900 bg-rose-950/40 p-3 text-sm text-rose-200">{{ error.message }}</p>
    <p v-else-if="operationMessage" class="mt-5 border border-emerald-900 bg-emerald-950/30 p-3 text-sm text-emerald-200">{{ operationMessage }}</p>
    <section class="mt-6">
      <InterfacesInterfaceList
        :interfaces="interfaces"
        :endpoint="endpoint"
        :pending="pending"
        @edit="edit"
        @remove="deleteInterface"
        @regenerate="regenerateToken"
        @copy="copy"
      />
    </section>
  </main>
</template>
