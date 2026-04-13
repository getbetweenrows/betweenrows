import DefaultTheme from 'vitepress/theme';
import type { Theme } from 'vitepress';
import { onMounted, watch, nextTick } from 'vue';
import { useRoute } from 'vitepress';
import mediumZoom from 'medium-zoom';
import CopyOrDownloadAsMarkdownButtons from 'vitepress-plugin-llms/vitepress-components/CopyOrDownloadAsMarkdownButtons.vue';
import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import './custom.css';

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component(
      'CopyOrDownloadAsMarkdownButtons',
      CopyOrDownloadAsMarkdownButtons,
    );
  },
  setup() {
    const route = useRoute();
    const initZoom = () => {
      // Click-to-zoom on every image in the main content area.
      // Attach AFTER the DOM is ready; re-attach on route change because
      // VitePress keeps the page shell but swaps content nodes.
      mediumZoom('.vp-doc img', {
        background: 'var(--vp-c-bg)',
        margin: 24,
      });
    };
    onMounted(() => initZoom());
    watch(
      () => route.path,
      () => nextTick(() => initZoom()),
    );
  },
} satisfies Theme;
