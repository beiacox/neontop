#!/bin/sh
# QA de la capa de FPS: dentro de un KWin anidado con Xwayland (sin tocar el escritorio del
# usuario) lanza un programa con el entorno dado, lee los segmentos /dev/shm/neontop-fps-* y cierra.
# uso: dbus-run-session -- tools/qa-fps.sh <segundos> <VAR=valor…> -- <programa> [args…]
secs=$1; shift
log=${TMPDIR:-/tmp}/neontop-qa
mkdir -p "$log"
run="$log/run-$$.sh"
{
  echo '#!/bin/sh'
  printf 'exec env'
  while [ "$1" != "--" ]; do printf " '%s'" "$1"; shift; done
  shift
  for a in "$@"; do printf " '%s'" "$a"; done
  echo
} > "$run"
chmod +x "$run"
sock=wl-neontop-fps-$$
kwin_wayland --virtual --xwayland --no-lockscreen --no-global-shortcuts --width 1280 --height 800 --socket "$sock" "$run" >"$log/kwin-fps.log" 2>&1 &
kw=$!
sleep "$secs"
for f in /dev/shm/neontop-fps-*; do
  [ -e "$f" ] || continue
  python3 -I -c '
import struct,sys
d=open(sys.argv[1],"rb").read()
magic,=struct.unpack_from("<Q",d,0)
pid,api,cap,app,bits=struct.unpack_from("<5I",d,12)
name=d[32:96].split(b"\0")[0].decode(errors="replace")
count,=struct.unpack_from("<Q",d,104)
print(f"magic={magic==0x3153504654504e4e} api={api} bits={bits} nombre={name} fotogramas={count}")
' "$f"
done
kill $kw 2>/dev/null
sleep 1
rm -f "$run"
