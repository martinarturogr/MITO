//! Pruebas de las insignias: geometría contra imágenes generadas con el `icons.py`
//! original (Pillow 12), colores exactos, lectura de DIB y caché.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::*;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn reference() -> Value {
    let text = std::fs::read_to_string(fixtures().join("badge-reference.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn fixture(name: &str) -> Image {
    let bytes = std::fs::read(fixtures().join(name)).unwrap();
    decode_png(&bytes).unwrap()
}

const GRAY: [u8; 4] = [128, 128, 128, 255];

/// Caja de los píxeles que cumplen `test`.
fn bounds(image: &Image, test: impl Fn([u8; 4]) -> bool) -> Option<[i64; 4]> {
    let mut found: Option<[i64; 4]> = None;
    for y in 0..image.height {
        for x in 0..image.width {
            if test(image.pixel(x, y)) {
                let (px, py) = (i64::from(x), i64::from(y));
                found = Some(match found {
                    None => [px, py, px, py],
                    Some([x0, y0, x1, y1]) => [x0.min(px), y0.min(py), x1.max(px), y1.max(py)],
                });
            }
        }
    }
    found
}

fn numbers(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_i64().unwrap())
        .collect()
}

#[test]
fn la_caja_del_texto_coincide_con_pillow() {
    let reference = reference();
    let font = font().unwrap();
    let mut worst = 0;
    for size in 4..70_i64 {
        for text in ["MITO", "DEV", "A"] {
            let expected = numbers(&reference["fonts"][size.to_string()][text]);
            let layout = TextLayout::new(font, size, text);
            let actual = layout.bbox;
            for (a, e) in actual.iter().zip(&expected) {
                worst = worst.max((a - e).abs());
            }
            assert!(
                actual
                    .iter()
                    .zip(&expected)
                    .all(|(a, e)| (a - e).abs() <= 2),
                "{text} a {size}: {actual:?} frente a {expected:?}"
            );
        }
    }
    // El rasterizado no es el de FreeType, pero la caja se queda a 2 píxeles.
    assert!(worst <= 2, "diferencia máxima {worst}");
}

#[test]
fn la_insignia_ocupa_la_misma_caja_que_en_pillow() {
    let reference = reference();
    for size in [16_u32, 32, 48, 64, 256] {
        for (text, color) in [
            ("MITO", "naranja"),
            ("DEV", "naranja"),
            ("A", "naranja"),
            ("MITO", "azul"),
        ] {
            let name = format!("badge-{size}-{text}-{color}.png");
            let expected = fixture(&name);
            let rgb = badge_rgb(color).unwrap();
            let actual = add_mito_badge(&Image::solid(size, size, GRAY), text, rgb).unwrap();
            let want = numbers(&reference[name.as_str()]["badge"]);
            let got = bounds(&actual, |p| p != GRAY).unwrap();
            for (g, w) in got.iter().zip(&want) {
                assert!((g - w).abs() <= 2, "{name}: caja {got:?} frente a {want:?}");
            }
            assert_eq!(bounds(&expected, |p| p != GRAY).unwrap().len(), 4, "{name}");
            // El fondo del icono fuera de la insignia no cambia.
            assert_eq!(actual.pixel(0, 0), GRAY, "{name}");
        }
    }
}

#[test]
fn el_texto_pequeno_es_blanco_y_cae_donde_pillow_lo_pone() {
    let reference = reference();
    for size in [16_u32, 32, 48] {
        for text in ["MITO", "DEV", "A"] {
            let name = format!("badge-{size}-{text}-naranja.png");
            let rgb = badge_rgb("naranja").unwrap();
            let actual = add_mito_badge(&Image::solid(size, size, GRAY), text, rgb).unwrap();
            let want = numbers(&reference[name.as_str()]["white"]);
            let white = |p: [u8; 4]| p == [255, 255, 255, 255];
            let got = bounds(&actual, white).unwrap();
            for (g, w) in got.iter().zip(&want) {
                assert!(
                    (g - w).abs() <= 2,
                    "{name}: texto {got:?} frente a {want:?}"
                );
            }
        }
    }
}

#[test]
fn el_fondo_de_la_insignia_pequena_es_el_color_exacto_y_hay_blanco() {
    for (name, rgb) in BADGE_COLORS {
        let image = add_mito_badge(&Image::solid(32, 32, [90, 90, 90, 255]), "MITO", rgb).unwrap();
        assert_eq!(image.pixel(31, 31), [rgb[0], rgb[1], rgb[2], 255], "{name}");
        assert_eq!(image.pixel(0, 0), [90, 90, 90, 255]);
        assert!(
            image
                .data
                .as_chunks::<4>()
                .0
                .contains(&[255, 255, 255, 255]),
            "{name}: sin texto blanco"
        );
    }
}

#[test]
fn la_insignia_grande_mezcla_el_color_y_dibuja_el_contorno() {
    let rgb = badge_rgb("azul").unwrap();
    let image = add_mito_badge(&Image::solid(256, 256, [200, 200, 200, 255]), "MITO", rgb).unwrap();
    // Borde inferior: contorno blanco con alfa 245 sobre gris.
    let edge = image.pixel(200, 255);
    assert!(edge[0] > 240 && edge[1] > 240 && edge[2] > 240, "{edge:?}");
    // La esquina está redondeada: el fondo del icono sigue ahí.
    assert_eq!(image.pixel(255, 255), [200, 200, 200, 255]);
    // Un píxel de relleno (fuera del texto) mezcla el azul con alfa 244.
    let fill = image.pixel(100, 252);
    let expected = |c: u8, base: u8| {
        (f32::from(c) * 244.0 / 255.0 + f32::from(base) * 11.0 / 255.0).round() as i16
    };
    assert!(
        (i16::from(fill[2]) - expected(rgb[2], 200)).abs() <= 2,
        "{fill:?}"
    );
    assert_eq!(fill[3], 255);
    assert_eq!(image.pixel(0, 0), [200, 200, 200, 255]);
}

#[test]
fn la_insignia_de_cuatro_letras_en_16_px_no_rompe() {
    let image = add_mito_badge(&Image::solid(16, 16, GRAY), "MITO", [196, 80, 19]).unwrap();
    assert_eq!(image.pixel(15, 15), [196, 80, 19, 255]);
}

// ---------------------------------------------------------------------------
// ICO
// ---------------------------------------------------------------------------

#[test]
fn el_ico_generado_conserva_los_tamanos_y_se_vuelve_a_leer() {
    let frames = [
        Image::solid(256, 256, [10, 20, 200, 255]),
        Image::solid(16, 16, [10, 20, 200, 255]),
        Image::solid(32, 32, [10, 20, 200, 255]),
        Image::solid(32, 32, [1, 2, 3, 255]),
    ];
    let bytes = encode_png_ico(&frames).unwrap();
    let entries = parse_directory(&bytes).unwrap();
    let sizes: Vec<_> = entries.iter().map(|e| (e.width, e.height)).collect();
    assert_eq!(sizes, [(16, 16), (32, 32), (256, 256)]);
    // Gana el último de cada tamaño.
    let loaded = load_icon_frames(&bytes, Path::new("x.ico")).unwrap();
    assert_eq!(loaded.len(), 3);
    assert_eq!(loaded[1].pixel(5, 5), [1, 2, 3, 255]);
    assert_eq!(loaded[2].pixel(255, 255), [10, 20, 200, 255]);
    // Entrada de 256: ancho y alto 0, un plano y 32 bits.
    assert_eq!(&bytes[6 + 32..6 + 32 + 6], &[0, 0, 0, 0, 1, 0]);
    assert_eq!(&bytes[6 + 32 + 6..6 + 32 + 8], &[32, 0]);
    assert!(encode_png_ico(&[]).is_err());
}

/// `.ico` con un solo frame DIB.
fn dib_icon(
    width: usize,
    height: usize,
    bits: u16,
    colors: &[[u8; 3]],
    pixels: &[u8],
    mask: &[u8],
) -> Vec<u8> {
    let mut dib = Vec::new();
    dib.extend_from_slice(&40_u32.to_le_bytes());
    dib.extend_from_slice(&(width as u32).to_le_bytes());
    dib.extend_from_slice(&((height * 2) as u32).to_le_bytes());
    dib.extend_from_slice(&1_u16.to_le_bytes());
    dib.extend_from_slice(&bits.to_le_bytes());
    dib.extend_from_slice(&[0; 4]);
    dib.extend_from_slice(&[0; 4]);
    dib.extend_from_slice(&[0; 8]);
    dib.extend_from_slice(&(colors.len() as u32).to_le_bytes());
    dib.extend_from_slice(&[0; 4]);
    for [r, g, b] in colors {
        dib.extend_from_slice(&[*b, *g, *r, 0]);
    }
    dib.extend_from_slice(pixels);
    dib.extend_from_slice(mask);
    let mut out = vec![
        0,
        0,
        1,
        0,
        1,
        0,
        width as u8,
        height as u8,
        0,
        0,
        1,
        0,
        bits as u8,
        0,
    ];
    out.extend_from_slice(&(dib.len() as u32).to_le_bytes());
    out.extend_from_slice(&22_u32.to_le_bytes());
    out.extend_from_slice(&dib);
    out
}

#[test]
fn lee_dib_de_32_bits_con_alfa() {
    // 2x2; filas de abajo arriba: (BGRA).
    let pixels = [
        1, 2, 3, 40, 4, 5, 6, 255, // fila inferior
        7, 8, 9, 128, 10, 11, 12, 0, // fila superior
    ];
    let icon = dib_icon(2, 2, 32, &[], &pixels, &[0; 8]);
    let frames = load_icon_frames(&icon, Path::new("a.ico")).unwrap();
    assert_eq!(frames[0].pixel(0, 0), [9, 8, 7, 128]);
    assert_eq!(frames[0].pixel(1, 0), [12, 11, 10, 0]);
    assert_eq!(frames[0].pixel(0, 1), [3, 2, 1, 40]);
    assert_eq!(frames[0].pixel(1, 1), [6, 5, 4, 255]);
}

#[test]
fn lee_dib_de_24_bits_con_mascara() {
    // 2x2, filas de 6 bytes + 2 de relleno. Máscara: píxel (1,0) transparente.
    let pixels = [
        1, 2, 3, 4, 5, 6, 0, 0, // inferior
        7, 8, 9, 10, 11, 12, 0, 0, // superior
    ];
    let mask = [0, 0, 0, 0, 0x40, 0, 0, 0];
    let icon = dib_icon(2, 2, 24, &[], &pixels, &mask);
    let frame = &load_icon_frames(&icon, Path::new("a.ico")).unwrap()[0];
    assert_eq!(frame.pixel(0, 0), [9, 8, 7, 255]);
    assert_eq!(frame.pixel(1, 0)[3], 0);
    assert_eq!(frame.pixel(1, 1), [6, 5, 4, 255]);
    // La máscara también va de abajo arriba: su primera fila es la inferior.
    let mask = [0x40, 0, 0, 0, 0, 0, 0, 0];
    let icon = dib_icon(2, 2, 24, &[], &pixels, &mask);
    let frame = &load_icon_frames(&icon, Path::new("a.ico")).unwrap()[0];
    assert_eq!(frame.pixel(1, 1)[3], 0);
    assert_eq!(frame.pixel(1, 0)[3], 255);
}

#[test]
fn lee_dib_con_paleta_de_8_4_y_1_bits() {
    let colors = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [9, 9, 9]];
    // 8 bits: 4x1... filas de 4 bytes.
    let icon = dib_icon(4, 1, 8, &colors, &[0, 1, 2, 3], &[0; 4]);
    let frame = &load_icon_frames(&icon, Path::new("a.ico")).unwrap()[0];
    assert_eq!(frame.pixel(1, 0), [0, 255, 0, 255]);
    assert_eq!(frame.pixel(3, 0), [9, 9, 9, 255]);
    // 4 bits: dos píxeles por byte, nibble alto primero.
    let icon = dib_icon(4, 1, 4, &colors, &[0x01, 0x23, 0, 0], &[0; 4]);
    let frame = &load_icon_frames(&icon, Path::new("a.ico")).unwrap()[0];
    assert_eq!(frame.pixel(0, 0), [255, 0, 0, 255]);
    assert_eq!(frame.pixel(2, 0), [0, 0, 255, 255]);
    // 1 bit: el bit alto es el primer píxel.
    let icon = dib_icon(8, 1, 1, &colors[..2], &[0b1010_0000, 0, 0, 0], &[0; 4]);
    let frame = &load_icon_frames(&icon, Path::new("a.ico")).unwrap()[0];
    assert_eq!(frame.pixel(0, 0), [0, 255, 0, 255]);
    assert_eq!(frame.pixel(1, 0), [255, 0, 0, 255]);
    assert_eq!(frame.pixel(2, 0), [0, 255, 0, 255]);
}

#[test]
fn un_ico_ilegible_o_sin_tamanos_validos_da_error() {
    let source = Path::new("x.ico");
    for bytes in [&b""[..], b"no es un icono", &[0, 0, 2, 0, 1, 0]] {
        let error = load_icon_frames(bytes, source).unwrap_err().to_string();
        assert!(
            error.contains("No se puede interpretar el icono"),
            "{error}"
        );
    }
    // Directorio sin entradas: el original lo trata como sin tamaños válidos.
    let error = load_icon_frames(&[0, 0, 1, 0, 0, 0], source)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no contiene tamaños válidos"), "{error}");
    // Entrada con datos fuera del archivo.
    let mut icon = dib_icon(2, 2, 24, &[], &[0; 16], &[0; 8]);
    icon.truncate(30);
    let error = load_icon_frames(&icon, source).unwrap_err().to_string();
    assert!(error.contains("No se puede leer el tamaño 2x2"), "{error}");
}

// ---------------------------------------------------------------------------
// create_mito_shortcut_icon
// ---------------------------------------------------------------------------

fn write_icon_source(folder: &Path, name: &str, color: [u8; 4]) -> PathBuf {
    let frames = [
        Image::solid(16, 16, color),
        Image::solid(32, 32, color),
        Image::solid(256, 256, color),
    ];
    let path = folder.join(name);
    std::fs::write(&path, encode_png_ico(&frames).unwrap()).unwrap();
    path
}

fn sizes_of(path: &Path) -> Vec<(u32, u32)> {
    parse_directory(&std::fs::read(path).unwrap())
        .unwrap()
        .iter()
        .map(|e| (e.width, e.height))
        .collect()
}

#[test]
fn crea_el_icono_con_insignia_en_la_cache_y_lo_reutiliza() {
    let folder = tempfile::tempdir().unwrap();
    let source = write_icon_source(folder.path(), "app.ico", [26, 75, 160, 255]);
    let cache = folder.path().join("core/state/icons");
    let made =
        create_mito_shortcut_icon(&source, &cache, "app-abc-1234", "mito", "naranja").unwrap();
    assert_eq!(made.parent().unwrap(), resolve_lenient(&cache));
    let name = made.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("app-abc-1234-mito-")
            && Path::new(&name).extension().is_some_and(|e| e == "ico"),
        "{name}"
    );
    assert_eq!(name.len(), "app-abc-1234-mito-".len() + 16 + 4);
    assert_eq!(sizes_of(&made), [(16, 16), (32, 32), (256, 256)]);
    // El píxel (0,0) conserva el fondo y la mitad inferior derecha cambia.
    let frames = load_icon_frames(&std::fs::read(&made).unwrap(), &made).unwrap();
    assert_eq!(frames[1].pixel(0, 0), [26, 75, 160, 255]);
    let mut colors: Vec<[u8; 4]> = (16..32)
        .flat_map(|y| (16..32).map(move |x| (x, y)))
        .map(|(x, y)| frames[1].pixel(x, y))
        .collect();
    colors.dedup();
    assert!(colors.len() > 1);
    // Mismos datos: mismo archivo y sin regenerarlo.
    let stamp = std::fs::metadata(&made).unwrap().modified().unwrap();
    let again =
        create_mito_shortcut_icon(&source, &cache, "app-abc-1234", "MITO", "naranja").unwrap();
    assert_eq!(again, made);
    assert_eq!(
        std::fs::metadata(&again).unwrap().modified().unwrap(),
        stamp
    );
    // Otro texto o color: otro archivo; el anterior queda.
    let other =
        create_mito_shortcut_icon(&source, &cache, "app-abc-1234", "DEV", "naranja").unwrap();
    let green =
        create_mito_shortcut_icon(&source, &cache, "app-abc-1234", "MITO", "verde").unwrap();
    assert!(other != made && green != made && green != other);
    assert!(made.is_file());
    // Sin temporales sueltos.
    let leftovers = std::fs::read_dir(&cache)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn cambiar_el_icono_de_origen_genera_otro_archivo() {
    let folder = tempfile::tempdir().unwrap();
    let cache = folder.path().join("icons");
    let red = write_icon_source(folder.path(), "red.ico", [255, 0, 0, 255]);
    let green = write_icon_source(folder.path(), "green.ico", [0, 255, 0, 255]);
    let first = create_mito_shortcut_icon(&red, &cache, "x", "MITO", "azul").unwrap();
    let second = create_mito_shortcut_icon(&green, &cache, "x", "MITO", "azul").unwrap();
    assert_ne!(first, second);
    let frames = load_icon_frames(&std::fs::read(&second).unwrap(), &second).unwrap();
    assert_eq!(frames[0].pixel(0, 0), [0, 255, 0, 255]);
}

#[test]
fn valida_el_texto_el_color_y_el_origen() {
    let folder = tempfile::tempdir().unwrap();
    let source = write_icon_source(folder.path(), "a.ico", [1, 2, 3, 255]);
    let cache = folder.path().join("icons");
    let message = |text: &str, color: &str, source: &Path| {
        create_mito_shortcut_icon(source, &cache, "x", text, color)
            .unwrap_err()
            .to_string()
    };
    for bad in ["ABCDE", "A B", "!", "", "  "] {
        assert_eq!(
            message(bad, "azul", &source),
            "shortcut.badgeText debe contener entre 1 y 4 letras o números",
            "{bad:?}"
        );
    }
    assert_eq!(
        message("AB", "fucsia", &source),
        "Elige un color de la paleta del icono"
    );
    assert!(
        message("AB", "azul", &folder.path().join("nada.ico"))
            .contains("No existe el icono del launcher")
    );
    let text = folder.path().join("a.txt");
    std::fs::write(&text, "x").unwrap();
    assert!(message("AB", "azul", &text).contains("Formato de icono no soportado"));
    let broken = folder.path().join("roto.ico");
    std::fs::write(&broken, "basura").unwrap();
    assert!(message("AB", "azul", &broken).contains("No se puede interpretar el icono"));
    // `ab12` se normaliza.
    assert!(create_mito_shortcut_icon(&source, &cache, "x", "ab12", "azul").is_ok());
    // Acepta letras no ASCII, como `isalnum` de Python.
    assert!(create_mito_shortcut_icon(&source, &cache, "x", "Ñ", "azul").is_ok());
}

#[test]
fn extrae_el_icono_de_un_ejecutable() {
    let folder = tempfile::tempdir().unwrap();
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    let executable = Path::new(&system).join(r"System32\notepad.exe");
    let made = create_mito_shortcut_icon(&executable, folder.path(), "pe", "MITO", "rojo").unwrap();
    let sizes = sizes_of(&made);
    assert!(!sizes.is_empty() && sizes.iter().all(|&(w, h)| w <= 256 && h <= 256));
    // Un ejecutable sin recursos de icono da el error del original.
    let error = create_mito_shortcut_icon(
        &std::env::current_exe().unwrap(),
        folder.path(),
        "pe",
        "MITO",
        "rojo",
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("no contiene un icono extraíble"), "{error}");
}

#[test]
fn el_digest_depende_de_los_bytes_la_revision_el_texto_y_el_color() {
    let folder = tempfile::tempdir().unwrap();
    let source = write_icon_source(folder.path(), "a.ico", [9, 9, 9, 255]);
    let bytes = std::fs::read(&source).unwrap();
    let digest = sha256_hex(&[&bytes, b"mito-shortcut-badge-v5", b"MITO", b"naranja"]);
    let made =
        create_mito_shortcut_icon(&source, &folder.path().join("i"), "id", "MITO", "naranja")
            .unwrap();
    assert_eq!(
        made.file_name().unwrap().to_string_lossy(),
        format!("id-mito-{}.ico", &digest[..16])
    );
}
