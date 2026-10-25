<script lang="ts">
  import Sparkline from "$lib/charts/Sparkline.svelte";
  import { C } from "$lib/theme";
  import type { SeriesLike } from "$lib/series";

  interface Props {
    label: string;
    value: string;
    color?: string;
    sub?: string;
    history?: SeriesLike;
  }
  let { label, value, color = C.cyan, sub, history }: Props = $props();
</script>

<div class="stat" style:--c={color}>
  <div class="label">{label}</div>
  <div class="value">{value}</div>
  {#if sub}<div class="sub">{sub}</div>{/if}
  {#if history}<div class="spark"><Sparkline values={history} {color} height={26} /></div>{/if}
</div>

<style>
  .stat {
    border-left: 2px solid var(--c);
    padding: 4px 0 2px 10px;
    min-width: 0;
    background: linear-gradient(90deg, color-mix(in srgb, var(--c) 8%, transparent), transparent 70%);
  }
  .label {
    font-size: 10px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.12em;
  }
  .value {
    font-family: var(--display);
    font-size: 18px;
    font-weight: 700;
    color: var(--c);
    text-shadow: 0 0 10px color-mix(in srgb, var(--c) 60%, transparent);
    white-space: nowrap;
  }
  .sub {
    font-size: 10px;
    color: var(--muted);
  }
  .spark {
    margin-top: 2px;
  }
</style>
