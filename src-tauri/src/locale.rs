//! The UI language: the user's choice, else the system's (same rule as `pickLanguage` in src/lib/i18n.ts).
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Fr,
}

/// The language setting: `System` follows the OS.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguagePref {
    #[default]
    System,
    En,
    Fr,
}

/// The first supported language among the OS preferred ones (BCP 47 tags such as `fr-FR`, `en_US`), else English.
pub fn system_language(tags: &[String]) -> Language {
    tags.iter()
        .find_map(|tag| match tag.split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase().as_str() {
            "fr" => Some(Language::Fr),
            "en" => Some(Language::En),
            _ => None,
        })
        .unwrap_or(Language::En)
}

pub fn resolve(pref: LanguagePref, system_tags: &[String]) -> Language {
    match pref {
        LanguagePref::System => system_language(system_tags),
        LanguagePref::En => Language::En,
        LanguagePref::Fr => Language::Fr,
    }
}

/// The tray menu entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayLabels {
    pub open: &'static str,
    pub pause: &'static str,
    pub quit: &'static str,
}

pub fn tray_labels(language: Language) -> TrayLabels {
    match language {
        Language::En => TrayLabels { open: "Open Scribe", pause: "Pause", quit: "Quit" },
        Language::Fr => TrayLabels { open: "Ouvrir Scribe", pause: "Mettre en pause", quit: "Quitter" },
    }
}

/// The macOS menu bar entries (the app menu, Edit and Window).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppMenuLabels {
    pub about: &'static str,
    pub hide: &'static str,
    pub hide_others: &'static str,
    pub show_all: &'static str,
    pub quit: &'static str,
    pub edit: &'static str,
    pub undo: &'static str,
    pub redo: &'static str,
    pub cut: &'static str,
    pub copy: &'static str,
    pub paste: &'static str,
    pub select_all: &'static str,
    pub window: &'static str,
    pub minimize: &'static str,
    pub zoom: &'static str,
    pub close: &'static str,
}

/// Wording of the standard macOS menus in each language.
pub fn app_menu_labels(language: Language) -> AppMenuLabels {
    match language {
        Language::En => AppMenuLabels {
            about: "About Scribe",
            hide: "Hide Scribe",
            hide_others: "Hide Others",
            show_all: "Show All",
            quit: "Quit Scribe",
            edit: "Edit",
            undo: "Undo",
            redo: "Redo",
            cut: "Cut",
            copy: "Copy",
            paste: "Paste",
            select_all: "Select All",
            window: "Window",
            minimize: "Minimize",
            zoom: "Zoom",
            close: "Close Window",
        },
        Language::Fr => AppMenuLabels {
            about: "À propos de Scribe",
            hide: "Masquer Scribe",
            hide_others: "Masquer les autres",
            show_all: "Tout afficher",
            quit: "Quitter Scribe",
            edit: "Édition",
            undo: "Annuler",
            redo: "Rétablir",
            cut: "Couper",
            copy: "Copier",
            paste: "Coller",
            select_all: "Tout sélectionner",
            window: "Fenêtre",
            minimize: "Placer dans le Dock",
            zoom: "Réduire/agrandir",
            close: "Fermer la fenêtre",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn the_system_language_is_the_first_supported_one() {
        assert_eq!(system_language(&tags(&["fr-FR", "en-US"])), Language::Fr);
        assert_eq!(system_language(&tags(&["de-DE", "en_GB", "fr"])), Language::En, "first supported wins");
        assert_eq!(system_language(&tags(&["de", "FR_ca"])), Language::Fr, "case and separator");
        assert_eq!(system_language(&tags(&["fry-NL", "french"])), Language::En, "the base must match exactly");
        assert_eq!(system_language(&tags(&["de-DE"])), Language::En, "unsupported: English");
        assert_eq!(system_language(&[]), Language::En);
    }

    #[test]
    fn a_chosen_language_wins_over_the_system() {
        let fr = tags(&["fr-FR"]);
        assert_eq!(resolve(LanguagePref::System, &fr), Language::Fr);
        assert_eq!(resolve(LanguagePref::En, &fr), Language::En);
        assert_eq!(resolve(LanguagePref::Fr, &tags(&["en-US"])), Language::Fr);
    }

    #[test]
    fn languages_serialize_as_lowercase_codes() {
        assert_eq!(serde_json::to_value(Language::Fr).unwrap(), serde_json::json!("fr"));
        assert_eq!(serde_json::to_value(LanguagePref::System).unwrap(), serde_json::json!("system"));
        assert_eq!(serde_json::from_str::<LanguagePref>("\"en\"").unwrap(), LanguagePref::En);
    }

    #[test]
    fn the_tray_speaks_the_ui_language() {
        assert_eq!(tray_labels(Language::En), TrayLabels { open: "Open Scribe", pause: "Pause", quit: "Quit" });
        assert_eq!(
            tray_labels(Language::Fr),
            TrayLabels { open: "Ouvrir Scribe", pause: "Mettre en pause", quit: "Quitter" }
        );
    }

    #[test]
    fn the_macos_menus_are_translated_entry_by_entry() {
        let (en, fr) = (app_menu_labels(Language::En), app_menu_labels(Language::Fr));
        assert_eq!((en.edit, fr.edit), ("Edit", "Édition"));
        assert_eq!((en.minimize, fr.minimize), ("Minimize", "Placer dans le Dock"));
        let all = |l: AppMenuLabels| {
            [
                l.about,
                l.hide,
                l.hide_others,
                l.show_all,
                l.quit,
                l.edit,
                l.undo,
                l.redo,
                l.cut,
                l.copy,
                l.paste,
                l.select_all,
                l.window,
                l.minimize,
                l.zoom,
                l.close,
            ]
        };
        for (e, f) in all(en).into_iter().zip(all(fr)) {
            assert_ne!(e, f, "untranslated entry");
        }
    }
}
