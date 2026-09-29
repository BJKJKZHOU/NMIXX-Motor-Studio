<script lang="ts">
  import { onMount } from "svelte";
  export let connected = false;
  let page: typeof import("./AutomationPage.svelte").default | undefined;
  let error = "";
  onMount(() => {
    let disposed = false;
    import("./AutomationPage.svelte").then((module) => { if (!disposed) page = module.default; })
      .catch((failure) => { if (!disposed) error = String(failure); });
    return () => { disposed = true; };
  });
</script>
{#if page}<svelte:component this={page} {connected} />
{:else if error}<div class="error-text">Automation could not load: {error}</div>
{:else}<div class="empty-state">Loading Automation…</div>{/if}
