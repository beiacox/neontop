#!/usr/bin/env python3
"""Lista los textos traducibles (t!("…") y n!("…")) del código, uno por línea, sin repetir.
Uso: tools/i18n-keys.py [--missing xx]  → con --missing, solo los que faltan en locales/xx.tsv."""
import glob, re, sys, os
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
pat = re.compile(r'(?<![A-Za-z_])[tn]!\(\s*"((?:[^"\\]|\\.)*)"')
keys = []
for p in sorted(glob.glob(f'{root}/crates/*/src/**/*.rs', recursive=True)):
    src = open(p, encoding='utf-8').read()
    cut = src.find('#[cfg(test)]')
    if cut >= 0:
        src = src[:cut]
    src = '\n'.join(l for l in src.split('\n') if not l.lstrip().startswith('//'))
    for m in pat.finditer(src):
        k = m.group(1).encode().decode('unicode_escape').encode('latin-1').decode('utf-8') if '\\' in m.group(1) else m.group(1)
        if k not in keys:
            keys.append(k)
if '--missing' in sys.argv:
    lang = sys.argv[sys.argv.index('--missing') + 1]
    have = set()
    for l in open(f'{root}/crates/neontop-core/locales/{lang}.tsv', encoding='utf-8'):
        if l.strip() and not l.startswith('#') and '\t' in l:
            have.add(l.split('\t', 1)[0])
    keys = [k for k in keys if k.replace('\n', '\\n') not in have]
for k in keys:
    print(k.replace('\n', '\\n'))
