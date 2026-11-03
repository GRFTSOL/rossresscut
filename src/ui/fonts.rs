//! Font discovery and the font selector widgets.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;

use super::{RossresscutApp, Strings};

impl RossresscutApp {
    fn selected_font_family(&self) -> Option<&FontFamilyChoice> {
        if self.settings.font_path.is_empty() {
            return None;
        }
        self.fonts.iter().find(|family| {
            family.faces.iter().any(|face| {
                face.index == self.settings.font_index
                    && face.path.as_os_str() == self.settings.font_path.as_str()
            })
        })
    }

    fn selected_font_face(&self) -> Option<FontFaceChoice> {
        let family = self.selected_font_family()?;
        family
            .faces
            .iter()
            .find(|face| {
                face.index == self.settings.font_index
                    && face.path.as_os_str() == self.settings.font_path.as_str()
            })
            .cloned()
    }

    pub(super) fn font_family_ui(&mut self, ui: &mut egui::Ui, strings: &Strings) {
        let selected_family = self
            .selected_font_family()
            .map(|family| family.name.clone());
        let family_label = selected_family
            .clone()
            .unwrap_or_else(|| strings.default_font.clone());

        ui.horizontal(|ui| {
            ui.label(strings.font_family.as_str());
            egui::ComboBox::from_id_salt("font_family")
                .width(240.0)
                .selected_text(family_label)
                // The default `CloseOnClick` would close the popup as soon as
                // the search field is clicked.
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            self.settings.font_path.is_empty(),
                            strings.default_font.as_str(),
                        )
                        .clicked()
                    {
                        self.settings.font_path.clear();
                        self.settings.font_index = 0;
                        ui.close();
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.font_search)
                            .hint_text(strings.font_search.as_str())
                            .desired_width(240.0),
                    );
                    let query = self.font_search.trim().to_lowercase();
                    egui::ScrollArea::vertical()
                        .max_height(240.0)
                        .show(ui, |ui| {
                            let fonts = &self.fonts;
                            for family in fonts {
                                if !query.is_empty() && !family.name.to_lowercase().contains(&query)
                                {
                                    continue;
                                }
                                let selected =
                                    selected_family.as_deref() == Some(family.name.as_str());
                                if ui
                                    .selectable_label(selected, family.name.as_str())
                                    .clicked()
                                {
                                    if let Some(face) = family.default_face() {
                                        self.settings.font_path =
                                            face.path.to_string_lossy().into_owned();
                                        self.settings.font_index = face.index;
                                    }
                                    ui.close();
                                }
                            }
                        });
                });

            if let Some(face) = self.selected_font_face() {
                let faces = self
                    .fonts
                    .iter()
                    .find(|family| selected_family.as_deref() == Some(family.name.as_str()))
                    .map(|family| family.faces.clone())
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("font_weight")
                    .selected_text(face_label(&face))
                    .show_ui(ui, |ui| {
                        for option in &faces {
                            let selected = option.index == self.settings.font_index
                                && option.path.as_os_str() == self.settings.font_path.as_str();
                            if ui.selectable_label(selected, face_label(option)).clicked() {
                                self.settings.font_path =
                                    option.path.to_string_lossy().into_owned();
                                self.settings.font_index = option.index;
                                ui.close();
                            }
                        }
                    });
            }
        });
    }
}

/// One font face (file plus face index) of a family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FontFaceChoice {
    weight: u16,
    style: FontFaceStyle,
    path: PathBuf,
    index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum FontFaceStyle {
    Normal,
    Italic,
    Oblique,
}

/// One system font family with all discovered faces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FontFamilyChoice {
    name: String,
    faces: Vec<FontFaceChoice>,
}

impl FontFamilyChoice {
    fn default_face(&self) -> Option<&FontFaceChoice> {
        self.faces
            .iter()
            .find(|face| face.weight == 400 && face.style == FontFaceStyle::Normal)
            .or_else(|| self.faces.first())
    }
}

pub(super) fn scan_fonts() -> Vec<FontFamilyChoice> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();

    let mut families: BTreeMap<String, Vec<FontFaceChoice>> = BTreeMap::new();
    for face in database.faces() {
        let Some((name, _)) = face.families.first() else {
            continue;
        };
        let fontdb::Source::File(path) = &face.source else {
            continue;
        };
        let style = match face.style {
            fontdb::Style::Normal => FontFaceStyle::Normal,
            fontdb::Style::Italic => FontFaceStyle::Italic,
            fontdb::Style::Oblique => FontFaceStyle::Oblique,
        };
        families
            .entry(name.clone())
            .or_default()
            .push(FontFaceChoice {
                weight: face.weight.0,
                style,
                path: path.clone(),
                index: face.index,
            });
    }

    families
        .into_iter()
        .filter_map(|(name, mut faces)| {
            faces.sort_by_key(|face| (face.weight, face.style));
            faces.dedup_by_key(|face| (face.weight, face.style));
            (!faces.is_empty()).then_some(FontFamilyChoice { name, faces })
        })
        .collect()
}

pub(super) fn font_definitions(path: &Path, index: u32) -> Option<egui::FontDefinitions> {
    let bytes = fs::read(path).ok()?;
    let mut data = egui::FontData::from_owned(bytes);
    data.index = index;
    let mut definitions = egui::FontDefinitions::default();
    definitions
        .font_data
        .insert("user_font".to_owned(), Arc::new(data));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .insert(0, "user_font".to_owned());
    }
    Some(definitions)
}

/// Nerd Fonts Propo, used as a fallback for icon glyphs.
const ICON_FONT: &[u8] =
    include_bytes!("../../assets/fonts/JetBrainsMonoNerdFontPropo-Regular.ttf");

/// Appends the icon font to both families so missing glyphs fall back to it.
pub(super) fn install_icon_font(definitions: &mut egui::FontDefinitions) {
    const FAMILY: &str = "nerd_icons";
    definitions.font_data.insert(
        FAMILY.to_owned(),
        Arc::new(egui::FontData::from_static(ICON_FONT)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(FAMILY.to_owned());
    }
}

pub(super) fn face_label(face: &FontFaceChoice) -> String {
    let weight = match face.weight {
        100 => "Thin".to_owned(),
        200 => "ExtraLight".to_owned(),
        300 => "Light".to_owned(),
        400 => "Regular".to_owned(),
        500 => "Medium".to_owned(),
        600 => "SemiBold".to_owned(),
        700 => "Bold".to_owned(),
        800 => "ExtraBold".to_owned(),
        900 => "Black".to_owned(),
        other => other.to_string(),
    };
    match face.style {
        FontFaceStyle::Normal => weight,
        FontFaceStyle::Italic => format!("{weight} Italic"),
        FontFaceStyle::Oblique => format!("{weight} Oblique"),
    }
}
