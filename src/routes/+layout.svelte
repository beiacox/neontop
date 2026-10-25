<script lang="ts">
  import "@fontsource/orbitron/500.css";
  import "@fontsource/orbitron/700.css";
  import "@fontsource/orbitron/900.css";
  import "@fontsource/jetbrains-mono/400.css";
  import "@fontsource/jetbrains-mono/500.css";
  import "@fontsource/jetbrains-mono/700.css";
  // tipografías de los distintos temas (empaquetadas: la app no depende de internet)
  import "@fontsource/inter/400.css";
  import "@fontsource/inter/500.css";
  import "@fontsource/inter/600.css";
  import "@fontsource/inter/700.css";
  import "@fontsource/space-grotesk/500.css";
  import "@fontsource/space-grotesk/700.css";
  import "@fontsource/vt323/400.css";
  import "@fontsource/lora/400.css";
  import "@fontsource/lora/600.css";
  import "@fontsource/lora/700.css";
  import "@fontsource/ibm-plex-mono/400.css";
  import "@fontsource/ibm-plex-mono/500.css";
  import "../app.css";

  let { children } = $props();
</script>

{@render children()}
