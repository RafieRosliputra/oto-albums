//! Bundled album fonts (all SIL Open Font License 1.1, see assets/fonts/*-OFL.txt).

macro_rules! font {
    ($f:literal) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/", $f))
    };
}

pub struct FontFace {
    pub family: &'static str,
    pub postscript: &'static str,
    pub bold: bool,
    pub italic: bool,
    pub data: &'static [u8],
}

pub static FACES: &[FontFace] = &[
    FontFace { family: "Inter", postscript: "Inter-Regular", bold: false, italic: false, data: font!("Inter-Regular.ttf") },
    FontFace { family: "Inter", postscript: "Inter-Bold", bold: true, italic: false, data: font!("Inter-Bold.ttf") },
    FontFace { family: "Inter", postscript: "Inter-Italic", bold: false, italic: true, data: font!("Inter-Italic.ttf") },
    FontFace { family: "Inter", postscript: "Inter-BoldItalic", bold: true, italic: true, data: font!("Inter-BoldItalic.ttf") },
    FontFace { family: "Lora", postscript: "Lora-Regular", bold: false, italic: false, data: font!("Lora-Regular.ttf") },
    FontFace { family: "Lora", postscript: "Lora-Bold", bold: true, italic: false, data: font!("Lora-Bold.ttf") },
    FontFace { family: "Lora", postscript: "Lora-Italic", bold: false, italic: true, data: font!("Lora-Italic.ttf") },
    FontFace { family: "Lora", postscript: "Lora-BoldItalic", bold: true, italic: true, data: font!("Lora-BoldItalic.ttf") },
    FontFace { family: "Playfair Display", postscript: "PlayfairDisplay-Regular", bold: false, italic: false, data: font!("PlayfairDisplay-Regular.ttf") },
    FontFace { family: "Playfair Display", postscript: "PlayfairDisplay-Bold", bold: true, italic: false, data: font!("PlayfairDisplay-Bold.ttf") },
    FontFace { family: "Playfair Display", postscript: "PlayfairDisplay-Italic", bold: false, italic: true, data: font!("PlayfairDisplay-Italic.ttf") },
    FontFace { family: "Playfair Display", postscript: "PlayfairDisplay-BoldItalic", bold: true, italic: true, data: font!("PlayfairDisplay-BoldItalic.ttf") },
    FontFace { family: "Cormorant Garamond", postscript: "CormorantGaramond-Regular", bold: false, italic: false, data: font!("CormorantGaramond-Regular.ttf") },
    FontFace { family: "Cormorant Garamond", postscript: "CormorantGaramond-Bold", bold: true, italic: false, data: font!("CormorantGaramond-Bold.ttf") },
    FontFace { family: "Cormorant Garamond", postscript: "CormorantGaramond-Italic", bold: false, italic: true, data: font!("CormorantGaramond-Italic.ttf") },
    FontFace { family: "Cormorant Garamond", postscript: "CormorantGaramond-BoldItalic", bold: true, italic: true, data: font!("CormorantGaramond-BoldItalic.ttf") },
    FontFace { family: "Montserrat", postscript: "Montserrat-Regular", bold: false, italic: false, data: font!("Montserrat-Regular.ttf") },
    FontFace { family: "Montserrat", postscript: "Montserrat-Bold", bold: true, italic: false, data: font!("Montserrat-Bold.ttf") },
    FontFace { family: "Montserrat", postscript: "Montserrat-Italic", bold: false, italic: true, data: font!("Montserrat-Italic.ttf") },
    FontFace { family: "Montserrat", postscript: "Montserrat-BoldItalic", bold: true, italic: true, data: font!("Montserrat-BoldItalic.ttf") },
    FontFace { family: "Great Vibes", postscript: "GreatVibes-Regular", bold: false, italic: false, data: font!("GreatVibes-Regular.ttf") },
];

/// UI font (Inter SemiBold is only used by the UI).
pub static INTER_SEMIBOLD: &[u8] = font!("Inter-SemiBold.ttf");

/// sRGB output profile generated for oto-albums (LittleCMS built-in sRGB).
pub static SRGB_ICC: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/icc/sRGB-oto.icc"));

pub fn is_known_family(family: &str) -> bool {
    FACES.iter().any(|f| f.family == family)
}

/// Best face for a style; falls back to the closest style of the family, then to Inter.
pub fn face(family: &str, bold: bool, italic: bool) -> (usize, &'static FontFace) {
    let exact = FACES.iter().position(|f| f.family == family && f.bold == bold && f.italic == italic);
    let fam = || FACES.iter().position(|f| f.family == family && f.bold == bold).or_else(|| FACES.iter().position(|f| f.family == family));
    let i = exact.or_else(fam).unwrap_or(0);
    (i, &FACES[i])
}
