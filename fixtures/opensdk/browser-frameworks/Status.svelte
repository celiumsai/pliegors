<script>
  export let message = "Svelte";
  export let motion = "unknown";
</script>

<p data-framework="svelte" data-motion={motion}>{message}</p>
