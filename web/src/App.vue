<template>
  <div class="app-root">
    <header class="app-header">
      <div class="app-frame flex flex-col gap-4 px-6 py-4 lg:flex-row lg:items-center">
        <div>
          <h1 class="text-xl font-semibold tracking-tight text-stone-900">Provider Relay</h1>
          <p class="mt-1 text-xs tracking-wide text-stone-500">大模型服务透传代理</p>
        </div>

        <nav
          class="inline-flex rounded-lg border border-stone-200 bg-white p-1"
          aria-label="主导航"
        >
          <a
            v-for="tab in tabs"
            :key="tab.value"
            :href="tab.path"
            class="rounded-md px-4 py-2 text-sm font-medium transition-colors"
            :class="
              activeView === tab.value
                ? 'bg-[#e8f4f0] text-[#176b5d]'
                : 'text-stone-500 hover:bg-stone-50 hover:text-stone-700'
            "
            @click.prevent="navigateTo(tab.value)"
          >
            {{ tab.label }}
          </a>
        </nav>
      </div>
    </header>

    <main class="min-h-0 flex-1 overflow-hidden">
      <div class="app-frame h-full min-h-0 px-6 py-8">
        <StatsView v-if="activeView === 'stats'" />
        <ProvidersView v-else-if="activeView === 'providers'" />
        <InterfacesView v-else />
      </div>
    </main>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import {
  APP_ROUTES,
  defaultRoutePathForPath,
  pathForView,
  routeViewForPath,
  type ViewKey,
} from './utils/appRoutes';
import InterfacesView from './views/InterfacesView.vue';
import ProvidersView from './views/ProvidersView.vue';
import StatsView from './views/StatsView.vue';

const currentPath = () => (typeof window === 'undefined' ? '/stats' : window.location.pathname);

const tabs = APP_ROUTES;
const activeView = ref<ViewKey>(routeViewForPath(currentPath()));

onMounted(() => {
  replaceMissingRoute();
  window.addEventListener('popstate', syncRouteFromLocation);
});

onBeforeUnmount(() => {
  window.removeEventListener('popstate', syncRouteFromLocation);
});

function navigateTo(view: ViewKey) {
  const nextPath = pathForView(view);
  activeView.value = view;
  if (window.location.pathname !== nextPath) {
    window.history.pushState(null, '', nextPath);
  }
}

function syncRouteFromLocation() {
  replaceMissingRoute();
  activeView.value = routeViewForPath(window.location.pathname);
}

function replaceMissingRoute() {
  const defaultPath = defaultRoutePathForPath(window.location.pathname);
  if (defaultPath) {
    window.history.replaceState(null, '', defaultPath);
  }
}
</script>
