<script lang="ts">
  let { path } = $props();

  let trimmedPath: string = $state("");

  $effect(() => {
    const maxLen: number = 150;
    if (path.length > maxLen) {
      const start = path.length - maxLen;
      trimmedPath = path.substring(start, maxLen);
    } else {
      trimmedPath = path.substring(0, maxLen);
    }
  });
</script>

{trimmedPath}
