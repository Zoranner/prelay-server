import { ref, shallowRef } from 'vue';
import { managementErrorMessage } from '../utils/apiErrors';

export function useManagementList<T>(
  fetchItems: () => Promise<T[]>,
  errorMessage: (error: unknown) => string = managementErrorMessage,
) {
  const items = shallowRef<T[]>([]);
  const loading = ref(true);
  const loadError = ref<string | null>(null);
  let latestLoadGeneration = 0;

  async function load(): Promise<boolean> {
    const loadGeneration = ++latestLoadGeneration;
    loading.value = true;
    loadError.value = null;
    try {
      const loadedItems = await fetchItems();
      if (loadGeneration !== latestLoadGeneration) {
        return false;
      }
      items.value = loadedItems;
      return true;
    } catch (error) {
      if (loadGeneration !== latestLoadGeneration) {
        return false;
      }
      loadError.value = errorMessage(error);
      return false;
    } finally {
      if (loadGeneration === latestLoadGeneration) {
        loading.value = false;
      }
    }
  }

  return { items, loading, loadError, load };
}
