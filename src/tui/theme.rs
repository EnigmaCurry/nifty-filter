use cursive::theme::*;

pub fn theme1() -> Theme {
    let mut palette = Palette::default();
    palette[PaletteColor::Background] = Color::Dark(BaseColor::Black);
    palette[PaletteColor::View] = Color::Dark(BaseColor::Black);
    palette[PaletteColor::Primary] = Color::Light(BaseColor::White);
    palette[PaletteColor::TitlePrimary] = Color::Light(BaseColor::Yellow);
    palette[PaletteColor::Highlight] = Color::Dark(BaseColor::Red);
    palette[PaletteColor::HighlightText] = Color::Light(BaseColor::White);

    return Theme {
        shadow: false,
        borders: BorderStyle::None,
        palette,
    };
}
