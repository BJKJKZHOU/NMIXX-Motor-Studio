<script lang="ts">
  import { uiLanguage, translate, translateFormat } from "../i18n";
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
{:else if error}<div class="error-text">{translateFormat("automation.load.failed", $uiLanguage, { reason: error })}</div>
{:else}<div class="empty-state">{translate("automation.loading", $uiLanguage)}</div>{/if}
