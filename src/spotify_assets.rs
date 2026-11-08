//! Reads UI assets out of the user's *own* local Spotify install at runtime
//! (`%APPDATA%\Spotify\Apps\*.spa`, which are plain zip archives).
//! Nothing read here is bundled into or committed with Spotirust.

use std::{io::Read, path::PathBuf};

use anyhow::{Context as _, Result, anyhow};
use image::RgbaImage;

pub fn spa_path(name: &str) -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let p = PathBuf::from(appdata).join("Spotify").join("Apps").join(name);
    p.exists().then_some(p)
}

pub struct Spa {
    zip: zip::ZipArchive<std::fs::File>,
}

impl Spa {
    pub fn open(name: &str) -> Result<Self> {
        let path = spa_path(name).ok_or_else(|| anyhow!("{name} not found in Spotify install"))?;
        let file = std::fs::File::open(&path)?;
        Ok(Self { zip: zip::ZipArchive::new(file)? })
    }

    pub fn read(&mut self, entry: &str) -> Result<Vec<u8>> {
        let mut f = self.zip.by_name(entry).with_context(|| format!("missing {entry}"))?;
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)?;
        Ok(buf)
    }

    pub fn image(&mut self, entry: &str) -> Result<RgbaImage> {
        let bytes = self.read(entry)?;
        if entry.ends_with(".png") {
            Ok(image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?.to_rgba8())
        } else if entry.ends_with(".avif") {
            // WIC drops AVIF's auxiliary alpha plane; zenavif keeps it.
            avif_decode(&bytes).or_else(|e| {
                eprintln!("{entry}: zenavif failed ({e:#}), falling back to WIC (no alpha)");
                wic_decode(&bytes)
            })
        } else {
            wic_decode(&bytes)
        }
    }
}

/// Alpha-aware AVIF decode (pure Rust: rav1d).
pub fn avif_decode(bytes: &[u8]) -> Result<RgbaImage> {
    use zenpixels::PixelDescriptor;
    let config = zenavif::DecoderConfig::new();
    let img = zenavif::decode_with(bytes, &config, &zenavif::Unstoppable)
        .map_err(|e| anyhow!("{e:?}"))?;
    let desc = img.descriptor();
    if desc.layout_compatible(PixelDescriptor::RGBA8) {
        let buf = img.try_as_imgref::<rgb::Rgba<u8>>().ok_or_else(|| anyhow!("rgba view"))?;
        let (w, h) = (buf.width() as u32, buf.height() as u32);
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in buf.rows() {
            for p in row {
                out.extend_from_slice(&[p.r, p.g, p.b, p.a]);
            }
        }
        RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("bad rgba buffer"))
    } else if desc.layout_compatible(PixelDescriptor::RGB8) {
        let buf = img.try_as_imgref::<rgb::Rgb<u8>>().ok_or_else(|| anyhow!("rgb view"))?;
        let (w, h) = (buf.width() as u32, buf.height() as u32);
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in buf.rows() {
            for p in row {
                out.extend_from_slice(&[p.r, p.g, p.b, 255]);
            }
        }
        RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("bad rgb buffer"))
    } else {
        Err(anyhow!("unsupported AVIF pixel layout {desc:?}"))
    }
}

/// Decode any format Windows Imaging Component understands (AVIF, WebP, HEIF, ...).
pub fn wic_decode(bytes: &[u8]) -> Result<RgbaImage> {
    use windows::Win32::Graphics::Imaging::*;
    use windows::Win32::System::Com::*;

    unsafe {
        // May already be initialised on this thread; any mode is fine for WIC.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = factory.CreateStream()?;
        stream.InitializeFromMemory(bytes)?;
        let decoder =
            factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let conv = factory.CreateFormatConverter()?;
        conv.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let (mut w, mut h) = (0u32, 0u32);
        conv.GetSize(&mut w, &mut h)?;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        conv.CopyPixels(std::ptr::null(), w * 4, &mut buf)?;
        RgbaImage::from_raw(w, h, buf).ok_or_else(|| anyhow!("bad WIC buffer"))
    }
}

/// Family names resolved after loading fonts (falls back to bundled Figtree).
pub struct LoadedFonts {
    pub ui: String,
    pub title: String,
    pub data: Vec<Vec<u8>>,
}

fn family_name(ttf: &[u8]) -> Option<String> {
    let face = ttf_parser::Face::parse(ttf, 0).ok()?;
    let names = face.names();
    let pick = |id: u16| {
        names
            .into_iter()
            .find(|n| n.name_id == id && n.is_unicode())
            .and_then(|n| n.to_string())
    };
    pick(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| pick(ttf_parser::name_id::FAMILY))
}

pub fn load_spotify_fonts() -> Result<LoadedFonts> {
    let mut spa = Spa::open("login.spa").or_else(|_| Spa::open("xpui.spa"))?;
    let mut data = Vec::new();
    let (mut ui, mut title) = (None, None);
    for (entry, is_title) in [
        ("fonts/SpotifyMixUI-Regular.woff2", false),
        ("fonts/SpotifyMixUI-Bold.woff2", false),
        ("fonts/SpotifyMixUITitle-Bold.woff2", true),
        ("fonts/SpotifyMixUITitle-Extrabold.woff2", true),
    ] {
        let woff2 = spa.read(entry)?;
        let ttf = woff2_patched::convert_woff2_to_ttf(&mut woff2.as_slice())
            .map_err(|e| anyhow!("{entry}: {e:?}"))?;
        let fam = family_name(&ttf);
        if is_title { title = title.or(fam) } else { ui = ui.or(fam) }
        data.push(ttf);
    }
    Ok(LoadedFonts {
        ui: ui.ok_or_else(|| anyhow!("no UI family name"))?,
        title: title.ok_or_else(|| anyhow!("no title family name"))?,
        data,
    })
}
