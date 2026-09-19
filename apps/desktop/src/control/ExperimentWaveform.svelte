<script lang="ts">
  import type { ScopeChannel } from "../analysis/scope/types";
  import type { TuningExperimentSnapshot } from "./tuningExperiment";
  import WaveformGroup from "./WaveformGroup.svelte";

  export let result: TuningExperimentSnapshot | undefined;

  type Group = {
    title: string;
    unit: string;
    channels: ScopeChannel[];
  };

  $: groups = buildGroups(result?.config.channels ?? []);

  function buildGroups(channels: ScopeChannel[]): Group[] {
    const ordered = [
      { unit: "A", title: "Current" },
      { unit: "rad/s", title: "Speed" },
      { unit: "turn", title: "Position" },
    ];

    const groups: Group[] = [];
    for (const spec of ordered) {
      const matched = channels.filter((channel) => (channel.unit ?? "") === spec.unit);
      if (matched.length) groups.push({ ...spec, channels: matched });
    }

    const known = new Set(ordered.map((item) => item.unit));
    for (const channel of channels) {
      const unit = channel.unit ?? "";
      if (known.has(unit)) continue;
      let group = groups.find((item) => item.unit === unit);
      if (!group) {
        group = { title: unit || "Other", unit, channels: [] };
        groups.push(group);
      }
      group.channels.push(channel);
    }

    return groups;
  }
</script>

{#if !result}
  <div class="waveform-empty">
    <i class="codicon codicon-graph-line"></i>
    <div>Run an experiment to capture the tuning response.</div>
    <small>Iq Ref / Iq use FAST; motion references and feedback use NORMAL.</small>
  </div>
{:else}
  <div class="waveform-groups">
    {#each groups as group (group.unit)}
      <WaveformGroup
        title={group.title}
        unit={group.unit}
        channels={group.channels}
        series={result.snapshot.series}
      />
    {/each}
  </div>
{/if}

<style>
  .waveform-empty {
    min-height: 360px;
    display: grid;
    place-items: center;
    align-content: center;
    gap: 8px;
    color: var(--vscode-descriptionForeground);
    border: 1px dashed var(--vscode-panel-border);
    text-align: center;
  }

  .waveform-empty .codicon {
    font-size: 24px;
  }

  .waveform-empty small {
    font-size: 10px;
  }

  .waveform-groups {
    display: grid;
    gap: 10px;
  }
</style>
