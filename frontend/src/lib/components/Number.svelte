<script lang="ts">
  let { number } = $props();

  let fomattedNumber: string = $state("");

  $effect(() => {
    const kb: number = 1024;
    const mb: number = kb * kb;
    const gb: number = mb * mb;
    if (number > gb) {
      fomattedNumber = `${(number / gb).toFixed(2)} GB`;
    } else if (number > mb) {
      fomattedNumber = `${(number / mb).toFixed(2)} MB`;
    } else if (number > kb) {
      fomattedNumber = `${(number / kb).toFixed(2)} KB`;
    } else {
      fomattedNumber = `${number} B`;
    }
  });
</script>

<span>{fomattedNumber}</span>

<style>
  span {
    font-variant-numeric: tabular-nums;
  }
</style>
