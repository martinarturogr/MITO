//! Iconos de acceso directo con insignia de MITO (`icons.py`).
//!
//! Genera un `.ico` con una insignia de texto (1 a 4 caracteres) en la esquina inferior
//! derecha sobre el icono de la aplicación, con el fondo de una paleta cerrada, y lo
//! guarda en `core/state/icons`. El original dibujaba con Pillow; aquí se usa el mismo
//! algoritmo, constantes y fuente (Aileron Regular, la que Pillow trae por defecto,
//! ADR 0013), de modo que solo puede cambiar el antialias de algún píxel del texto.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "aritmética de píxeles: los valores están acotados por el tamaño del icono (<= 256)"
)]

use std::fmt::Write as _;
use std::io::{Cursor, Write as _};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ab_glyph::{Font as _, FontRef, GlyphId, PxScale, ScaleFont as _, point};
use mito_core::Message;
use mito_core::storage::resolve_lenient;
use mito_engine::{Error, Result};
use mito_win::resources::{IconReadError, first_icon};
use sha2::{Digest as _, Sha256};

/// Texto de la insignia por defecto.
pub const DEFAULT_BADGE_TEXT: &str = "MITO";
/// Paleta de colores de la insignia (`BADGE_COLORS`), en el orden del original.
pub const BADGE_COLORS: [(&str, [u8; 3]); 8] = [
    ("negro", [34, 34, 39]),
    ("rojo", [172, 35, 47]),
    ("azul", [26, 75, 160]),
    ("naranja", [196, 80, 19]),
    ("rosa", [157, 43, 100]),
    ("verde", [26, 112, 61]),
    ("marron", [109, 65, 35]),
    ("morado", [102, 54, 135]),
];
/// Color por defecto de la insignia.
pub const DEFAULT_BADGE_COLOR: &str = "naranja";
/// Revisión del dibujo; entra en el digest de la caché (subirla la invalida).
const BADGE_REVISION: &[u8] = b"mito-shortcut-badge-v5";
/// Aileron Regular (CC0), la fuente por defecto de Pillow.
const FONT: &[u8] = include_bytes!("../assets/aileron-pillow.ttf");
/// Cobertura mínima para pintar un píxel sin antialias. Baja, porque `FreeType` engrosa
/// los trazos finos al ajustarlos a la rejilla.
const MONO_THRESHOLD: f32 = 0.3;
/// Cobertura mínima para que un píxel cuente en la caja del texto: `FreeType`, con
/// hinting, no genera tinta donde el contorno apenas sobresale (el desborde de la O).
const INK_THRESHOLD: f32 = 0.35;
/// Tamaño máximo de un frame de `.ico`.
const MAX_SIZE: u32 = 256;
/// Firma de un PNG.
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// RGB de un color de la paleta, o `None` si no existe.
pub fn badge_rgb(color: &str) -> Option<[u8; 3]> {
    BADGE_COLORS
        .iter()
        .find(|(name, _)| *name == color)
        .map(|(_, rgb)| *rgb)
}

/// Imagen RGBA de 8 bits por canal, sin premultiplicar, por filas de arriba abajo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Ancho en píxeles.
    pub width: u32,
    /// Alto en píxeles.
    pub height: u32,
    /// `width * height * 4` bytes `R, G, B, A`.
    pub data: Vec<u8>,
}

impl Image {
    /// Imagen transparente.
    pub fn transparent(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; width as usize * height as usize * 4],
        }
    }

    /// Imagen de un solo color.
    pub fn solid(width: u32, height: u32, pixel: [u8; 4]) -> Self {
        Self {
            width,
            height,
            data: pixel.repeat(width as usize * height as usize),
        }
    }

    /// Píxel `(x, y)`. Fuera de la imagen devuelve transparente.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let at = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.data[at],
            self.data[at + 1],
            self.data[at + 2],
            self.data[at + 3],
        ]
    }

    fn set(&mut self, x: i64, y: i64, pixel: [u8; 4]) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let at = (y as usize * self.width as usize + x as usize) * 4;
        self.data[at..at + 4].copy_from_slice(&pixel);
    }

    /// Mezcla `ink` sobre el píxel con la cobertura `mask` (0 a 1), canal a canal y
    /// también el alfa, como `ImageDraw.text` sobre una imagen RGBA.
    fn blend(&mut self, x: i64, y: i64, ink: [u8; 4], mask: f32) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let at = (y as usize * self.width as usize + x as usize) * 4;
        for (old, new) in self.data[at..at + 4].iter_mut().zip(ink) {
            let mixed = f32::from(*old) * (1.0 - mask) + f32::from(new) * mask;
            *old = mixed.round().clamp(0.0, 255.0) as u8;
        }
    }

    /// `Image.alpha_composite(self, overlay)`: `overlay` encima de `self`.
    fn composite(&self, overlay: &Self) -> Self {
        let mut out = self.clone();
        let (targets, _) = out.data.as_chunks_mut::<4>();
        let (overlays, _) = overlay.data.as_chunks::<4>();
        for (target, over) in targets.iter_mut().zip(overlays) {
            let source_alpha = f32::from(over[3]) / 255.0;
            if over[3] == 0 {
                continue;
            }
            let destination_alpha = f32::from(target[3]) / 255.0;
            let alpha = source_alpha + destination_alpha * (1.0 - source_alpha);
            for channel in 0..3 {
                let value = (f32::from(over[channel]) * source_alpha
                    + f32::from(target[channel]) * destination_alpha * (1.0 - source_alpha))
                    / alpha;
                target[channel] = value.round().clamp(0.0, 255.0) as u8;
            }
            target[3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        out
    }
}

fn domain(message: Message) -> Error {
    Error::domain(message)
}

/// `round()` de Python: el medio va al par.
fn py_round(value: f64) -> i64 {
    value.round_ties_even() as i64
}

/// `_validate_badge_text`: recortado, en mayúsculas, de 1 a 4 letras o números.
fn validate_badge_text(value: &str) -> Result<String> {
    let normalized = value.trim().to_uppercase();
    let length = normalized.chars().count();
    if !(1..=4).contains(&length) || !normalized.chars().all(char::is_alphanumeric) {
        return Err(domain(Message::new(
            "shortcut.badgeText debe contener entre 1 y 4 letras o números",
        )));
    }
    Ok(normalized)
}

/// SHA-256 en hexadecimal minúscula.
pub(crate) fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// Bytes de un `.ico` desde un `.ico`, `.exe` o `.dll` (`_read_icon_container`).
fn read_icon_container(source: &Path) -> Result<Vec<u8>> {
    let extension = source
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "ico" => std::fs::read(source).map_err(|error| {
            domain(
                Message::new("No se pudo leer el icono {p1}: {p3}")
                    .arg("p1", source.display())
                    .arg("p3", error),
            )
        }),
        "exe" | "dll" => first_icon(source).map_err(|error| read_error(&error, source)),
        _ => Err(domain(
            Message::new("Formato de icono no soportado para el acceso directo: {p1}")
                .arg("p1", source.display()),
        )),
    }
}

/// Mensaje del original para un fallo al leer el icono de un ejecutable.
fn read_error(error: &IconReadError, source: &Path) -> Error {
    let path = source.display();
    let message = match error {
        IconReadError::Open(code) => {
            Message::new("Windows no pudo abrir los recursos de {p1}: error {p3}")
                .arg("p1", path)
                .arg("p3", code)
        }
        IconReadError::NoIcon => {
            Message::new("El ejecutable no contiene un icono extraíble: {p1}").arg("p1", path)
        }
        IconReadError::Locate => {
            Message::new("Windows no pudo localizar un recurso de icono en {p1}").arg("p1", path)
        }
        IconReadError::Load => {
            Message::new("Windows no pudo cargar un recurso de icono en {p1}").arg("p1", path)
        }
        IconReadError::Truncated => {
            Message::new("Recurso de icono truncado en {p1}").arg("p1", path)
        }
        IconReadError::Invalid => {
            Message::new("Recurso de icono no válido en {p1}").arg("p1", path)
        }
        IconReadError::DirectoryTruncated => {
            Message::new("Directorio de icono truncado en {p1}").arg("p1", path)
        }
    };
    domain(message)
}

/// Genera (o reutiliza de la caché) el `.ico` con insignia de `source` y devuelve su
/// ruta: `<destination_directory>/<launcher_id>-mito-<digest>.ico`.
///
/// `source` es un `.ico`, `.exe` o `.dll`. `badge_color` es una clave de
/// [`BADGE_COLORS`]. Mismos datos dan el mismo archivo, que no se regenera; los
/// archivos antiguos de la caché no se borran.
///
/// # Errors
///
/// `shortcut.badgeText debe contener entre 1 y 4 letras o números`, `Elige un color de
/// la paleta del icono`, `No existe el icono del launcher: …` y los demás del original
/// al leer el origen, interpretar el `.ico` o guardar el resultado.
pub fn create_mito_shortcut_icon(
    source: &Path,
    destination_directory: &Path,
    launcher_id: &str,
    badge_text: &str,
    badge_color: &str,
) -> Result<PathBuf> {
    let badge_text = validate_badge_text(badge_text)?;
    let Some(color) = badge_rgb(badge_color) else {
        return Err(domain(Message::new(
            "Elige un color de la paleta del icono",
        )));
    };
    let source = resolve_lenient(source);
    if !source.is_file() {
        return Err(domain(
            Message::new("No existe el icono del launcher: {p1}").arg("p1", source.display()),
        ));
    }
    let icon_bytes = read_icon_container(&source)?;
    let digest = sha256_hex(&[
        &icon_bytes,
        BADGE_REVISION,
        badge_text.as_bytes(),
        badge_color.as_bytes(),
    ]);
    std::fs::create_dir_all(destination_directory)
        .map_err(|error| Error::io(destination_directory, error))?;
    let destination = resolve_lenient(
        &destination_directory.join(format!("{launcher_id}-mito-{}.ico", &digest[..16])),
    );
    if !destination.starts_with(resolve_lenient(destination_directory)) {
        return Err(domain(
            Message::new("El icono generado sale de su directorio: {p1}")
                .arg("p1", destination.display()),
        ));
    }
    if destination.is_file() {
        return Ok(destination);
    }
    let mut frames = Vec::new();
    for frame in load_icon_frames(&icon_bytes, &source)? {
        frames.push(add_mito_badge(&frame, &badge_text, color)?);
    }
    let generated = encode_png_ico(&frames)?;
    write_icon(destination_directory, launcher_id, &destination, &generated).map_err(|error| {
        domain(Message::new("No se pudo guardar el icono MITO: {p1}").arg("p1", error))
    })?;
    Ok(destination)
}

/// Escritura atómica: temporal en la misma carpeta, `fsync` y reemplazo.
fn write_icon(
    directory: &Path,
    launcher_id: &str,
    destination: &Path,
    content: &[u8],
) -> std::io::Result<()> {
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{launcher_id}-mito-"))
        .suffix(".tmp")
        .tempfile_in(directory)?;
    temporary.write_all(content)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(destination).map_err(|e| e.error)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Lectura de .ico
// ---------------------------------------------------------------------------

/// Entrada del directorio de un `.ico`.
#[derive(Debug, Clone, Copy)]
struct Entry {
    width: u32,
    height: u32,
    size: usize,
    offset: usize,
}

fn le16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn le32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn parse_directory(data: &[u8]) -> Option<Vec<Entry>> {
    if le16(data, 0)? != 0 || le16(data, 2)? != 1 {
        return None;
    }
    let count = usize::from(le16(data, 4)?);
    (0..count)
        .map(|index| {
            let at = 6 + index * 16;
            let dimension = |byte: u8| if byte == 0 { MAX_SIZE } else { u32::from(byte) };
            Some(Entry {
                width: dimension(*data.get(at)?),
                height: dimension(*data.get(at + 1)?),
                size: le32(data, at + 8)? as usize,
                offset: le32(data, at + 12)? as usize,
            })
        })
        .collect()
}

/// Tamaños del `.ico` y un frame RGBA por tamaño (`_load_icon_frames`), de menor a
/// mayor área. Se descartan los tamaños de más de 256 píxeles.
///
/// # Errors
///
/// `No se puede interpretar el icono …`, `El icono no contiene tamaños válidos: …` y
/// `No se puede leer el tamaño AxB de …`.
pub fn load_icon_frames(icon_bytes: &[u8], source: &Path) -> Result<Vec<Image>> {
    let unreadable = || {
        domain(Message::new("No se puede interpretar el icono {p1}").arg("p1", source.display()))
    };
    let entries = parse_directory(icon_bytes).ok_or_else(unreadable)?;
    let mut sizes: Vec<(u32, u32)> = entries
        .iter()
        .map(|e| (e.width, e.height))
        .filter(|&(w, h)| (1..=MAX_SIZE).contains(&w) && (1..=MAX_SIZE).contains(&h))
        .collect();
    sizes.sort_by_key(|&(w, h)| (w * h, (w, h)));
    sizes.dedup();
    if sizes.is_empty() {
        return Err(domain(
            Message::new("El icono no contiene tamaños válidos: {p1}").arg("p1", source.display()),
        ));
    }
    let mut frames = Vec::with_capacity(sizes.len());
    for (width, height) in sizes {
        let entry = entries
            .iter()
            .find(|e| (e.width, e.height) == (width, height));
        let frame = entry.and_then(|e| {
            let end = e.offset.checked_add(e.size)?;
            decode_frame(icon_bytes.get(e.offset..end)?)
        });
        frames.push(frame.ok_or_else(|| {
            domain(
                Message::new("No se puede leer el tamaño {p1}x{p3} de {p5}")
                    .arg("p1", width)
                    .arg("p3", height)
                    .arg("p5", source.display()),
            )
        })?);
    }
    Ok(frames)
}

fn decode_frame(data: &[u8]) -> Option<Image> {
    let image = if data.starts_with(PNG_SIGNATURE) {
        decode_png(data)?
    } else {
        decode_dib(data)?
    };
    (image.width <= MAX_SIZE && image.height <= MAX_SIZE).then_some(image)
}

/// PNG a RGBA de 8 bits.
fn decode_png(data: &[u8]) -> Option<Image> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let pixels = &buffer[..info.buffer_size()];
    let count = info.width as usize * info.height as usize;
    let mut out = Vec::with_capacity(count * 4);
    match info.color_type {
        png::ColorType::Rgba => out.extend_from_slice(pixels.get(..count * 4)?),
        png::ColorType::Rgb => {
            for pixel in pixels.get(..count * 3)?.as_chunks::<3>().0 {
                out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in pixels.get(..count * 2)?.as_chunks::<2>().0 {
                out.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        png::ColorType::Grayscale => {
            for &gray in pixels.get(..count)? {
                out.extend_from_slice(&[gray, gray, gray, 255]);
            }
        }
        png::ColorType::Indexed => return None,
    }
    Some(Image {
        width: info.width,
        height: info.height,
        data: out,
    })
}

/// DIB (`BITMAPINFOHEADER`) de un `.ico`: bitmap de colores y máscara AND de 1 bit, con
/// el alto de la cabecera duplicado. Admite 32, 24, 8, 4 y 1 bits sin compresión.
fn decode_dib(data: &[u8]) -> Option<Image> {
    let header = le32(data, 0)? as usize;
    if header < 40 {
        return None;
    }
    let width = le32(data, 4)? as usize;
    let doubled = i32::from_le_bytes(data.get(8..12)?.try_into().ok()?);
    let bits = usize::from(le16(data, 14)?);
    let compression = le32(data, 16)?;
    let used = le32(data, 32)? as usize;
    if compression != 0 || doubled <= 0 || width == 0 || width > MAX_SIZE as usize {
        return None;
    }
    let height = doubled as usize / 2;
    if height == 0 || height > MAX_SIZE as usize {
        return None;
    }
    let palette_size = match bits {
        1 | 4 | 8 => {
            if used == 0 {
                1 << bits
            } else {
                used.min(1 << bits)
            }
        }
        24 | 32 => 0,
        _ => return None,
    };
    let palette_at = header;
    let palette = data.get(palette_at..palette_at + palette_size * 4)?;
    let pixels_at = palette_at + palette_size * 4;
    let stride = (width * bits).div_ceil(32) * 4;
    let pixels = data.get(pixels_at..pixels_at + stride * height)?;
    let mask_at = pixels_at + stride * height;
    let mask_stride = width.div_ceil(32) * 4;
    let mask = data.get(mask_at..mask_at + mask_stride * height);
    let mut out = vec![0_u8; width * height * 4];
    for y in 0..height {
        // Las filas van de abajo arriba.
        let source_row = &pixels[(height - 1 - y) * stride..][..stride];
        for x in 0..width {
            let (rgb, alpha) = match bits {
                32 => {
                    let p = &source_row[x * 4..x * 4 + 4];
                    ([p[2], p[1], p[0]], p[3])
                }
                24 => {
                    let p = &source_row[x * 3..x * 3 + 3];
                    ([p[2], p[1], p[0]], 255)
                }
                _ => {
                    let per_byte = 8 / bits;
                    let byte = source_row[x / per_byte];
                    let shift = 8 - bits * (x % per_byte + 1);
                    let index = usize::from(byte >> shift) & ((1 << bits) - 1);
                    let entry = palette.get(index * 4..index * 4 + 4).unwrap_or(&[0; 4]);
                    ([entry[2], entry[1], entry[0]], 255)
                }
            };
            let transparent = bits != 32
                && mask.is_some_and(|mask| {
                    let row = &mask[(height - 1 - y) * mask_stride..][..mask_stride];
                    row[x / 8] >> (7 - x % 8) & 1 == 1
                });
            let at = (y * width + x) * 4;
            out[at..at + 3].copy_from_slice(&rgb);
            out[at + 3] = if transparent { 0 } else { alpha };
        }
    }
    Some(Image {
        width: width as u32,
        height: height as u32,
        data: out,
    })
}

// ---------------------------------------------------------------------------
// Texto
// ---------------------------------------------------------------------------

fn font() -> Result<&'static FontRef<'static>> {
    static FONT_FACE: OnceLock<Option<FontRef<'static>>> = OnceLock::new();
    FONT_FACE
        .get_or_init(|| FontRef::try_from_slice(FONT).ok())
        .as_ref()
        .ok_or_else(|| domain(Message::new("No se pudo cargar la fuente de la insignia")))
}

/// Texto colocado con la fuente a un tamaño: glifos, caja de tinta y ascenso.
struct TextLayout {
    scale: PxScale,
    /// Glifo y posición horizontal de su origen.
    glyphs: Vec<(GlyphId, f32)>,
    /// `textbbox((0, 0), …)`: `(izquierda, arriba, derecha, abajo)`.
    bbox: [i64; 4],
    /// Distancia de la línea superior a la línea base.
    ascent: f32,
}

impl TextLayout {
    fn new(font: &FontRef<'_>, size: i64, text: &str) -> Self {
        let units = font.units_per_em().unwrap_or(1000.0);
        let scale = PxScale::from(size as f32 * font.height_unscaled() / units);
        let scaled = font.as_scaled(scale);
        let ascent = scaled.ascent().ceil();
        let mut glyphs = Vec::new();
        let mut pen = 0.0_f32;
        let mut previous: Option<GlyphId> = None;
        for character in text.chars() {
            let id = font.glyph_id(character);
            if let Some(before) = previous {
                pen += scaled.kern(before, id);
            }
            glyphs.push((id, pen));
            pen += scaled.h_advance(id);
            previous = Some(id);
        }
        // Caja de los píxeles con tinta, como la del mapa de bits que mide Pillow.
        let (mut left, mut top, mut right, mut bottom) =
            (0_i64, i64::MAX, pen.ceil() as i64, i64::MIN);
        for &(id, x) in &glyphs {
            let glyph = id.with_scale_and_position(scale, point(x, ascent));
            let Some(outline) = font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            let (origin_x, origin_y) = (bounds.min.x as i64, bounds.min.y as i64);
            outline.draw(|px, py, coverage| {
                if coverage >= INK_THRESHOLD {
                    let (px, py) = (origin_x + i64::from(px), origin_y + i64::from(py));
                    left = left.min(px);
                    right = right.max(px + 1);
                    top = top.min(py);
                    bottom = bottom.max(py + 1);
                }
            });
        }
        if top > bottom {
            (top, bottom) = (0, 0);
        }
        Self {
            scale,
            glyphs,
            bbox: [left, top, right, bottom],
            ascent,
        }
    }

    fn width(&self) -> i64 {
        self.bbox[2] - self.bbox[0]
    }

    fn height(&self) -> i64 {
        self.bbox[3] - self.bbox[1]
    }

    /// Dibuja el texto con su esquina superior izquierda en `(x, y)`. En modo
    /// monocromo (`fontmode = "1"`) no hay antialias.
    fn draw(&self, font: &FontRef<'_>, target: &mut Image, x: f32, y: f32, mono: bool) {
        let ink = [255, 255, 255, 255];
        for &(id, offset) in &self.glyphs {
            // FreeType ajusta los glifos a la rejilla en modo monocromo: se parte de
            // posiciones enteras para que los trazos finos cubran píxeles completos.
            let (at_x, at_y) = if mono {
                ((x + offset).round(), (y + self.ascent).round())
            } else {
                (x + offset, y + self.ascent)
            };
            let glyph = id.with_scale_and_position(self.scale, point(at_x, at_y));
            let Some(outline) = font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            let (origin_x, origin_y) = (bounds.min.x as i64, bounds.min.y as i64);
            outline.draw(|px, py, coverage| {
                let coverage = if mono {
                    f32::from(u8::from(coverage >= MONO_THRESHOLD))
                } else {
                    coverage
                };
                if coverage > 0.0 {
                    target.blend(
                        origin_x + i64::from(px),
                        origin_y + i64::from(py),
                        ink,
                        coverage,
                    );
                }
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Insignias
// ---------------------------------------------------------------------------

/// Ajusta el tamaño de la fuente hasta que la insignia cabe a lo ancho.
fn fit_text(
    font: &FontRef<'_>,
    text: &str,
    start: i64,
    width: i64,
    (horizontal, vertical): (i64, i64),
) -> (TextLayout, i64, i64) {
    let mut size = start;
    loop {
        let layout = TextLayout::new(font, size, text);
        let badge_width = layout.width() + 2 * horizontal;
        let badge_height = layout.height() + 2 * vertical;
        if badge_width <= width || size <= 4 {
            return (layout, badge_width, badge_height);
        }
        size -= 1;
    }
}

/// `_add_mito_badge`: insignia redondeada con contorno (lado más corto > 48) o, si no,
/// la pequeña.
///
/// # Errors
///
/// Si no se puede cargar la fuente incluida.
pub fn add_mito_badge(frame: &Image, badge_text: &str, color: [u8; 3]) -> Result<Image> {
    let (width, height) = (i64::from(frame.width), i64::from(frame.height));
    let shortest = width.min(height);
    if shortest <= 48 {
        return add_small_badge(frame, badge_text, color);
    }
    let font = font()?;
    let hpad = py_round(shortest as f64 * 0.035).max(1);
    let vpad = py_round(shortest as f64 * 0.018).max(1);
    let start = py_round(shortest as f64 * 0.23).max(5);
    let (layout, badge_width, badge_height) =
        fit_text(font, badge_text, start, width, (hpad, vpad));
    let (left, top) = (width - badge_width, height - badge_height);
    let radius = py_round(shortest as f64 * 0.04).max(1);
    let outline = py_round(shortest as f64 * 0.008).max(1);
    let mut overlay = Image::transparent(frame.width, frame.height);
    let rectangle = (left, top, width - 1, height - 1);
    let inner = (
        rectangle.0 + outline,
        rectangle.1 + outline,
        rectangle.2 - outline,
        rectangle.3 - outline,
    );
    for y in top.max(0)..height {
        for x in left.max(0)..width {
            if !in_rounded(x, y, rectangle, radius) {
                continue;
            }
            let border = !in_rounded(x, y, inner, (radius - outline).max(0));
            let pixel = if border {
                [255, 255, 255, 245]
            } else {
                [color[0], color[1], color[2], 244]
            };
            overlay.set(x, y, pixel);
        }
    }
    let text_x = left as f32 + (badge_width - layout.width()) as f32 / 2.0 - layout.bbox[0] as f32;
    let text_y = top as f32 + (badge_height - layout.height()) as f32 / 2.0 - layout.bbox[1] as f32;
    layout.draw(font, &mut overlay, text_x, text_y, false);
    Ok(frame.composite(&overlay))
}

/// ¿Está el píxel `(x, y)` dentro del rectángulo (extremos incluidos) con esquinas de
/// ese radio?
fn in_rounded(x: i64, y: i64, (x0, y0, x1, y1): (i64, i64, i64, i64), radius: i64) -> bool {
    if x < x0 || x > x1 || y < y0 || y > y1 {
        return false;
    }
    let radius = radius.min((x1 - x0 + 1) / 2).min((y1 - y0 + 1) / 2);
    let center_x = if x < x0 + radius {
        x0 + radius
    } else if x > x1 - radius {
        x1 - radius
    } else {
        return true;
    };
    let center_y = if y < y0 + radius {
        y0 + radius
    } else if y > y1 - radius {
        y1 - radius
    } else {
        return true;
    };
    let (dx, dy) = ((x - center_x) as f64, (y - center_y) as f64);
    dx * dx + dy * dy <= (radius as f64 - 0.5).powi(2) + 0.5
}

/// `_add_small_badge`: rectángulo sólido sin contorno y texto sin antialias.
///
/// # Errors
///
/// Si no se puede cargar la fuente incluida.
pub fn add_small_badge(frame: &Image, badge_text: &str, color: [u8; 3]) -> Result<Image> {
    let (width, height) = (i64::from(frame.width), i64::from(frame.height));
    let shortest = width.min(height);
    let font = font()?;
    let padding = (shortest / 32).max(1);
    let start = py_round(shortest as f64 * 0.3).max(4);
    let (layout, badge_width, badge_height) =
        fit_text(font, badge_text, start, width, (padding, padding));
    let (left, top) = (width - badge_width, height - badge_height);
    let mut overlay = Image::transparent(frame.width, frame.height);
    for y in top.max(0)..height {
        for x in left.max(0)..width {
            overlay.set(x, y, [color[0], color[1], color[2], 255]);
        }
    }
    let text_x = left as f32 + (badge_width - layout.width()) as f32 / 2.0 - layout.bbox[0] as f32;
    let text_y = top as f32 + (badge_height - layout.height()) as f32 / 2.0 - layout.bbox[1] as f32;
    layout.draw(font, &mut overlay, text_x, text_y, true);
    Ok(frame.composite(&overlay))
}

// ---------------------------------------------------------------------------
// Escritura de .ico
// ---------------------------------------------------------------------------

fn encode_png(image: &Image) -> Option<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut encoder = png::Encoder::new(&mut buffer, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&image.data).ok()?;
    writer.finish().ok()?;
    Some(buffer)
}

/// `_encode_png_ico`: un `.ico` con una imagen PNG por tamaño (gana el último de cada
/// tamaño), de menor a mayor área.
///
/// # Errors
///
/// `No hay imágenes para construir el icono MITO` si no hay frames o no se puede
/// codificar alguno.
pub fn encode_png_ico(frames: &[Image]) -> Result<Vec<u8>> {
    let mut unique: Vec<&Image> = Vec::new();
    for frame in frames {
        match unique
            .iter()
            .position(|f| (f.width, f.height) == (frame.width, frame.height))
        {
            Some(at) => unique[at] = frame,
            None => unique.push(frame),
        }
    }
    unique.sort_by_key(|f| (f.width * f.height, (f.width, f.height)));
    let empty = || domain(Message::new("No hay imágenes para construir el icono MITO"));
    if unique.is_empty() {
        return Err(empty());
    }
    let mut images = Vec::with_capacity(unique.len());
    for frame in &unique {
        images.push(encode_png(frame).ok_or_else(empty)?);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&(unique.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * unique.len();
    for (frame, image) in unique.iter().zip(&images) {
        let dimension = |value: u32| if value < MAX_SIZE { value as u8 } else { 0 };
        out.extend_from_slice(&[dimension(frame.width), dimension(frame.height), 0, 0]);
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&32_u16.to_le_bytes());
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    for image in &images {
        out.extend_from_slice(image);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
