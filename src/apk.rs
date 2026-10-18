//! APK file helpers: reading and patching `AndroidManifest.xml`, and re-signing.

use std::{
    fs::OpenOptions,
    io::{Cursor, Read, Seek},
    path::Path,
};

use color_eyre::eyre::{Context, ContextCompat, bail, eyre};
use mbf_axml::{AxmlReader, AxmlWriter, axml_to_xml, xml_to_axml};
use mbf_zip::{FileCompression, ZipFile};

pub const MANIFEST_FILE: &str = "AndroidManifest.xml";
const CERT_PEM: &[u8] = include_bytes!("debug_cert.pem");

/// Opens `path` as an APK for reading.
pub fn open(path: &Path) -> color_eyre::Result<ZipFile<std::fs::File>> {
    let file = std::fs::File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    ZipFile::open(file)
        .map_err(|a| eyre!(a))
        .with_context(|| format!("{} is not a valid APK (zip)", path.display()))
}

/// Reads the raw (binary AXML) `AndroidManifest.xml` out of `apk`.
pub fn read_manifest<T: Read + Seek>(apk: &mut ZipFile<T>) -> color_eyre::Result<Vec<u8>> {
    apk.read_file(MANIFEST_FILE)
        .map_err(|a| eyre!(a))
        .context("Failed to read AndroidManifest.xml from APK")
}

/// Converts binary AXML into plain XML text.
pub fn axml_to_string(axml_bytes: Vec<u8>) -> color_eyre::Result<String> {
    let mut cursor = Cursor::new(axml_bytes);
    let mut axml_reader = AxmlReader::new(&mut cursor)
        .map_err(|a| eyre!(a))
        .context("Failed to parse AndroidManifest.xml as AXML")?;
    let mut xml_bytes = Vec::new();
    {
        let mut writer = xml::EventWriter::new(&mut xml_bytes);
        axml_to_xml(&mut writer, &mut axml_reader).map_err(|a| eyre!(a))?;
    }
    String::from_utf8(xml_bytes).context("Converted AndroidManifest.xml is not valid UTF-8")
}

/// Converts plain XML text back into binary AXML.
pub fn string_to_axml(xml_str: &str) -> color_eyre::Result<Vec<u8>> {
    let mut axml_bytes = Vec::new();
    {
        let mut axml_writer = AxmlWriter::new(&mut axml_bytes);
        let mut xml_reader = xml::EventReader::from_str(xml_str);
        xml_to_axml(&mut axml_writer, &mut xml_reader).map_err(|a| eyre!(a))?;
        axml_writer.finish().map_err(|a| eyre!(a))?;
    }
    Ok(axml_bytes)
}

/// Reads the `package` attribute of the APK's `<manifest>`.
pub fn read_package_name(apk_path: &Path) -> color_eyre::Result<String> {
    let mut apk = open(apk_path)?;
    let xml_str = axml_to_string(read_manifest(&mut apk)?)?;

    for event in xml::EventReader::from_str(&xml_str) {
        if let xml::reader::XmlEvent::StartElement {
            name, attributes, ..
        } = event.context("Failed to read converted AndroidManifest.xml")?
            && name.local_name == "manifest"
        {
            return attributes
                .into_iter()
                .find(|attr| attr.name.local_name == "package")
                .map(|attr| attr.value)
                .context("AndroidManifest.xml has no package attribute");
        }
    }
    bail!("AndroidManifest.xml has no <manifest> element")
}

/// Patches the APK at `path` in place to work in the emulator (see
/// `patch_manifest`), then re-signs it with the bundled debug certificate.
pub fn patch(path: &Path) -> color_eyre::Result<()> {
    println!("Patching APK from path: {path:?}");
    let apk_file = OpenOptions::new()
        .write(true)
        .read(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let mut apk = ZipFile::open(apk_file)
        .map_err(|a| eyre!(a))
        .context("Failed to read APK as zip file")?;
    let axml_bytes = patch_manifest(read_manifest(&mut apk)?)?;
    let mut axml_cursor = Cursor::new(axml_bytes);
    apk.write_file(MANIFEST_FILE, &mut axml_cursor, FileCompression::Store)
        .map_err(|a| eyre!(a))
        .context("Failed to write modified AndroidManifest.xml back to APK")?;
    let (cert, priv_key) = mbf_zip::signing::load_cert_and_priv_key(CERT_PEM);
    apk.save_and_sign_v2(&priv_key, &cert)
        .map_err(|a| eyre!(a))
        .context("Failed to save modified APK")?;
    println!("Successfully patched AndroidManifest.xml");
    Ok(())
}

/// AI generated code to patch the manifest
/// I'm too lazy to do it myself
/// since it's XML :(
pub fn patch_manifest(manifest_bytes: Vec<u8>) -> color_eyre::Result<Vec<u8>> {
    let mut xml_str = axml_to_string(manifest_bytes)?;
    let insert_str = r#"  <queries>    <package android:name="com.oculus.horizon"/>  </queries>"#;

    // Check if the <queries> block with the package is already present
    if !xml_str.contains(r#"<package android:name="com.oculus.horizon""#) {
        match xml_str.rfind("</manifest>") {
            Some(idx) => {
                xml_str.insert_str(idx, insert_str);
            }
            None => {
                bail!("No </manifest> tag found in manifest");
            }
        }
    }
    string_to_axml(&xml_str)
}
