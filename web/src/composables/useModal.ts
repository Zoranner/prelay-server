import { ref } from 'vue';

export interface UseModalOptions {
  title: string;
  message: string;
  onConfirm: () => void;
}

export function useModal() {
  const open = ref(false);
  const title = ref('');
  const message = ref('');
  const onConfirm = ref<(() => void) | null>(null);

  function show(opts: UseModalOptions) {
    title.value = opts.title;
    message.value = opts.message;
    onConfirm.value = opts.onConfirm;
    open.value = true;
  }

  function hide() {
    open.value = false;
  }

  return { open, title, message, onConfirm, show, hide };
}
