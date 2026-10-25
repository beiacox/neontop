<script lang="ts">
  let { name, sprint }: { name: string; sprint: number } = $props();
</script>

<div class="ph">
  <div class="glitch" data-text={name}>{name}</div>
  <p>Módulo en construcción · llega en el sprint {sprint}</p>
</div>

<style>
  .ph {
    height: 60vh;
    display: grid;
    place-content: center;
    text-align: center;
    gap: 12px;
  }
  .glitch {
    font-family: var(--display);
    font-size: 42px;
    font-weight: 900;
    letter-spacing: 0.2em;
    color: var(--cyan);
    text-shadow:
      2px 0 var(--magenta),
      -2px 0 var(--cyan),
      0 0 24px color-mix(in srgb, var(--cyan) 60%, transparent);
  }
  p {
    color: var(--muted);
    margin: 0;
  }
</style>
