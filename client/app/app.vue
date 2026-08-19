<script setup lang="ts">
import {
  Activity,
  KeyRound,
  LayoutDashboard,
  Plug,
  RefreshCw,
  Settings,
  WifiOff,
} from "lucide-vue-next";

const primaryNavigation = [
  { label: "工作台", path: "/", icon: LayoutDashboard },
  { label: "供应商", path: "/providers", icon: Plug },
  { label: "接入", path: "/interfaces", icon: KeyRound },
  { label: "活动", path: "/stats", icon: Activity },
];
const settingsNavigation = { label: "设置", path: "/settings", icon: Settings };

const managementApi = useRelayManagementApiStatus();
const relaySettings = useRelaySettings();
const route = useRoute();
const isSetupRoute = computed(() => route.path === "/setup");
const canShowManagementError = computed(
  () => route.path !== "/setup" && route.path !== "/settings",
);

function reloadApplication() {
  window.location.reload();
}

function openServiceSettings() {
  managementApi.clear();
  void navigateTo("/settings");
}
</script>

<template>
  <div class="app-root" :class="{ 'app-root--setup': isSetupRoute }">
    <aside v-if="!isSetupRoute" class="workspace-sidebar">
      <NuxtLink class="workspace-brand" to="/">
        <span class="workspace-brand__name">Provider Relay</span>
        <span class="workspace-brand__caption">大模型服务透传代理</span>
      </NuxtLink>
      <nav class="workspace-nav" aria-label="工作区导航">
        <NuxtLink
          v-for="item in primaryNavigation"
          :key="item.path"
          class="workspace-nav__link"
          :to="item.path"
        >
          <component :is="item.icon" :size="18" aria-hidden="true" />
          {{ item.label }}
        </NuxtLink>
      </nav>
      <div class="workspace-sidebar__footer">
        <NuxtLink class="workspace-nav__link" :to="settingsNavigation.path">
          <component
            :is="settingsNavigation.icon"
            :size="18"
            aria-hidden="true"
          />
          {{ settingsNavigation.label }}
        </NuxtLink>
      </div>
    </aside>
    <div v-if="!isSetupRoute" class="workspace-main">
      <header class="workspace-topbar">
        <p>管理工作台</p>
        <span>{{ relaySettings.relayUrl ?? "未配置服务地址" }}</span>
      </header>
      <main class="workspace-page">
        <div class="workspace-frame">
          <NuxtPage />
        </div>
      </main>
    </div>
    <NuxtPage v-else />
    <section
      v-if="managementApi.error && canShowManagementError"
      class="app-unavailable"
      role="alert"
      aria-live="assertive"
    >
      <div class="app-unavailable__content">
        <WifiOff :size="32" stroke-width="1.75" aria-hidden="true" />
        <h2>无法连接管理服务</h2>
        <p>当前无法访问 Provider Relay 管理 API。</p>
        <p class="app-unavailable__hint">
          请检查网络连接和服务地址，然后重新加载。
        </p>
        <div class="flex flex-wrap justify-center gap-2">
          <button
            class="button-secondary"
            type="button"
            @click="openServiceSettings"
          >
            服务设置
          </button>
          <button
            class="button-primary"
            type="button"
            @click="reloadApplication"
          >
            <RefreshCw :size="16" aria-hidden="true" />
            重新加载
          </button>
        </div>
      </div>
    </section>
  </div>
</template>
