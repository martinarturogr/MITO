//! Bundles `WiX` 3 Burn leídos como datos (`burn.py`): se localizan los contenedores de
//! la sección `.wixburn` y sus CAB se descomprimen con el 7-Zip privado. El bundle
//! nunca se ejecuta.
//!
//! Contrato: `docs/spec/modules/burn.md`. Formato de referencia: `BurnCommon.cs` y
//! `BurnReader.cs` de `wixtoolset/wix3`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use mito_core::Message;
use mito_core::storage::{inside, portable_path};

use super::{Limits, MAX_ENTRIES, MAX_EXPANDED, casefold, extract_7zip_with};
use crate::error::{Error, Result};
use crate::hash::parse_hash;

/// Espacio de nombres del manifiesto.
const NS: &str = "http://schemas.microsoft.com/wix/2008/Burn";
/// Tamaño máximo del manifiesto.
const MAX_MANIFEST: u64 = 16 * 1024 * 1024;
/// Bloque de copia y de hash.
const CHUNK: usize = 1024 * 1024;

fn fail(text: &str) -> Error {
    Error::untranslated(format!("Burn: {text}"))
}

fn u32_at(data: &[u8], offset: usize) -> u64 {
    data.get(offset..offset + 4)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map_or(0, |bytes| u64::from(u32::from_le_bytes(bytes)))
}

fn u16_at(data: &[u8], offset: usize) -> u64 {
    data.get(offset..offset + 2)
        .and_then(|bytes| <[u8; 2]>::try_from(bytes).ok())
        .map_or(0, |bytes| u64::from(u16::from_le_bytes(bytes)))
}

/// Posición y tamaño de los dos contenedores: UX y adjunto (`containers`).
fn containers<R: Read + Seek>(stream: &mut R) -> Result<[(u64, u64); 2]> {
    let length = stream
        .seek(SeekFrom::End(0))
        .map_err(|e| Error::untranslated(format!("Burn: {e}")))?;
    let mut read = |offset: u64, size: u64| -> Result<Vec<u8>> {
        if offset.saturating_add(size) > length {
            return Err(fail("truncated or invalid container range"));
        }
        let mut data = vec![0_u8; usize::try_from(size).map_err(|_| fail("truncated header"))?];
        stream
            .seek(SeekFrom::Start(offset))
            .and_then(|_| stream.read_exact(&mut data))
            .map_err(|_| fail("truncated header"))?;
        Ok(data)
    };
    let dos = read(0, 64)?;
    if !dos.starts_with(b"MZ") {
        return Err(fail("missing DOS header"));
    }
    let pe = u32_at(&dos, 60);
    let header = read(pe, 24)?;
    if !header.starts_with(b"PE\0\0") {
        return Err(fail("missing PE header"));
    }
    let count = u16_at(&header, 6);
    let optional = u16_at(&header, 20);
    if !(1..=96).contains(&count) {
        return Err(fail("invalid PE section count"));
    }
    let sections = read(pe + 24 + optional, count * 40)?;
    let matches: Vec<&[u8]> = sections
        .chunks(40)
        .filter(|section| section.starts_with(b".wixburn"))
        .collect();
    let [section] = matches.as_slice() else {
        return Err(fail("missing or duplicate .wixburn section"));
    };
    let (raw_size, offset) = (u32_at(section, 16), u32_at(section, 20));
    if raw_size < 56 {
        return Err(fail("short section"));
    }
    let data = read(offset, 56)?;
    let (magic, version) = (u32_at(&data, 0), u32_at(&data, 4));
    let (stub, signature, signature_size) =
        (u32_at(&data, 24), u32_at(&data, 32), u32_at(&data, 36));
    let (kind, count, ux, attached) = (
        u32_at(&data, 40),
        u32_at(&data, 44),
        u32_at(&data, 48),
        u32_at(&data, 52),
    );
    if magic != 0x00F1_4300 || version != 2 || kind != 1 || count != 2 {
        return Err(fail(
            "only v2 bundles with UX and one attached CAB are supported",
        ));
    }
    let start = if signature == 0 {
        stub + ux
    } else {
        signature + signature_size
    };
    if stub < offset + 56 || start < stub + ux || ux == 0 || attached == 0 {
        return Err(fail("invalid or overlapping containers"));
    }
    let result = [(stub, ux), (start, attached)];
    for (position, size) in result {
        if size > MAX_EXPANDED || position + size > length {
            return Err(fail("truncated or oversized CAB"));
        }
        let cab = read(position, 36)?;
        if !cab.starts_with(b"MSCF") || u32_at(&cab, 8) != size || u16_at(&cab, 30) & 3 != 0 {
            return Err(fail("invalid or spanning CAB"));
        }
    }
    Ok(result)
}

/// Un `Payload` embebido del manifiesto, ya validado.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Payload {
    /// Nombre del archivo dentro del CAB (`SourcePath`).
    source: String,
    /// Destino relativo, con `/` (`FilePath`).
    target: String,
    /// Tamaño declarado.
    size: Option<u64>,
    /// Hash declarado, tal cual.
    digest: Option<String>,
}

fn element_children<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    name: &'a str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    node.children().filter(move |child| {
        child.is_element()
            && child.tag_name().name() == name
            && child.tag_name().namespace() == Some(NS)
    })
}

/// Valida el manifiesto XML y devuelve los planes de los dos contenedores
/// (`inventory`). Los payloads `external` se omiten con un aviso.
fn inventory(manifest: &[u8], log: &dyn Fn(Message)) -> Result<[Vec<Payload>; 2]> {
    if manifest.len() as u64 > MAX_MANIFEST {
        return Err(fail("oversized manifest"));
    }
    let manifest = manifest.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(manifest);
    let text = std::str::from_utf8(manifest).map_err(|_| fail("manifest must be UTF-8"))?;
    let upper = text.to_uppercase();
    if text.contains('\0') || upper.contains("<!DOCTYPE") || upper.contains("<!ENTITY") {
        return Err(fail("unsupported XML declaration or oversized manifest"));
    }
    let document = roxmltree::Document::parse(text).map_err(|_| fail("invalid manifest XML"))?;
    let root = document.root_element();
    if root.tag_name().name() != "BurnManifest" || root.tag_name().namespace() != Some(NS) {
        return Err(fail("unsupported manifest namespace"));
    }
    let groups: [Vec<roxmltree::Node<'_, '_>>; 2] = [
        element_children(root, "UX")
            .flat_map(|ux| element_children(ux, "Payload"))
            .collect(),
        element_children(root, "Payload").collect(),
    ];
    if groups.iter().map(Vec::len).sum::<usize>() > MAX_ENTRIES {
        return Err(fail("oversized payload inventory"));
    }
    let mut plans: [Vec<Payload>; 2] = [Vec::new(), Vec::new()];
    let (mut total, mut external) = (0_u64, 0_usize);
    for (index, group) in groups.iter().enumerate() {
        let (mut names, mut ids) = (HashSet::new(), HashSet::new());
        for entry in group {
            let packaging = entry
                .attribute("Packaging")
                .unwrap_or("embedded")
                .to_lowercase();
            // Como `BurnReader` de WiX: solo se extraen los payloads adjuntos. Los
            // `external` no se resuelven ni se descargan nunca.
            if packaging == "external" && index == 1 {
                external += 1;
                continue;
            }
            if packaging != "embedded" {
                return Err(fail("unsupported payload packaging"));
            }
            let source = entry.attribute("SourcePath").unwrap_or_default();
            let target = entry.attribute("FilePath").unwrap_or_default();
            if source.is_empty()
                || target.is_empty()
                || portable_path(source)?.segments().len() != 1
            {
                return Err(fail("invalid payload source"));
            }
            let path = portable_path(target)?;
            if path.is_empty() {
                return Err(fail(&format!("empty payload destination for {source}")));
            }
            let folded = casefold(&path.segments().join("/"));
            if !ids.insert(casefold(source)) {
                return Err(fail(&format!("duplicate payload source: {source}")));
            }
            // Varios ámbitos pueden llevar copias del mismo destino: se conservan
            // todas y se fusionan solo tras comparar los bytes extraídos.
            names.insert(folded);
            let size = match entry.attribute("FileSize") {
                None => None,
                Some(text) => match text.trim().parse::<i64>() {
                    Ok(size) if size >= 0 => Some(size.unsigned_abs()),
                    _ => return Err(fail("invalid payload size")),
                },
            };
            if let Some(size) = size {
                total = total.saturating_add(size);
            }
            let digest = entry.attribute("Hash");
            parse_hash(digest)?;
            plans[index].push(Payload {
                source: source.to_owned(),
                target: path.segments().join("/"),
                size,
                digest: digest.map(str::to_owned),
            });
        }
        for payload in &plans[index] {
            let mut parent = payload.target.as_str();
            while let Some((upper, _)) = parent.rsplit_once('/') {
                if names.contains(&casefold(upper)) {
                    return Err(fail("file/directory collision"));
                }
                parent = upper;
            }
        }
    }
    if plans.iter().map(Vec::len).sum::<usize>() > MAX_ENTRIES || total > MAX_EXPANDED {
        return Err(fail("oversized payload inventory"));
    }
    if external > 0 {
        log(Message::new("{p1}").arg(
            "p1",
            format!(
                "Burn: {external} external payload reference(s) omitted; extracting embedded files only"
            ),
        ));
    }
    Ok(plans)
}

/// Extrae un CAB (`cabinet`) en `folder` con los límites dados.
type CabinetExtractor<'a> = &'a dyn Fn(&Path, &Path, &Limits) -> Result<()>;

/// Suma de los bytes de los archivos de `folder` y sus subcarpetas.
fn folder_size(folder: &Path) -> Result<u64> {
    let mut total = 0_u64;
    let mut pending = vec![folder.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).map_err(|e| mito_core::Error::io(&current, e))? {
            let entry = entry.map_err(|e| mito_core::Error::io(&current, e))?;
            let kind = entry
                .file_type()
                .map_err(|e| mito_core::Error::io(entry.path(), e))?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                total = total.saturating_add(
                    entry
                        .metadata()
                        .map_err(|e| mito_core::Error::io(entry.path(), e))?
                        .len(),
                );
            }
        }
    }
    Ok(total)
}

/// `filecmp.cmp(a, b, shallow=False)`.
fn same_content(a: &Path, b: &Path) -> Result<bool> {
    let open = |path: &Path| fs::File::open(path).map_err(|e| mito_core::Error::io(path, e));
    let (mut left, mut right) = (open(a)?, open(b)?);
    let length = |file: &fs::File, path: &Path| {
        file.metadata()
            .map(|m| m.len())
            .map_err(|e| mito_core::Error::io(path, e))
    };
    if length(&left, a)? != length(&right, b)? {
        return Ok(false);
    }
    let (mut x, mut y) = (vec![0_u8; CHUNK], vec![0_u8; CHUNK]);
    loop {
        let count = left.read(&mut x).map_err(|e| mito_core::Error::io(a, e))?;
        if count == 0 {
            return Ok(true);
        }
        right
            .read_exact(&mut y[..count])
            .map_err(|e| mito_core::Error::io(b, e))?;
        if x[..count] != y[..count] {
            return Ok(false);
        }
    }
}

/// Comprueba tamaño y hash declarados de un payload extraído.
fn verify(file: &Path, payload: &Payload) -> Result<()> {
    let size = fs::metadata(file)
        .map_err(|e| mito_core::Error::io(file, e))?
        .len();
    if payload.size.is_some_and(|expected| expected != size) {
        return Err(fail("payload size mismatch"));
    }
    if let Some(expected) = parse_hash(payload.digest.as_deref())? {
        let mut hasher = expected.hasher();
        let mut data = fs::File::open(file).map_err(|e| mito_core::Error::io(file, e))?;
        let mut buffer = vec![0_u8; CHUNK];
        loop {
            let count = data
                .read(&mut buffer)
                .map_err(|e| mito_core::Error::io(file, e))?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        if hasher.hex_digest().to_lowercase() != expected.digest {
            return Err(fail("payload hash mismatch"));
        }
    }
    Ok(())
}

/// Extrae el contenido de `archive` en `destination` (`extract_burn`). `log` recibe el
/// progreso.
///
/// El bundle nunca se ejecuta. Los contenedores UX y adjunto se extraen con el 7-Zip
/// privado en un directorio de trabajo junto a `destination`; solo si todo el bundle
/// (manifiesto, nombres, tamaños y hashes) es válido se publica `UX/` y
/// `AttachedContainer/` en `destination`.
///
/// # Errors
///
/// `Burn: …` (sin traducir, como el original), los de 7-Zip, límites y E/S.
pub fn extract_burn(
    archive: &Path,
    destination: &Path,
    sevenzip: Option<&Path>,
    log: &dyn Fn(Message),
) -> Result<()> {
    let extractor = |cabinet: &Path, folder: &Path, limits: &Limits| {
        extract_7zip_with(cabinet, folder, sevenzip, limits)
    };
    extract_with(archive, destination, &extractor, log)
}

/// Núcleo de [`extract_burn`] con el extractor de CAB inyectado.
fn extract_with(
    archive: &Path,
    destination: &Path,
    extract_cabinet: CabinetExtractor<'_>,
    log: &dyn Fn(Message),
) -> Result<()> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| mito_core::Error::io(parent, e))?;
    let mut stream = fs::File::open(archive).map_err(|e| mito_core::Error::io(archive, e))?;
    let scratch = tempfile::tempdir_in(parent).map_err(|e| mito_core::Error::io(parent, e))?;
    let ranges = containers(&mut stream)?;
    let mut expanded = Vec::with_capacity(2);
    let mut remaining = MAX_EXPANDED;
    for (index, (offset, size)) in ranges.into_iter().enumerate() {
        let cabinet = scratch.path().join(format!("{index}.cab"));
        copy_range(&mut stream, offset, size, &cabinet)?;
        let folder = scratch.path().join(index.to_string());
        let limits = Limits {
            max_entries: MAX_ENTRIES,
            max_expanded: remaining,
        };
        extract_cabinet(&cabinet, &folder, &limits)?;
        remaining = remaining
            .checked_sub(folder_size(&folder)?)
            .ok_or_else(|| fail("oversized containers"))?;
        expanded.push(folder);
    }
    let manifest = expanded[0].join("0");
    let manifest_ok = fs::metadata(&manifest).is_ok_and(|m| m.is_file() && m.len() <= MAX_MANIFEST);
    if !manifest_ok {
        return Err(fail("missing or oversized manifest"));
    }
    let bytes = fs::read(&manifest).map_err(|e| mito_core::Error::io(&manifest, e))?;
    let plans = inventory(&bytes, log)?;
    let mut copies: Vec<(PathBuf, String)> = vec![(manifest.clone(), "UX/manifest.xml".to_owned())];
    let mut destinations: HashMap<String, PathBuf> =
        HashMap::from([("ux/manifest.xml".to_owned(), manifest)]);
    for (index, (folder, plan)) in expanded.iter().zip(&plans).enumerate() {
        let mut expected: HashSet<String> = plan.iter().map(|p| casefold(&p.source)).collect();
        if index == 0 {
            expected.insert("0".to_owned());
        }
        let mut actual = HashSet::new();
        for entry in fs::read_dir(folder).map_err(|e| mito_core::Error::io(folder, e))? {
            let entry = entry.map_err(|e| mito_core::Error::io(folder, e))?;
            let kind = entry
                .file_type()
                .map_err(|e| mito_core::Error::io(entry.path(), e))?;
            if kind.is_dir() {
                return Err(fail("CAB contents do not match manifest"));
            }
            if kind.is_file() {
                actual.insert(casefold(&entry.file_name().to_string_lossy()));
            }
        }
        if actual != expected {
            return Err(fail("CAB contents do not match manifest"));
        }
        for payload in plan {
            let file = inside(folder, &payload.source)?;
            verify(&file, payload)?;
            let prefix = if index == 0 {
                "UX/"
            } else {
                "AttachedContainer/"
            };
            let relative = format!("{prefix}{}", payload.target);
            let folded = casefold(&relative);
            if folded == "ux/manifest.xml" {
                return Err(fail("reserved manifest destination"));
            }
            if let Some(previous) = destinations.get(&folded) {
                if !same_content(previous, &file)? {
                    return Err(fail(&format!(
                        "conflicting payloads for destination: {relative}"
                    )));
                }
                log(Message::new("{p1}")
                    .arg("p1", format!("Burn: identical payload merged: {relative}")));
                continue;
            }
            destinations.insert(folded, file.clone());
            copies.push((file, relative));
        }
    }
    // Se publica solo cuando todo el bundle está validado.
    for (source, relative) in copies {
        let target = inside(destination, &relative)?;
        if let Some(folder) = target.parent() {
            fs::create_dir_all(folder).map_err(|e| mito_core::Error::io(folder, e))?;
        }
        fs::copy(&source, &target).map_err(|e| mito_core::Error::io(&target, e))?;
    }
    Ok(())
}

/// Copia `size` bytes desde `offset` de `stream` a `target` en bloques de 1 MiB.
fn copy_range(stream: &mut fs::File, offset: u64, size: u64, target: &Path) -> Result<()> {
    stream
        .seek(SeekFrom::Start(offset))
        .map_err(|e| mito_core::Error::io(target, e))?;
    let mut output = fs::File::create(target).map_err(|e| mito_core::Error::io(target, e))?;
    let mut left = size;
    let mut buffer = vec![0_u8; CHUNK];
    while left > 0 {
        let want = usize::try_from(left).map_or(CHUNK, |l| l.min(CHUNK));
        let count = stream
            .read(&mut buffer[..want])
            .map_err(|e| mito_core::Error::io(target, e))?;
        if count == 0 {
            return Err(fail("truncated CAB"));
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| mito_core::Error::io(target, e))?;
        left -= count as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::process::Command;

    use super::*;
    use crate::hash::{HashAlgorithm, Hasher};

    type Files<'a> = Vec<(&'a str, &'a [u8])>;

    /// Bundle sintético mínimo con `.wixburn` y dos CAB (`bundle()` de `test_burn.py`,
    /// con tamaños de contenedor variables).
    fn build(ux: &[u8], attached: &[u8], signed: bool) -> Vec<u8> {
        let mut data = vec![0_u8; 1024];
        data[..2].copy_from_slice(b"MZ");
        data[60..64].copy_from_slice(&128_u32.to_le_bytes());
        data[128..132].copy_from_slice(b"PE\0\0");
        data[134..136].copy_from_slice(&1_u16.to_le_bytes());
        data[148..150].copy_from_slice(&224_u16.to_le_bytes());
        data[376..384].copy_from_slice(b".wixburn");
        data[392..396].copy_from_slice(&512_u32.to_le_bytes());
        data[396..400].copy_from_slice(&512_u32.to_le_bytes());
        data[512..516].copy_from_slice(&0x00F1_4300_u32.to_le_bytes());
        data[516..520].copy_from_slice(&2_u32.to_le_bytes());
        let ux_end = 1024 + ux.len();
        let (signature, signature_size) = if signed { (ux_end, 20) } else { (0, 0) };
        let fields = [
            1024,
            0,
            signature,
            signature_size,
            1,
            2,
            ux.len(),
            attached.len(),
        ];
        for (index, value) in fields.iter().enumerate() {
            let value = u32::try_from(*value).unwrap();
            data[536 + index * 4..540 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        data.extend_from_slice(ux);
        data.extend_from_slice(&vec![0xEE; signature_size]);
        data.extend_from_slice(attached);
        data
    }

    /// CAB falso: solo la cabecera que valida `containers`.
    fn fake_cab(size: usize, flags: u16) -> Vec<u8> {
        let mut data = vec![0_u8; size];
        data[..4].copy_from_slice(b"MSCF");
        data[8..12].copy_from_slice(&u32::try_from(size).unwrap().to_le_bytes());
        data[30..32].copy_from_slice(&flags.to_le_bytes());
        data
    }

    fn bundle(signed: bool) -> Vec<u8> {
        build(&fake_cab(100, 0), &fake_cab(100, 0), signed)
    }

    fn patched(offset: usize, value: u32) -> Vec<u8> {
        let mut data = bundle(false);
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        data
    }

    fn sha1_hex(data: &[u8]) -> String {
        let mut hasher = Hasher::new(HashAlgorithm::Sha1);
        hasher.update(data);
        hasher.hex_digest()
    }

    fn manifest(target: &str, changes: &[(&str, &str)]) -> Vec<u8> {
        let mut attributes = vec![
            ("SourcePath", "a0".to_owned()),
            ("FilePath", target.to_owned()),
            ("Packaging", "embedded".to_owned()),
            ("FileSize", "4".to_owned()),
            ("Hash", sha1_hex(b"tool")),
        ];
        for (name, value) in changes {
            attributes.retain(|(n, _)| n != name);
            attributes.push((name, (*value).to_owned()));
        }
        let pairs: Vec<String> = attributes
            .iter()
            .map(|(name, value)| format!("{name}=\"{value}\""))
            .collect();
        format!(
            "<BurnManifest xmlns=\"{NS}\"><UX/><Payload {}/></BurnManifest>",
            pairs.join(" ")
        )
        .into_bytes()
    }

    fn quiet(manifest: &[u8]) -> Result<[Vec<Payload>; 2]> {
        inventory(manifest, &|_| {})
    }

    #[test]
    fn lee_los_contenedores_con_y_sin_firma() {
        for (signed, second) in [(false, 1124), (true, 1144)] {
            let mut stream = Cursor::new(bundle(signed));
            assert_eq!(
                containers(&mut stream).unwrap(),
                [(1024, 100), (second, 100)]
            );
        }
    }

    #[test]
    fn rechaza_contenedores_truncados_no_soportados_solapados_o_partidos() {
        let mut cases: Vec<(&str, Vec<u8>)> = vec![
            ("versión", patched(516, 3)),
            ("tipo", patched(552, 2)),
            ("cuenta", patched(556, 3)),
            ("ux vacío", patched(560, 0)),
            ("adjunto vacío", patched(564, 0)),
            ("firma solapada", patched(544, 1030)),
            ("stub dentro de la cabecera", patched(536, 100)),
            ("tamaño de CAB", patched(1032, 7)),
            ("spanning", patched(1054, 1)),
            ("secciones", {
                let mut data = bundle(false);
                data[134..136].copy_from_slice(&0_u16.to_le_bytes());
                data
            }),
            ("DOS", {
                let mut data = bundle(false);
                data[0] = b'X';
                data
            }),
            ("PE", {
                let mut data = bundle(false);
                data[128] = b'X';
                data
            }),
            ("duplicada", {
                let mut data = bundle(false);
                data[134..136].copy_from_slice(&2_u16.to_le_bytes());
                data[416..424].copy_from_slice(b".wixburn");
                data
            }),
            ("truncado", bundle(false)[..1150].to_vec()),
        ];
        // `raw_size` (offset 392) menor que 56.
        cases.push(("raw corto", patched(392, 10)));
        for (name, data) in cases {
            assert!(containers(&mut Cursor::new(data)).is_err(), "{name}");
        }
        assert_eq!(
            containers(&mut Cursor::new(patched(516, 3)))
                .unwrap_err()
                .to_string(),
            "Burn: only v2 bundles with UX and one attached CAB are supported"
        );
        assert_eq!(
            containers(&mut Cursor::new(bundle(false)[..1150].to_vec()))
                .unwrap_err()
                .to_string(),
            "Burn: truncated or oversized CAB"
        );
        assert_eq!(
            containers(&mut Cursor::new(bundle(false)[..100].to_vec()))
                .unwrap_err()
                .to_string(),
            "Burn: truncated or invalid container range"
        );
    }

    #[test]
    fn rechaza_manifiestos_inseguros_duplicados_y_xml_peligroso() {
        let bad: Vec<Vec<u8>> = vec![
            manifest("../escape", &[]),
            manifest("C:/escape", &[]),
            manifest("NUL.txt", &[]),
            manifest("x", &[("SourcePath", "../a0")]),
            manifest("x", &[("SourcePath", "dir/a0")]),
            manifest("x", &[("Packaging", "unknown")]),
            manifest("x", &[("FileSize", "-1")]),
            manifest("x", &[("FileSize", "mucho")]),
            manifest("x", &[("Hash", "bad")]),
            manifest(".", &[]),
            manifest("x", &[("SourcePath", "")]),
            manifest("x", &[("FilePath", "")]),
            // Packaging externo en el contenedor UX.
            manifest("x", &[]).replace_all(
                "<UX/>",
                "<UX><Payload SourcePath=\"u\" FilePath=\"u\" Packaging=\"external\"/></UX>",
            ),
            // Duplicado de a0 (otro destino, misma fuente en casefold).
            manifest("x", &[]).replace_all(
                "</BurnManifest>",
                "<Payload SourcePath=\"A0\" FilePath=\"y\"/></BurnManifest>",
            ),
            // Colisión archivo/directorio.
            manifest("nested/tool.msi", &[]).replace_all(
                "</BurnManifest>",
                "<Payload SourcePath=\"a1\" FilePath=\"nested/tool.msi/child\"/></BurnManifest>",
            ),
            b"<!DOCTYPE x [<!ENTITY e \"test\">]><BurnManifest/>".to_vec(),
            b"<BurnManifest xmlns=\"otro\"/>".to_vec(),
            b"<BurnManifest".to_vec(),
            b"\xFF\xFE".to_vec(),
            b"<a>\0</a>".to_vec(),
        ];
        for (index, data) in bad.iter().enumerate() {
            assert!(
                quiet(data).is_err(),
                "{index}: {}",
                String::from_utf8_lossy(data)
            );
        }
        assert_eq!(
            quiet(&vec![b' '; 17 * 1024 * 1024])
                .unwrap_err()
                .to_string(),
            "Burn: oversized manifest"
        );
        assert_eq!(
            quiet(b"<!DOCTYPE x><a/>").unwrap_err().to_string(),
            "Burn: unsupported XML declaration or oversized manifest"
        );
        assert_eq!(
            quiet(&manifest("x", &[("FileSize", "-1")]))
                .unwrap_err()
                .to_string(),
            "Burn: invalid payload size"
        );
    }

    trait ReplaceAll {
        fn replace_all(self, from: &str, to: &str) -> Vec<u8>;
    }

    impl ReplaceAll for Vec<u8> {
        fn replace_all(self, from: &str, to: &str) -> Vec<u8> {
            String::from_utf8(self)
                .unwrap()
                .replace(from, to)
                .into_bytes()
        }
    }

    #[test]
    fn el_inventario_acepta_bom_y_omite_los_externos_con_un_aviso() {
        let embedded = manifest("nested/tool.msi", &[]);
        let mixed = embedded.clone().replace_all(
            "</BurnManifest>",
            "<Payload Packaging=\"external\" SourcePath=\"remote/debug.msi\" FilePath=\"debug.msi\" DownloadUrl=\"https://example.invalid/debug.msi\"/></BurnManifest>",
        );
        let logs = RefCell::new(Vec::new());
        let plans = inventory(&mixed, &|m| logs.borrow_mut().push(m.to_string())).unwrap();
        assert_eq!(plans, quiet(&embedded).unwrap());
        assert_eq!(
            plans[1],
            [Payload {
                source: "a0".into(),
                target: "nested/tool.msi".into(),
                size: Some(4),
                digest: Some(sha1_hex(b"tool")),
            }]
        );
        assert_eq!(
            logs.take(),
            ["Burn: 1 external payload reference(s) omitted; extracting embedded files only"]
        );
        let mut with_bom = b"\xEF\xBB\xBF".to_vec();
        with_bom.extend(&embedded);
        assert_eq!(quiet(&with_bom).unwrap(), plans);
        // Un payload `external` del contenedor adjunto no valida su hash.
        let hashless = manifest("a", &[("Packaging", "External"), ("Hash", "bad")]);
        assert_eq!(quiet(&hashless).unwrap()[1], []);
    }

    /// Extractor de pruebas: escribe `files` en la carpeta del contenedor (`0` o `1`).
    fn fake<'a>(
        files: &'a [(&'a str, &'a [u8])],
        second: &'a [(&'a str, &'a [u8])],
        calls: &'a RefCell<Vec<(String, Limits)>>,
    ) -> impl Fn(&Path, &Path, &Limits) -> Result<()> + 'a {
        move |cabinet, folder, limits| {
            let name = cabinet.file_name().unwrap().to_string_lossy().into_owned();
            calls.borrow_mut().push((name.clone(), *limits));
            fs::create_dir_all(folder).unwrap();
            let chosen = if name == "0.cab" { files } else { second };
            for (name, content) in chosen {
                fs::write(folder.join(name), content).unwrap();
            }
            Ok(())
        }
    }

    fn run(
        folder: &Path,
        ux: &[(&str, &[u8])],
        attached: &[(&str, &[u8])],
    ) -> (Result<()>, Vec<String>, PathBuf) {
        let archive = folder.join("setup.exe");
        fs::write(&archive, bundle(false)).unwrap();
        let out = folder.join("stage").join("output");
        let calls = RefCell::new(Vec::new());
        let logs = RefCell::new(Vec::new());
        let extractor = fake(ux, attached, &calls);
        let result = extract_with(&archive, &out, &extractor, &|m| {
            logs.borrow_mut().push(m.to_string());
        });
        (result, logs.take(), out)
    }

    #[test]
    fn publica_ux_y_adjuntos_solo_tras_validar_todo() {
        let folder = tempfile::tempdir().unwrap();
        let manifest = manifest("nested/tool.msi", &[]);
        let (result, logs, out) = run(folder.path(), &[("0", &manifest)], &[("a0", b"tool")]);
        result.unwrap();
        assert_eq!(logs, Vec::<String>::new());
        assert_eq!(fs::read(out.join("UX/manifest.xml")).unwrap(), manifest);
        assert_eq!(
            fs::read(out.join("AttachedContainer/nested/tool.msi")).unwrap(),
            b"tool"
        );
        // No quedan restos del directorio de trabajo.
        let names: Vec<_> = fs::read_dir(folder.path().join("stage"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["output"]);
    }

    #[test]
    fn los_fallos_no_crean_el_destino() {
        let folder = tempfile::tempdir().unwrap();
        let good = manifest("nested/tool.msi", &[]);
        let cases: Vec<(&str, Files, Files, &str)> = vec![
            (
                "tamaño",
                vec![("0", &good)],
                vec![("a0", b"evil!")],
                "Burn: payload size mismatch",
            ),
            (
                "hash",
                vec![("0", &good)],
                vec![("a0", b"evil")],
                "Burn: payload hash mismatch",
            ),
            (
                "de más",
                vec![("0", &good)],
                vec![("a0", b"tool"), ("unknown", b"x")],
                "Burn: CAB contents do not match manifest",
            ),
            (
                "sin a0",
                vec![("0", &good)],
                vec![],
                "Burn: CAB contents do not match manifest",
            ),
            (
                "sin manifiesto",
                vec![],
                vec![("a0", b"tool")],
                "Burn: missing or oversized manifest",
            ),
        ];
        for (index, (name, ux, attached, expected)) in cases.into_iter().enumerate() {
            let sub = folder.path().join(index.to_string());
            fs::create_dir_all(&sub).unwrap();
            let (result, _, out) = run(&sub, &ux, &attached);
            assert_eq!(result.unwrap_err().to_string(), expected, "{name}");
            assert!(!out.exists(), "{name}");
        }
    }

    #[test]
    fn un_destino_repetido_exige_bytes_identicos() {
        let folder = tempfile::tempdir().unwrap();
        // Dos fuentes con el mismo destino en casefold, con y sin hash.
        let manifest = |hash: &str| -> Vec<u8> {
            let hash = if hash.is_empty() {
                String::new()
            } else {
                format!(" Hash=\"{hash}\"")
            };
            format!(
                "<BurnManifest xmlns=\"{NS}\"><UX/>\
                 <Payload SourcePath=\"a0\" FilePath=\"nested/tool.msi\" FileSize=\"4\" Hash=\"{}\"/>\
                 <Payload SourcePath=\"a1\" FilePath=\"NESTED/TOOL.MSI\" FileSize=\"4\"{hash}/>\
                 </BurnManifest>",
                sha1_hex(b"tool")
            )
            .into_bytes()
        };
        let same = manifest(&sha1_hex(b"tool"));
        let sub = folder.path().join("same");
        fs::create_dir_all(&sub).unwrap();
        let (result, logs, out) = run(&sub, &[("0", &same)], &[("a0", b"tool"), ("a1", b"tool")]);
        result.unwrap();
        assert_eq!(
            logs,
            ["Burn: identical payload merged: AttachedContainer/NESTED/TOOL.MSI"]
        );
        assert!(out.join("AttachedContainer/nested/tool.msi").is_file());
        // Distintos bytes y sin hash en la segunda.
        let different = manifest("");
        let sub = folder.path().join("different");
        fs::create_dir_all(&sub).unwrap();
        let (result, _, out) = run(
            &sub,
            &[("0", &different)],
            &[("a0", b"tool"), ("a1", b"evil")],
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "Burn: conflicting payloads for destination: AttachedContainer/NESTED/TOOL.MSI"
        );
        assert!(!out.exists());
    }

    #[test]
    fn el_destino_del_manifiesto_esta_reservado() {
        let folder = tempfile::tempdir().unwrap();
        let reserved = format!(
            "<BurnManifest xmlns=\"{NS}\"><UX><Payload SourcePath=\"u0\" FilePath=\"Manifest.XML\"/></UX></BurnManifest>"
        )
        .into_bytes();
        let (result, _, out) = run(folder.path(), &[("0", &reserved), ("u0", b"x")], &[]);
        assert_eq!(
            result.unwrap_err().to_string(),
            "Burn: reserved manifest destination"
        );
        assert!(!out.exists());
    }

    #[test]
    fn un_adjunto_que_supera_el_presupuesto_se_rechaza() {
        let calls = RefCell::new(Vec::new());
        let folder = tempfile::tempdir().unwrap();
        let archive = folder.path().join("setup.exe");
        fs::write(&archive, bundle(false)).unwrap();
        let manifest = manifest("x", &[]);
        let ux: [(&str, &[u8]); 1] = [("0", &manifest)];
        let attached: [(&str, &[u8]); 1] = [("a0", b"tool")];
        let extractor = fake(&ux, &attached, &calls);
        extract_with(&archive, &folder.path().join("o"), &extractor, &|_| {}).unwrap();
        let calls = calls.take();
        assert_eq!(calls[0].1.max_expanded, MAX_EXPANDED);
        // El segundo CAB recibe el presupuesto restante tras el primero.
        assert_eq!(
            calls[1].1.max_expanded,
            MAX_EXPANDED - manifest.len() as u64
        );
        assert_eq!(calls[1].1.max_entries, MAX_ENTRIES);
    }

    fn real_7zip() -> Option<PathBuf> {
        [
            std::env::var_os("MITO_7ZIP").map(PathBuf::from),
            Some(PathBuf::from(r"C:\Program Files\7-Zip\7z.exe")),
            Some(PathBuf::from(r"C:\Program Files (x86)\7-Zip\7z.exe")),
        ]
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
    }

    fn make_cab(source: &Path, target: &Path) -> Vec<u8> {
        let made = Command::new(r"C:\Windows\System32\makecab.exe")
            .arg(source)
            .arg(target)
            .output()
            .unwrap();
        assert!(made.status.success(), "makecab falló");
        fs::read(target).unwrap()
    }

    #[test]
    fn bundle_real_con_makecab_y_7zip() {
        let Some(sevenzip) = real_7zip() else {
            eprintln!("se omite: no hay 7-Zip (define MITO_7ZIP)");
            return;
        };
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let mixed = manifest("nested/tool.msi", &[]).replace_all(
            "</BurnManifest>",
            "<Payload Packaging=\"external\" SourcePath=\"remote/debug.msi\" FilePath=\"debug.msi\" DownloadUrl=\"https://example.invalid/debug.msi\"/></BurnManifest>",
        );
        fs::write(root.join("0"), &mixed).unwrap();
        fs::write(root.join("a0"), b"tool").unwrap();
        let ux = make_cab(&root.join("0"), &root.join("0.cab"));
        let attached = make_cab(&root.join("a0"), &root.join("a0.cab"));
        let archive = root.join("setup.exe");
        fs::write(&archive, build(&ux, &attached, true)).unwrap();
        let out = root.join("stage").join("output");
        let logs = RefCell::new(Vec::new());
        extract_burn(&archive, &out, Some(&sevenzip), &|m| {
            logs.borrow_mut().push(m.to_string());
        })
        .unwrap();
        assert_eq!(
            fs::read(out.join("AttachedContainer/nested/tool.msi")).unwrap(),
            b"tool"
        );
        assert_eq!(fs::read(out.join("UX/manifest.xml")).unwrap(), mixed);
        assert_eq!(logs.take().len(), 1);
        // Sin 7-Zip falla y no publica.
        let out = root.join("stage").join("sin-7zip");
        assert_eq!(
            extract_burn(&archive, &out, None, &|_| {})
                .unwrap_err()
                .to_string(),
            "Falta el extractor privado 7-Zip; ejecuta bootstrap"
        );
        assert!(!out.exists());
    }
}
