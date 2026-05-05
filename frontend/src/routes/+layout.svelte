<script lang="ts">
  let { children } = $props();
</script>

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto,
      'Helvetica Neue', Arial, sans-serif;
    background: #f5f6fa;
    color: #1f2937;
  }
</style>

{@render children()}
