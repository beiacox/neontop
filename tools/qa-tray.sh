#!/bin/sh
# QA de la bandeja del sistema, en un KWin anidado con su propio bus D-Bus (no toca tu escritorio):
#  1. arranca el servicio de bandeja de KDE (kded6) y NeonTop aislado (config y socket propios);
#  2. cierra la ventana con un script de KWin (como pulsar ✕);
#  3. comprueba que el proceso sigue vivo y con su icono en la bandeja;
#  4. activa el icono por D-Bus (como hacer clic) y comprueba que la ventana vuelve.
# uso: dbus-run-session -- tools/qa-tray.sh <binario> <true|false (cerrar a la bandeja)>
set -u
bin=$1; to_tray=$2
log=${TMPDIR:-/tmp}/neontop-qa; mkdir -p "$log"
q=$(mktemp -d)
mkdir -p "$q/config/com.neontop.app" "$q/data"
echo "{\"fpsAuto\": false, \"ui\": {\"closeToTray\": $to_tray}}" > "$q/config/com.neontop.app/config.json"
cat > "$q/close.js" <<'JS'
for (const w of workspace.windowList()) { if (w.resourceClass === "neontop") { w.closeWindow(); } }
JS
sock=wl-neontop-tray-$$
kded6 >"$log/kded.log" 2>&1 &
kd=$!
kwin_wayland --virtual --no-lockscreen --width 1280 --height 800 --socket "$sock" >"$log/kwin-tray.log" 2>&1 &
kw=$!
sleep 3
WAYLAND_DISPLAY=$sock XDG_CONFIG_HOME="$q/config" XDG_DATA_HOME="$q/data" NEONTOP_SOCKET="$q/neontop.sock" NEONTOP_SIZE=1000x700 "$bin" >"$log/app-tray.log" 2>&1 &
app=$!
sleep 5
script() { # ejecuta un script de KWin
  id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$1" "qa$2$$")
  qdbus6 org.kde.KWin "/Scripting/Script$id" org.kde.kwin.Script.run >/dev/null
  sleep 1
  qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "qa$2$$" >/dev/null
}
# ventana visible: el escritorio anidado es negro; con la ventana, la captura tiene contenido
windows() {
  WAYLAND_DISPLAY=$sock QT_QPA_PLATFORM=wayland spectacle -b -n -f -o "$q/s$1.png" >/dev/null 2>&1
  m=$(magick "$q/s$1.png" -colorspace gray -format "%[fx:mean]" info: 2>/dev/null)
  python3 -I -c "import sys; print('sí' if float(sys.argv[1] or 0) > 0.01 else 'no')" "${m:-0}"
}
tray() { busctl --user get-property org.kde.StatusNotifierWatcher /StatusNotifierWatcher org.kde.StatusNotifierWatcher RegisteredStatusNotifierItems 2>/dev/null | tr ' ' '\n' | grep -c "StatusNotifierItem-$app-"; }
alive() { kill -0 $app 2>/dev/null && echo sí || echo no; }
echo "inicio:        vivo=$(alive) ventana=$(windows 1) icono=$(tray)"
script "$q/close.js" x
sleep 2
echo "tras cerrar:   vivo=$(alive) ventana=$(windows 2) icono=$(tray)"
if kill -0 $app 2>/dev/null; then
  svc=$(busctl --user get-property org.kde.StatusNotifierWatcher /StatusNotifierWatcher org.kde.StatusNotifierWatcher RegisteredStatusNotifierItems | tr ' ' '\n' | grep "StatusNotifierItem-$app-" | tr -d '"' | cut -d/ -f1)
  busctl --user call "$svc" /StatusNotifierItem org.kde.StatusNotifierItem Activate ii 0 0
  sleep 3
  echo "tras el icono: vivo=$(alive) ventana=$(windows 3) icono=$(tray)"
  # otra vez a la bandeja y abrir NeonTop de nuevo (instancia única: muestra la ventana existente)
  script "$q/close.js" y
  sleep 2
  WAYLAND_DISPLAY=$sock XDG_CONFIG_HOME="$q/config" XDG_DATA_HOME="$q/data" NEONTOP_SOCKET="$q/neontop.sock" "$bin" >/dev/null 2>&1
  sleep 3
  echo "reabrir:       vivo=$(alive) ventana=$(windows 4) icono=$(tray) procesos=$(pgrep -fc "^$bin")"
fi
kill $app 2>/dev/null; sleep 1; kill $kw $kd 2>/dev/null
wait 2>/dev/null
rm -rf "$q"
