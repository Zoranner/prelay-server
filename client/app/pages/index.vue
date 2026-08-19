<script setup lang="ts">
import type { BootstrapState, Provider, RelayInterface } from "~/stores/relay";
import { useRelayStore } from "~/stores/relay";
import { PageHeader, PageShell, SurfacePanel } from "~/components/base";

const { bootstrap, setBootstrap } = useRelayStore();
const { error, pending, invokeCommand } = useRelayCommand();
const rotatingCredential = ref(false);
const providers = ref<Provider[]>([]);
const interfaces = ref<RelayInterface[]>([]);

async function loadBootstrap() {
  try {
    const [bootstrapState, providerList, interfaceList] = await Promise.all([
      invokeCommand<BootstrapState>("bootstrap"),
      invokeCommand<Provider[]>("providers_list"),
      invokeCommand<RelayInterface[]>("interfaces_list"),
    ]);
    setBootstrap(bootstrapState);
    providers.value = providerList;
    interfaces.value = interfaceList;
  } catch {
    // The command composable keeps the stable error for the view.
  }
}

async function rotateCredential() {
  rotatingCredential.value = true;
  try {
    await invokeCommand("credential_rotate");
    await loadBootstrap();
  } catch {
    // The command composable keeps the stable error for the view.
  } finally {
    rotatingCredential.value = false;
  }
}

onMounted(loadBootstrap);
</script>

<template>
  <PageShell>
    <PageHeader
      title="工作台"
      description="查看当前服务连接，并继续完成供应商与接入配置。"
    >
      <template #actions
        ><NuxtLink class="button-primary no-underline" to="/providers"
          >添加供应商</NuxtLink
        ><NuxtLink class="button-secondary no-underline" to="/interfaces"
          >管理接入</NuxtLink
        ></template
      >
    </PageHeader>
    <SurfacePanel>
      <section class="grid min-h-0 gap-4 p-6 lg:grid-cols-3">
        <div class="rounded-lg border border-stone-200 bg-stone-50 p-5">
          <p class="text-sm text-stone-500">管理服务</p>
          <strong class="mt-2 block text-lg font-semibold text-stone-800">
            {{ bootstrap?.relay_url ?? "正在连接" }}
          </strong>
          <NuxtLink
            class="mt-4 inline-block text-sm text-[#176b5d]"
            to="/settings"
          >
            服务设置
          </NuxtLink>
        </div>
        <div class="rounded-lg border border-stone-200 bg-stone-50 p-5">
          <p class="text-sm text-stone-500">供应商</p>
          <strong class="mt-2 block text-3xl font-semibold text-stone-800">{{
            providers.length
          }}</strong>
          <NuxtLink
            class="mt-4 inline-block text-sm text-[#176b5d]"
            to="/providers"
          >
            管理供应商
          </NuxtLink>
        </div>
        <div class="rounded-lg border border-stone-200 bg-stone-50 p-5">
          <p class="text-sm text-stone-500">接入配置</p>
          <strong class="mt-2 block text-3xl font-semibold text-stone-800">{{
            interfaces.length
          }}</strong>
          <NuxtLink
            class="mt-4 inline-block text-sm text-[#176b5d]"
            to="/interfaces"
          >
            管理接入
          </NuxtLink>
        </div>
        <div
          class="rounded-lg border border-stone-200 bg-white p-5 lg:col-span-2"
        >
          <h2 class="font-semibold text-stone-800">当前身份</h2>
          <p v-if="pending && !bootstrap" class="mt-3 text-sm text-stone-500">
            正在读取管理状态。
          </p>
          <p
            v-else-if="bootstrap"
            class="mt-3 text-sm leading-6 text-stone-600"
          >
            {{ bootstrap.username || "未提供 Windows 账户" }}
            的供应商和接入配置仅在当前身份范围内生效。
          </p>
          <p v-else-if="error" class="mt-3 text-sm text-red-700">
            {{ error.message }}
          </p>
        </div>
        <div class="rounded-lg border border-stone-200 bg-white p-5">
          <h2 class="font-semibold text-stone-800">设备凭据</h2>
          <p class="mt-3 text-sm text-stone-500">
            {{ bootstrap?.has_device_credential ? "已保存" : "尚未注册" }}
          </p>
          <button
            class="button-secondary mt-4"
            :disabled="rotatingCredential"
            @click="rotateCredential"
          >
            {{ rotatingCredential ? "轮换中" : "轮换设备凭据" }}
          </button>
        </div>
      </section>
    </SurfacePanel>
  </PageShell>
</template>
