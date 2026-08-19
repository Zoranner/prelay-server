<script setup lang="ts">
import {
  useRelayStore,
  type BootstrapState,
  type InterfaceModel,
  type Provider,
  type RelayInterface,
} from "~/stores/relay";
import { DrawerPanel, PageHeader, PageShell } from "~/components/base";

type InterfaceFormPayload = {
  id?: string;
  name: string;
  protocol: string;
  models: InterfaceModel[];
};

const { error, pending, invokeCommand } = useRelayCommand();
const { bootstrap, setBootstrap } = useRelayStore();
const providers = ref<Provider[]>([]);
const interfaces = ref<RelayInterface[]>([]);
const editingInterface = ref<RelayInterface | null>(null);
const showForm = ref(false);
const operationMessage = ref<string | null>(null);
const endpoint = computed(
  () =>
    `${(bootstrap.value?.relay_url ?? "https://relay.rd.kim").replace(/\/$/, "")}/v1/`,
);

async function load() {
  try {
    const [providerList, interfaceList, bootstrapState] = await Promise.all([
      invokeCommand<Provider[]>("providers_list"),
      invokeCommand<RelayInterface[]>("interfaces_list"),
      bootstrap.value
        ? Promise.resolve(bootstrap.value)
        : invokeCommand<BootstrapState>("bootstrap"),
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
        models: payload.models.map(
          ({ provider_id, upstream_model, model_name }) => ({
            provider_id,
            upstream_model,
            model_name: model_name || null,
          }),
        ),
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
  if (!confirm(`重置“${item.name}”的 Interface Token？现有工具将立即失效。`))
    return;
  try {
    await invokeCommand("interfaces_regenerate_token", {
      interfaceId: item.id,
    });
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
  <PageShell>
    <PageHeader
      title="接口"
      description="AI 工具使用根 /v1/ 地址和 Interface Token 访问此电脑的配置。"
    >
      <template #actions
        ><button class="button-secondary" :disabled="pending" @click="load">
          {{ pending ? "刷新中..." : "刷新" }}</button
        ><button
          class="button-primary"
          :disabled="!providers.length"
          @click="createInterface"
        >
          添加接口
        </button></template
      >
    </PageHeader>
    <p v-if="!providers.length" class="notice notice--warning">
      请先在供应商页面配置至少一个模型。
    </p>
    <p v-if="error" class="notice notice--error">{{ error.message }}</p>
    <p v-else-if="operationMessage" class="notice">{{ operationMessage }}</p>
    <section class="min-h-0 flex-1">
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
    <DrawerPanel
      :open="showForm"
      :title="editingInterface ? '编辑接口' : '添加接口'"
      :description="'选择供应商模型并维护对外模型映射。'"
      label="接口配置"
      @close="showForm = false"
    >
      <InterfacesInterfaceForm
        :interface="editingInterface"
        :providers="providers"
        :pending="pending"
        @save="saveInterface"
        @cancel="showForm = false"
      />
    </DrawerPanel>
  </PageShell>
</template>
