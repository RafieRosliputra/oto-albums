//! UI strings: English is the key; Bahasa Melayu translations live in this table.
//! Untranslated strings fall back to English.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Lang {
    #[default]
    En,
    Ms,
}

impl Lang {
    pub fn label(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ms => "Bahasa Melayu",
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(l: Lang) {
    LANG.store(if l == Lang::Ms { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 {
        Lang::Ms
    } else {
        Lang::En
    }
}

const MS: &[(&str, &str)] = &[
    // Hub & onboarding
    ("Design the story. Print it right.", "Reka cerita. Cetak dengan betul."),
    ("Projects", "Projek"),
    ("New project", "Projek baharu"),
    ("Open project…", "Buka projek…"),
    ("Open sample project", "Buka projek contoh"),
    ("Create your first album", "Buat album pertama"),
    ("Recent projects", "Projek terkini"),
    ("Search projects", "Cari projek"),
    ("No recent projects yet.", "Belum ada projek terkini."),
    ("Due", "Tarikh akhir"),
    ("Status", "Status"),
    ("Remove from list", "Buang dari senarai"),
    ("Show in Finder", "Tunjuk dalam Finder"),
    ("Settings", "Tetapan"),
    ("Language", "Bahasa"),
    ("Theme", "Tema"),
    ("Dark", "Gelap"),
    ("Light", "Cerah"),
    ("High contrast", "Kontras tinggi"),
    ("Reduce motion", "Kurangkan animasi"),
    ("UI scale", "Skala UI"),
    ("Units", "Unit"),
    ("Close", "Tutup"),
    ("Cancel", "Batal"),
    ("Apply", "Guna"),
    ("Create", "Cipta"),
    ("Back", "Kembali"),
    ("Next", "Seterusnya"),
    ("Done", "Selesai"),
    ("Delete", "Padam"),
    ("Duplicate", "Gandakan"),
    ("Rename", "Namakan semula"),
    ("Getting started", "Mula di sini"),
    ("Choose size", "Pilih saiz"),
    ("Import photos", "Import foto"),
    ("Edit", "Sunting"),
    ("Export", "Eksport"),
    ("Hide", "Sembunyi"),
    // Wizard
    ("Album size", "Saiz album"),
    ("Project name", "Nama projek"),
    ("Client", "Klien"),
    ("Save in", "Simpan dalam"),
    ("Choose…", "Pilih…"),
    ("Lab preset", "Pratetap makmal"),
    ("Custom size", "Saiz tersuai"),
    ("Size", "Saiz"),
    ("Page width", "Lebar halaman"),
    ("Page height", "Tinggi halaman"),
    ("Bleed", "Bleed"),
    ("Safe zone", "Zon selamat"),
    ("Gutter", "Gutter"),
    ("Trim tolerance", "Toleransi potong"),
    ("Inside trim", "Potong dalam"),
    ("Pages min", "Halaman min"),
    ("Pages max", "Halaman maks"),
    ("Page step", "Langkah halaman"),
    ("Spreads", "Spread"),
    ("Cover", "Kulit"),
    ("Binding", "Jilid"),
    ("Layflat (spreads)", "Layflat (spread)"),
    ("Single pages", "Halaman tunggal"),
    ("Output profile", "Profil output"),
    ("Template style", "Gaya templat"),
    ("Resolution", "Resolusi"),
    ("Example profile — not verified by a real lab.", "Profil contoh — belum disahkan oleh makmal sebenar."),
    ("Fix these before continuing:", "Baiki ini sebelum teruskan:"),
    ("Create project", "Cipta projek"),
    // Workspace
    ("Undo", "Buat asal"),
    ("Redo", "Buat semula"),
    ("Auto Build", "Bina Auto"),
    ("Auto Build…", "Bina Auto…"),
    ("Preflight", "Preflight"),
    ("Proof", "Proof"),
    ("Saved", "Disimpan"),
    ("Saving…", "Menyimpan…"),
    ("Unsaved changes", "Perubahan belum disimpan"),
    ("Book map", "Peta buku"),
    ("Timeline", "Garis masa"),
    ("Photos", "Foto"),
    ("Templates", "Templat"),
    ("Spread", "Spread"),
    ("Image", "Imej"),
    ("Text", "Teks"),
    ("Layers", "Lapisan"),
    ("Add spread", "Tambah spread"),
    ("Insert blank spread", "Sisip spread kosong"),
    ("Insert story page", "Sisip halaman cerita"),
    ("Delete spread", "Padam spread"),
    ("Lock spread", "Kunci spread"),
    ("Unlock spread", "Buka kunci spread"),
    ("Auto Build from here", "Bina Auto dari sini"),
    ("Reflow from here", "Susun semula dari sini"),
    ("Duplicate spread", "Gandakan spread"),
    ("All", "Semua"),
    ("Used", "Digunakan"),
    ("Unused", "Belum digunakan"),
    ("Missing", "Hilang"),
    ("Rejected", "Ditolak"),
    ("Client picks", "Pilihan klien"),
    ("Add to timeline", "Tambah ke garis masa"),
    ("Reject", "Tolak"),
    ("Unreject", "Batal tolak"),
    ("Pick", "Pilih"),
    ("Rating", "Penarafan"),
    ("Label", "Label"),
    ("Bin", "Bin"),
    ("Add to bin", "Tambah ke bin"),
    ("Sort", "Susun"),
    ("Name", "Nama"),
    ("Capture time", "Masa ambil"),
    ("Modified", "Diubah"),
    ("Search", "Cari"),
    ("Import folder…", "Import folder…"),
    ("Import files…", "Import fail…"),
    ("Relink…", "Pautkan semula…"),
    ("Not placed yet", "Belum diletak"),
    ("Place as new spreads", "Letak sebagai spread baharu"),
    ("Too many photos on this spread", "Terlalu banyak foto pada spread ini"),
    ("Background", "Latar"),
    ("Pattern", "Corak"),
    ("None", "Tiada"),
    ("Color", "Warna"),
    ("Opacity", "Kelegapan"),
    ("Scale", "Skala"),
    ("Angle", "Sudut"),
    ("Notes", "Nota"),
    ("Fit", "Muat"),
    ("Fill", "Isi"),
    ("Zoom", "Zum"),
    ("Rotation", "Putaran"),
    ("Reset crop", "Set semula crop"),
    ("Focus point", "Titik fokus"),
    ("Border", "Bingkai"),
    ("Radius", "Jejari"),
    ("Brightness", "Kecerahan"),
    ("Contrast", "Kontras"),
    ("Black & white", "Hitam putih"),
    ("Lock", "Kunci"),
    ("Bring forward", "Bawa ke depan"),
    ("Send backward", "Hantar ke belakang"),
    ("Font", "Fon"),
    ("Bold", "Tebal"),
    ("Italic", "Condong"),
    ("Align", "Jajar"),
    ("Line height", "Tinggi baris"),
    ("Letter spacing", "Jarak huruf"),
    ("Kerning", "Kerning"),
    ("Style", "Gaya"),
    ("Save as template", "Simpan sebagai templat"),
    ("Select a photo frame to edit it.", "Pilih bingkai foto untuk menyunting."),
    ("Select a text box to edit it.", "Pilih kotak teks untuk menyunting."),
    ("Frame", "Bingkai"),
    ("Shape", "Bentuk"),
    ("Select", "Pilih"),
    ("Crop", "Crop"),
    ("Compare", "Banding"),
    ("Effective DPI", "DPI berkesan"),
    ("Used on", "Digunakan pada"),
    // Dialogs
    ("Source", "Sumber"),
    ("All unused photos", "Semua foto belum digunakan"),
    ("Selected photos", "Foto dipilih"),
    ("Timeline (not placed)", "Garis masa (belum diletak)"),
    ("Number of spreads", "Bilangan spread"),
    ("Automatic", "Automatik"),
    ("Photos per spread", "Foto setiap spread"),
    ("Grouping", "Kumpulan"),
    ("Time blocks", "Blok masa"),
    ("Visual similarity", "Persamaan visual"),
    ("Template reuse", "Guna semula templat"),
    ("Low", "Rendah"),
    ("Medium", "Sederhana"),
    ("High", "Tinggi"),
    ("Shuffle", "Kocok"),
    ("Build", "Bina"),
    ("Apply variant", "Guna variasi"),
    ("Why this layout?", "Kenapa susun atur ini?"),
    ("Critical", "Kritikal"),
    ("Warning", "Amaran"),
    ("Info", "Info"),
    ("Why", "Kenapa"),
    ("Fix", "Baiki"),
    ("Go to", "Pergi ke"),
    ("Override…", "Abaikan…"),
    ("Remove override", "Buang pengabaian"),
    ("Reason", "Sebab"),
    ("No issues. Ready to print.", "Tiada isu. Sedia untuk cetak."),
    ("Export blocked", "Eksport disekat"),
    ("I have reviewed the warnings", "Saya telah semak amaran"),
    ("Output folder", "Folder output"),
    ("Preset", "Pratetap"),
    ("Include cover", "Sertakan kulit"),
    ("Crop marks", "Tanda potong"),
    ("Spec sheet", "Helaian spesifikasi"),
    ("JPEG per spread", "JPEG setiap spread"),
    ("TIFF per spread", "TIFF setiap spread"),
    ("Lab package (.otopack)", "Pakej makmal (.otopack)"),
    ("Image quality", "Kualiti imej"),
    ("Keep originals", "Kekalkan asal"),
    ("Downsample", "Kurangkan resolusi"),
    ("Export finished", "Eksport selesai"),
    ("Reveal", "Tunjuk"),
    ("Local proof", "Proof tempatan"),
    ("Make proof", "Buat proof"),
    ("Snapshots", "Snapshot"),
    ("Take snapshot", "Ambil snapshot"),
    ("Restore", "Pulihkan"),
    ("Recover work", "Pulihkan kerja"),
    ("Continue", "Teruskan"),
    ("Command palette", "Palet arahan"),
    ("Keyboard shortcuts", "Pintasan papan kekunci"),
    ("Save", "Simpan"),
    ("Save as…", "Simpan sebagai…"),
    ("Close project", "Tutup projek"),
    ("Project info", "Info projek"),
    ("Album settings", "Tetapan album"),
    ("Edit cover", "Sunting kulit"),
    ("Back to spreads", "Kembali ke spread"),
    ("Focus mode", "Mod fokus"),
    ("Full screen", "Skrin penuh"),
    ("Grid", "Grid"),
    ("Guides", "Panduan"),
    ("Rulers", "Pembaris"),
    ("Continuous view", "Paparan berterusan"),
    ("Fit to screen", "Muat skrin"),
    ("Importing", "Mengimport"),
    ("Exporting", "Mengeksport"),
];

fn table() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| MS.iter().copied().collect())
}

/// Translate a UI string.
pub fn t(s: &'static str) -> &'static str {
    if lang() == Lang::Ms {
        table().get(s).copied().unwrap_or(s)
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_and_falls_back() {
        set_lang(Lang::Ms);
        assert_eq!(t("Auto Build"), "Bina Auto");
        assert_eq!(t("Something new"), "Something new");
        set_lang(Lang::En);
        assert_eq!(t("Auto Build"), "Auto Build");
        let keys: std::collections::HashSet<_> = MS.iter().map(|(k, _)| k).collect();
        assert_eq!(keys.len(), MS.len(), "duplicate keys in MS table");
    }
}
