use cursive::theme::*;
use cursive::views::{Dialog, LinearLayout, ThemedView};
use cursive::Cursive;

pub fn theme1() -> Theme {
    let mut palette = Palette::default();
    palette[PaletteColor::Background] = Color::Dark(BaseColor::Black);
    palette[PaletteColor::View] = Color::Dark(BaseColor::Black);
    palette[PaletteColor::Primary] = Color::Light(BaseColor::White);
    palette[PaletteColor::TitlePrimary] = Color::Light(BaseColor::Yellow);
    palette[PaletteColor::Highlight] = Color::Dark(BaseColor::Red);
    palette[PaletteColor::HighlightText] = Color::Dark(BaseColor::Black);

    Theme {
        shadow: false,
        borders: BorderStyle::None,
        palette,
    }
}

pub fn set_highlight_disabled(siv: &mut Cursive) {
    siv.with_theme(|theme| {
        theme.palette[PaletteColor::Highlight] = Color::Rgb(50, 50, 50);
        theme.palette[PaletteColor::HighlightText] = Color::Dark(BaseColor::White);
    });
}

// pub fn set_highlight_enabled(siv: &mut Cursive) {
//     siv.with_theme(|theme| {
//         let t = theme1();
//         theme.palette[PaletteColor::Highlight] = t.palette[PaletteColor::Highlight];
//         theme.palette[PaletteColor::HighlightText] = t.palette[PaletteColor::HighlightText];
//     });
// }

pub fn get_borderless_layout(
    siv: &mut Cursive,
    layout: LinearLayout,
    title: Option<String>,
) -> ThemedView<Dialog> {
    let mut custom_theme = siv.current_theme().clone();
    custom_theme.borders = BorderStyle::None;

    // Wrap the layout in a dialog and remove padding around it
    let dialog;
    if let Some(title) = title {
        dialog = Dialog::around(layout).title(title).padding_lrtb(0, 0, 0, 0);
    } else {
        dialog = Dialog::around(layout).padding_lrtb(0, 0, 0, 0);
    }

    // Apply the borderless theme
    ThemedView::new(custom_theme, dialog)
}

pub fn get_borderless_dialog(
    siv: &mut Cursive,
    content: &str,
    title: Option<String>,
) -> ThemedView<Dialog> {
    let mut custom_theme = siv.current_theme().clone();
    custom_theme.borders = BorderStyle::None; // Turn off border style

    // Create a dialog with no padding around it
    let dialog;
    if let Some(title) = title {
        dialog = Dialog::text(content).title(title).padding_lrtb(0, 0, 0, 0);
    } else {
        dialog = Dialog::text(content).padding_lrtb(0, 0, 0, 0);
    }

    // Wrap in a ThemedView to apply the custom theme
    ThemedView::new(custom_theme, dialog)
}
