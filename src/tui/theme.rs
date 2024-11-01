use cursive::theme::*;
use cursive::Cursive;

pub fn theme1() -> Theme {
    let mut palette = Palette::default();
    palette[PaletteColor::Background] = Color::Dark(BaseColor::Black);
    palette[PaletteColor::View] = Color::Dark(BaseColor::Black);
    palette[PaletteColor::Primary] = Color::Light(BaseColor::White);
    palette[PaletteColor::TitlePrimary] = Color::Light(BaseColor::Yellow);
    palette[PaletteColor::Highlight] = Color::Dark(BaseColor::Red);
    palette[PaletteColor::HighlightText] = Color::Dark(BaseColor::Black);

    return Theme {
        shadow: false,
        borders: BorderStyle::None,
        palette,
    };
}

pub fn set_highlight_disabled(siv: &mut Cursive) {
    siv.with_theme(|theme| {
        theme.palette[PaletteColor::Highlight] = Color::Rgb(50, 50, 50);
        theme.palette[PaletteColor::HighlightText] = Color::Dark(BaseColor::White);
    });
}
