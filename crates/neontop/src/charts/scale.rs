//! Escalas de los ejes: máximos "redondos" y marcas.

/// Máximo redondeado hacia arriba a 1, 2, 2,5 o 5 × 10ⁿ (para que el eje tenga cifras limpias).
pub fn nice_max(v: f32) -> f32 {
    if !v.is_finite() || v <= 0.0 {
        return 1.0;
    }
    let exp = v.log10().floor();
    let base = 10f32.powf(exp);
    let f = v / base;
    let nice = if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 2.5 {
        2.5
    } else if f <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * base
}

/// Como `nice_max` pero en unidades binarias (KiB, MiB, GiB…): el eje de bytes queda en
/// 256 MiB, 512 MiB, 1 GiB… en lugar de 954 MiB.
pub fn nice_max_binary(v: f32) -> f32 {
    if !v.is_finite() || v <= 0.0 {
        return 1.0;
    }
    let mut unit = 1.0f32;
    while v / unit >= 1024.0 {
        unit *= 1024.0;
    }
    let x = v / unit;
    // potencias de dos para que los cuartos también sean cifras redondas
    let mut n = 1.0f32;
    while n < x {
        n *= 2.0;
    }
    n * unit
}

/// `n + 1` marcas equiespaciadas de `min` a `max`.
pub fn ticks(min: f32, max: f32, n: usize) -> Vec<f32> {
    let n = n.max(1);
    (0..=n).map(|i| min + (max - min) * i as f32 / n as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice() {
        assert_eq!(nice_max(0.0), 1.0);
        assert_eq!(nice_max(-3.0), 1.0);
        assert_eq!(nice_max(f32::NAN), 1.0);
        assert_eq!(nice_max(1.0), 1.0);
        assert_eq!(nice_max(1.3), 2.0);
        assert_eq!(nice_max(2.2), 2.5);
        assert_eq!(nice_max(3.0), 5.0);
        assert_eq!(nice_max(7.0), 10.0);
        assert!((nice_max(730.0) - 1000.0).abs() < 1e-3);
        assert!((nice_max(0.042) - 0.05).abs() < 1e-6);
    }

    #[test]
    fn nice_binary() {
        let mib = 1024.0 * 1024.0;
        assert_eq!(nice_max_binary(900.0 * mib), 1024.0 * mib);
        assert_eq!(nice_max_binary(300.0 * mib), 512.0 * mib);
        assert_eq!(nice_max_binary(3.0 * 1024.0 * mib), 4.0 * 1024.0 * mib);
        assert_eq!(nice_max_binary(0.0), 1.0);
    }

    #[test]
    fn tick_marks() {
        assert_eq!(ticks(0.0, 100.0, 4), vec![0.0, 25.0, 50.0, 75.0, 100.0]);
        assert_eq!(ticks(0.0, 1.0, 0).len(), 2);
    }
}
