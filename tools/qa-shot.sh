#!/bin/sh
# Captura de pantalla de NeonTop para QA, sin tocar el escritorio del usuario: abre un KWin
# anidado con pantalla virtual en un bus D-Bus privado, lanza el programa dentro y lo captura
# con Spectacle.
#
# uso: dbus-run-session -- tools/qa-shot.sh <salida.png> <ancho> <alto> <espera_s> <programa> [args…]
set -u
out=$1; w=$2; h=$3; wait=$4; shift 4
log=${TMPDIR:-/tmp}/neontop-qa
mkdir -p "$log"
sock=wl-neontop-qa-$$
kwin_wayland --virtual --no-lockscreen --no-global-shortcuts --width "$w" --height "$h" --socket "$sock" >"$log/kwin.log" 2>&1 &
kw=$!
sleep 3
WAYLAND_DISPLAY=$sock "$@" >"$log/app.log" 2>&1 &
app=$!
sleep "$wait"
WAYLAND_DISPLAY=$sock QT_QPA_PLATFORM=wayland spectacle -b -n -f -o "$out" >"$log/spectacle.log" 2>&1
kill $app 2>/dev/null
sleep 0.5
kill $kw 2>/dev/null
wait 2>/dev/null
[ -s "$out" ]
