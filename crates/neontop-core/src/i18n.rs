//! Idiomas de la interfaz. El texto original (y la clave de traducción) es el español: cada texto
//! visible pasa por `t!("…")`, que devuelve la traducción al idioma activo o el original si no la
//! hay. Las traducciones están en `locales/<código>.tsv` (una línea por texto: original, tabulador,
//! traducción) y van dentro del binario.
//!
//! Con argumentos, `t!("Hace {} min", n)` sustituye cada `{}` en orden; las cifras se formatean
//! antes (`t!("{} W", format!("{w:.0}"))`). Una traducción puede reordenarlos con `{0}`, `{1}`…

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    Es,
    En,
    Pt,
    Fr,
    De,
}

/// Idiomas disponibles: (idioma, código, nombre en su propio idioma).
pub const LANGS: [(Lang, &str, &str); 5] = [
    (Lang::Es, "es", "Español"),
    (Lang::En, "en", "English"),
    (Lang::Pt, "pt", "Português"),
    (Lang::Fr, "fr", "Français"),
    (Lang::De, "de", "Deutsch"),
];

impl Lang {
    pub fn code(self) -> &'static str {
        LANGS.iter().find(|l| l.0 == self).map(|l| l.1).unwrap_or("es")
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        let c = code.trim().to_lowercase();
        let c = c.split(['_', '-', '.', '@']).next().unwrap_or("");
        LANGS.iter().find(|l| l.1 == c).map(|l| l.0)
    }

    fn index(self) -> u8 {
        LANGS.iter().position(|l| l.0 == self).unwrap_or(0) as u8
    }

    fn table(self) -> &'static str {
        match self {
            Lang::Es => "",
            Lang::En => include_str!("../locales/en.tsv"),
            Lang::Pt => include_str!("../locales/pt.tsv"),
            Lang::Fr => include_str!("../locales/fr.tsv"),
            Lang::De => include_str!("../locales/de.tsv"),
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);
static TABLES: [OnceLock<HashMap<&'static str, &'static str>>; 5] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];

/// Interpreta un fichero de traducciones: `original<TAB>traducción` por línea; `\n` y `\t`
/// escritos así dentro del texto. Las líneas vacías y las que empiezan por `#` se ignoran.
pub fn parse(table: &'static str) -> HashMap<&'static str, &'static str> {
    table
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .filter(|(_, t)| !t.is_empty())
        .collect()
}

fn table(lang: Lang) -> &'static HashMap<&'static str, &'static str> {
    TABLES[lang.index() as usize].get_or_init(|| parse(lang.table()))
}

pub fn set(lang: Lang) {
    CURRENT.store(lang.index(), Ordering::Relaxed);
}

pub fn current() -> Lang {
    LANGS[CURRENT.load(Ordering::Relaxed) as usize % LANGS.len()].0
}

/// Idioma del sistema (LANGUAGE, LC_ALL, LC_MESSAGES, LANG); español si no se reconoce.
pub fn system() -> Lang {
    for var in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            // LANGUAGE puede ser una lista: "fr:en"
            for part in v.split(':') {
                if let Some(l) = Lang::from_code(part) {
                    return l;
                }
            }
        }
    }
    Lang::Es
}

/// Elige el idioma guardado en los ajustes (código) o, si no hay, el del sistema.
pub fn init(saved: Option<&str>) {
    set(saved.and_then(Lang::from_code).unwrap_or_else(system));
}

/// Traducción de `s` al idioma activo (o `s` si no hay).
pub fn t(s: &str) -> &str {
    let lang = current();
    if lang == Lang::Es {
        return s;
    }
    table(lang).get(s).copied().unwrap_or(s)
}

/// Traducción con argumentos: cada `{}` se sustituye en orden; `{0}`, `{1}`… por posición.
pub fn tr(template: &str, args: &[&dyn Display]) -> String {
    fill(t(template), args)
}

pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0;
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) if after[..j].chars().all(|c| c.is_ascii_digit()) => {
                let idx = if j == 0 {
                    next += 1;
                    next - 1
                } else {
                    after[..j].parse().unwrap_or(0)
                };
                if let Some(a) = args.get(idx) {
                    out.push_str(&a.to_string());
                }
                rest = &after[j + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Número de huecos `{}`/`{n}` de un texto.
pub fn placeholders(s: &str) -> usize {
    let mut n = 0;
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) if after[..j].chars().all(|c| c.is_ascii_digit()) => {
                n += 1;
                rest = &after[j + 1..];
            }
            _ => rest = after,
        }
    }
    n
}

/// Marca un texto para traducir sin traducirlo aún (tablas `const`): se traduce donde se muestra
/// con `i18n::t(texto)`. La prueba de cobertura también recoge estos textos.
#[macro_export]
macro_rules! n {
    ($s:literal) => {
        $s
    };
}

/// `t!("texto")` traduce; `t!("texto {}", arg, …)` traduce y rellena.
#[macro_export]
macro_rules! t {
    ($s:literal) => {
        $crate::i18n::t($s)
    };
    ($s:literal, $($a:expr),+ $(,)?) => {
        $crate::i18n::tr($s, &[$(&$a as &dyn ::std::fmt::Display),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_and_reorder() {
        assert_eq!(fill("Hace {} min", &[&5]), "Hace 5 min");
        assert_eq!(fill("{} de {}", &[&"a", &"b"]), "a de b");
        assert_eq!(fill("{1} of {0}", &[&"a", &"b"]), "b of a");
        assert_eq!(fill("sin huecos", &[]), "sin huecos");
        assert_eq!(fill("llave {suelta", &[]), "llave {suelta");
        assert_eq!(placeholders("{} y {1} pero {x}"), 2);
    }

    #[test]
    fn codes_and_system() {
        assert_eq!(Lang::from_code("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::from_code("pt-BR"), Some(Lang::Pt));
        assert_eq!(Lang::from_code("de"), Some(Lang::De));
        assert_eq!(Lang::from_code("ja_JP"), None);
        for (l, c, _) in LANGS {
            assert_eq!(Lang::from_code(c), Some(l));
            assert_eq!(l.code(), c);
        }
    }

    /// Textos `t!("…")` / `n!("…")` del código de los crates (sin pruebas ni comentarios).
    fn source_keys() -> Vec<String> {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut files = Vec::new();
        for c in ["neontop-core", "neontop"] {
            walk(&crates.join(c).join("src"), &mut files);
        }
        assert!(files.len() > 20, "no se encuentra el código fuente");
        let mut keys: Vec<String> = Vec::new();
        for f in files {
            let src = std::fs::read_to_string(&f).unwrap();
            let src = src.split("#[cfg(test)]").next().unwrap();
            for line in src.lines().filter(|l| !l.trim_start().starts_with("//")) {
                let mut rest = line;
                while let Some(i) = rest.find("!(\"") {
                    let before = rest[..i].chars().next_back();
                    let pre = rest[..i].chars().rev().nth(1);
                    rest = &rest[i + 3..];
                    if !matches!(before, Some('t' | 'n')) || pre.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let mut key = String::new();
                    let mut chars = rest.char_indices();
                    while let Some((j, c)) = chars.next() {
                        match c {
                            '\\' => match chars.next() {
                                Some((_, 'n')) => key.push('\n'),
                                Some((_, o)) => key.push(o),
                                None => {}
                            },
                            '"' => {
                                rest = &rest[j + 1..];
                                break;
                            }
                            c => key.push(c),
                        }
                    }
                    if !keys.contains(&key) {
                        keys.push(key);
                    }
                }
            }
        }
        keys
    }

    #[test]
    fn every_text_is_translated() {
        let keys = source_keys();
        assert!(keys.len() > 500, "pocos textos encontrados: {}", keys.len());
        assert!(keys.iter().any(|k| k == "Memoria") && keys.iter().any(|k| k == "Ejecutando"));
        for (lang, code, _) in LANGS.into_iter().skip(1) {
            let tab = table(lang);
            let missing: Vec<&String> = keys.iter().filter(|k| !tab.contains_key(k.as_str())).collect();
            assert!(missing.is_empty(), "faltan {} textos en {code}.tsv: {:?}", missing.len(), &missing[..missing.len().min(10)]);
            for (k, v) in tab {
                assert_eq!(placeholders(k), placeholders(v), "{code}: huecos distintos en «{k}» → «{v}»");
            }
            let unused: Vec<&&str> = tab.keys().filter(|k| !keys.iter().any(|x| x == *k)).collect();
            assert!(unused.is_empty(), "{code}.tsv tiene textos que ya no existen: {:?}", unused);
        }
    }

    #[test]
    fn tables_parse_and_switch() {
        assert!(parse("# comentario\nHola\tHello\n\nVacío\t\n").len() == 1);
        // sin cambiar el idioma global (las pruebas corren en paralelo y comprueban textos en español)
        assert_eq!(table(Lang::En).get("Memoria"), Some(&"Memory"));
        assert_eq!(t("texto sin traducir xyz"), "texto sin traducir xyz");
    }
}
