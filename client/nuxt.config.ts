import tailwindcss from '@tailwindcss/vite';

export default defineNuxtConfig({
  srcDir: 'app/',
  css: ['~/assets/css/main.css'],
  vite: {
    plugins: [tailwindcss()],
  },
  typescript: {
    strict: true,
  },
  devtools: {
    enabled: true,
  },
});
