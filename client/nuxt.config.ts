import tailwindcss from "@tailwindcss/vite";

export default defineNuxtConfig({
  srcDir: "app/",
  ssr: false,
  devServer: {
    port: 18081,
  },
  css: ["~/assets/css/main.css"],
  vite: {
    plugins: [tailwindcss()],
    server: {
      strictPort: true,
    },
  },
  typescript: {
    strict: true,
  },
  devtools: {
    enabled: true,
  },
});
