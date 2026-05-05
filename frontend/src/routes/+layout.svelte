<script lang="ts">
  let { children } = $props();
</script>

<style>
  :global(:root) {
    /* Single accent: AutoPipe teal. Everything else is neutral. */
    --accent: #0f4c5c;
    --accent-hover: #0d3d4a;
    --accent-light: rgba(15, 76, 92, 0.08);

    --bg-page: #f8fafc;
    --bg-card: #ffffff;
    --bg-soft: #f1f5f9;

    --text: #1e293b;
    --text-muted: #64748b;
    --text-faint: #94a3b8;

    --border: #e2e8f0;
    --border-strong: #cbd5e1;

    --danger: #dc2626;
    --danger-bg: #ffffff;
  }

  :global(html, body) {
    margin: 0;
    padding: 0;
    background: var(--bg-page);
    color: var(--text);
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto,
      'Helvetica Neue', Arial, sans-serif;
    -webkit-font-smoothing: antialiased;
  }

  :global(*, *::before, *::after) {
    box-sizing: border-box;
  }

  :global(button) {
    font-family: inherit;
  }
</style>

{@render children()}
