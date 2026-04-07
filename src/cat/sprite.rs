// cat/sprite.rs — Décode un PNG, applique le tinting HSB, retourne BGRA prémultiplié.
// Port direct de l'algorithme Swift de cat.swift.

use std::path::Path;

use crate::config::CatColorDef;

// ── API publique ──────────────────────────────────────────────────────────────

/// Charge un PNG et retourne les pixels BGRA prémultipliés à l'échelle demandée.
/// `(Vec<u8>, width, height)` — width*height*4 octets.
pub fn load_sprite_bgra(
    path: &Path,
    color: &CatColorDef,
    scale: f32,
) -> Result<(Vec<u8>, u32, u32), String> {
    let img = image::open(path)
        .map_err(|e| format!("Impossible de charger {}: {e}", path.display()))?
        .into_rgba8();

    let (src_w, src_h) = img.dimensions();

    // Échantillonnage nearest-neighbor
    let dst_w = ((src_w as f32) * scale).round().max(1.0) as u32;
    let dst_h = ((src_h as f32) * scale).round().max(1.0) as u32;

    // Si orange (aucun tinting, hue_shift=0 sat_mul=1 bri_off=0) ET scale==1 → chemin rapide
    let no_tint = (color.hue_shift.abs() < 1e-6)
        && ((color.sat_mul - 1.0).abs() < 1e-6)
        && (color.bri_off.abs() < 1e-6);

    let src_pixels = img.as_raw();

    if no_tint && dst_w == src_w && dst_h == src_h {
        let bgra = rgba_to_bgra_premul(src_pixels);
        return Ok((bgra, dst_w, dst_h));
    }

    // ── Tinting RGBA ─────────────────────────────────────────────────────────
    let tinted_rgba = if no_tint {
        src_pixels.to_vec()
    } else {
        apply_tint(src_pixels, src_w, src_h, color)
    };

    // ── Scale nearest-neighbor ───────────────────────────────────────────────
    let scaled = if dst_w == src_w && dst_h == src_h {
        tinted_rgba
    } else {
        scale_nearest(&tinted_rgba, src_w, src_h, dst_w, dst_h)
    };

    // ── RGBA → BGRA prémultiplié ─────────────────────────────────────────────
    let bgra = rgba_to_bgra_premul(&scaled);
    Ok((bgra, dst_w, dst_h))
}

// ── Conversion finale ─────────────────────────────────────────────────────────

fn rgba_to_bgra_premul(rgba: &[u8]) -> Vec<u8> {
    let n = rgba.len();
    let mut out = vec![0u8; n];
    let mut i = 0;
    while i + 3 < n {
        let r = rgba[i] as u32;
        let g = rgba[i + 1] as u32;
        let b = rgba[i + 2] as u32;
        let a = rgba[i + 3] as u32;
        out[i] = ((b * a / 255) as u8);     // B prémultiplié
        out[i + 1] = ((g * a / 255) as u8); // G prémultiplié
        out[i + 2] = ((r * a / 255) as u8); // R prémultiplié
        out[i + 3] = a as u8;               // A
        i += 4;
    }
    out
}

// ── Tinting HSB ───────────────────────────────────────────────────────────────

fn apply_tint(rgba: &[u8], w: u32, h: u32, color: &CatColorDef) -> Vec<u8> {
    let n = rgba.len();
    let mut out = rgba.to_vec();
    let mut i = 0;
    while i + 3 < n {
        let a = rgba[i + 3];
        if a == 0 {
            i += 4;
            continue;
        }

        // 1. Dépremultiplication (les PNGs source ne sont PAS prémultipliés)
        let r = rgba[i] as f32 / 255.0;
        let g = rgba[i + 1] as f32 / 255.0;
        let b = rgba[i + 2] as f32 / 255.0;

        // 2. RGB → HSB
        let (mut h, mut s, mut bri) = rgb_to_hsb(r, g, b);

        // 3. Appliquer tinting
        h = (h + color.hue_shift).rem_euclid(1.0);
        s = (s * color.sat_mul).clamp(0.0, 1.0);
        bri = (bri + color.bri_off).clamp(0.0, 1.0);

        // 4. HSB → RGB
        let (nr, ng, nb) = hsb_to_rgb(h, s, bri);

        out[i] = (nr * 255.0).round() as u8;
        out[i + 1] = (ng * 255.0).round() as u8;
        out[i + 2] = (nb * 255.0).round() as u8;
        // alpha inchangé
        i += 4;
    }
    out
}

fn rgb_to_hsb(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let delta = mx - mn;

    let bri = mx;
    let s = if mx < 1e-6 { 0.0 } else { delta / mx };

    let h = if delta < 1e-4 {
        0.0
    } else if mx == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if mx == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };

    (h, s, bri)
}

fn hsb_to_rgb(h: f32, s: f32, b: f32) -> (f32, f32, f32) {
    if s < 1e-6 {
        return (b, b, b);
    }
    let h6 = h * 6.0;
    let i = h6.floor() as i32;
    let f = h6 - h6.floor();
    let p = b * (1.0 - s);
    let q = b * (1.0 - s * f);
    let t = b * (1.0 - s * (1.0 - f));
    match i % 6 {
        0 => (b, t, p),
        1 => (q, b, p),
        2 => (p, b, t),
        3 => (p, q, b),
        4 => (t, p, b),
        _ => (b, p, q),
    }
}

// ── Scale nearest-neighbor ────────────────────────────────────────────────────

fn scale_nearest(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    for dy in 0..dh {
        let sy = (dy as f32 * sh as f32 / dh as f32) as u32;
        let sy = sy.min(sh - 1);
        for dx in 0..dw {
            let sx = (dx as f32 * sw as f32 / dw as f32) as u32;
            let sx = sx.min(sw - 1);
            let src_i = ((sy * sw + sx) * 4) as usize;
            let dst_i = ((dy * dw + dx) * 4) as usize;
            out[dst_i..dst_i + 4].copy_from_slice(&src[src_i..src_i + 4]);
        }
    }
    out
}
