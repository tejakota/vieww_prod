//! The studio chrome in more than one language (ST-16).
//!
//! Every command, view and menu title is written once, in English, in
//! `command.rs` and `state.rs`; those strings are the catalog keys. A
//! language is a table from those keys to its own text. [`t`] looks a
//! title up in the current language, and an English string with no entry
//! falls back to itself. A test (`every_title_has_an_entry_in_every_catalog`)
//! makes sure no title is missing from any table, so that a fallback never
//! ships silently.
//!
//! The language is the `language` setting (`en`, `es`). `VIEWWSTUDIO_LANG`
//! overrides it for one run, which is how the screenshot test takes the
//! second-language capture.

use std::cell::Cell;

/// A language the studio's chrome is translated into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// English (the source strings).
    English,
    /// Spanish.
    Spanish,
}

impl Language {
    /// Every language, in the settings list's order.
    pub const ALL: [Self; 2] = [Self::English, Self::Spanish];

    /// The setting's value: `en`, `es`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
        }
    }

    /// Its own name, for the settings list.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Spanish => "Español",
        }
    }

    /// The language a setting names (`es`, `es-MX`, `ES`); English for
    /// anything unknown.
    #[must_use]
    pub fn from_code(code: &str) -> Self {
        let primary = code
            .split(['-', '_', '.'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        match primary.as_str() {
            "es" => Self::Spanish,
            _ => Self::English,
        }
    }

    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::English => &[],
            Self::Spanish => SPANISH,
        }
    }
}

thread_local! {
    static CURRENT: Cell<Language> = const { Cell::new(Language::English) };
}

/// The language the chrome is drawn in.
#[must_use]
pub fn language() -> Language {
    CURRENT.with(Cell::get)
}

/// Draw the chrome in `language` from now on.
pub fn set_language(language: Language) {
    CURRENT.with(|c| c.set(language));
}

/// The language a setting asks for, unless `VIEWWSTUDIO_LANG` overrides it.
#[must_use]
pub fn effective(setting: &str) -> Language {
    std::env::var("VIEWWSTUDIO_LANG").map_or_else(
        |_| Language::from_code(setting),
        |v| Language::from_code(&v),
    )
}

/// `english` in the current language.
#[must_use]
pub fn t(english: &'static str) -> &'static str {
    translate(language(), english)
}

/// `english` in `language`.
#[must_use]
pub fn translate(language: Language, english: &'static str) -> &'static str {
    language
        .table()
        .iter()
        .find(|(k, _)| *k == english)
        .map_or(english, |(_, v)| v)
}

/// Whether `language` has its own entry for `english` (English always does).
#[must_use]
pub fn has_entry(language: Language, english: &str) -> bool {
    language == Language::English || language.table().iter().any(|(k, _)| *k == english)
}

const SPANISH: &[(&str, &str)] = &[
    // Menus
    ("File", "Archivo"),
    ("Edit", "Editar"),
    ("Go", "Ir"),
    ("View", "Ver"),
    ("Render", "Renderizar"),
    ("Build", "Compilar"),
    ("Help", "Ayuda"),
    // Views
    ("Explorer", "Explorador"),
    ("Search", "Buscar"),
    ("Snippets", "Fragmentos"),
    ("Problems", "Problemas"),
    ("Inspector", "Inspector"),
    ("Learn", "Aprender"),
    ("Docs", "Documentación"),
    ("Toolchain", "Herramientas"),
    ("Source Control", "Control de código fuente"),
    ("Export", "Exportar"),
    ("Theme Tokens", "Tokens del tema"),
    ("Settings", "Ajustes"),
    // Commands
    ("Cancel Render", "Cancelar renderizado"),
    ("Preview as iOS", "Vista previa como iOS"),
    ("Preview as Android", "Vista previa como Android"),
    ("Preview as Desktop", "Vista previa como escritorio"),
    (
        "Toggle Safe-Area Overlay",
        "Mostrar u ocultar el área segura",
    ),
    ("Toggle Preview Dark Mode", "Modo oscuro en la vista previa"),
    (
        "Live Preview (No Build)",
        "Vista previa en vivo (sin compilar)",
    ),
    ("Load Flow File", "Cargar archivo de flujo"),
    (
        "Try a Say Screen Right Now",
        "Probar una pantalla Say ahora mismo",
    ),
    ("New Project\u{2026}", "Nuevo proyecto\u{2026}"),
    ("Open Folder…", "Abrir carpeta…"),
    ("Restore Sample Buffer", "Restaurar el búfer de ejemplo"),
    ("New File", "Nuevo archivo"),
    ("Collapse All Folders", "Contraer todas las carpetas"),
    ("Push", "Enviar (push)"),
    ("Pull", "Traer (pull)"),
    ("Fetch", "Obtener (fetch)"),
    ("New Screen from Template", "Nueva pantalla desde plantilla"),
    ("New Widget from Template", "Nuevo widget desde plantilla"),
    ("Save", "Guardar"),
    ("Save All", "Guardar todo"),
    ("Revert File", "Revertir archivo"),
    ("Close Tab", "Cerrar pestaña"),
    ("Undo", "Deshacer"),
    ("Redo", "Rehacer"),
    ("Cut", "Cortar"),
    ("Copy", "Copiar"),
    ("Paste", "Pegar"),
    ("Toggle Preview Pane", "Mostrar u ocultar la vista previa"),
    ("Zen Mode", "Modo zen"),
    ("Widget Inspector", "Inspector de widgets"),
    ("Preview: Zoom In", "Vista previa: acercar"),
    ("Preview: Zoom Out", "Vista previa: alejar"),
    ("Preview: Fit to Pane", "Vista previa: ajustar al panel"),
    ("Show Repainted Regions", "Mostrar regiones repintadas"),
    ("Show Semantics Tree", "Mostrar árbol semántico"),
    (
        "Pick a Widget to Inspect",
        "Elegir un widget para inspeccionar",
    ),
    ("Toggle Comment", "Comentar o descomentar"),
    ("Indent", "Aumentar sangría"),
    ("Outdent", "Reducir sangría"),
    ("Find", "Buscar"),
    ("Find and Replace", "Buscar y reemplazar"),
    ("Find Next", "Buscar siguiente"),
    ("Find Previous", "Buscar anterior"),
    ("Close Find", "Cerrar búsqueda"),
    ("Command Palette", "Paleta de comandos"),
    ("Go to File", "Ir al archivo"),
    ("Go to Symbol in File", "Ir al símbolo del archivo"),
    ("Go to Line", "Ir a la línea"),
    ("Toggle Panel", "Mostrar u ocultar el panel"),
    ("Toggle Sidebar", "Mostrar u ocultar la barra lateral"),
    (
        "Toggle Activity Bar",
        "Mostrar u ocultar la barra de actividad",
    ),
    ("Toggle Dark Theme", "Tema oscuro"),
    ("Next Tab", "Pestaña siguiente"),
    ("Previous Tab", "Pestaña anterior"),
    ("Show Problems", "Mostrar problemas"),
    ("Show Explorer", "Mostrar explorador"),
    ("Show Search", "Mostrar búsqueda"),
    ("Show Toolchain", "Mostrar herramientas"),
    ("Show Settings", "Mostrar ajustes"),
    ("Build (Release)", "Compilar (release)"),
    ("Build and Run", "Compilar y ejecutar"),
    ("Cancel Build", "Cancelar compilación"),
    ("Clear Output", "Borrar salida"),
    ("Export\u{2026}", "Exportar\u{2026}"),
    (
        "Export (run the selected format)",
        "Exportar (con el formato elegido)",
    ),
    ("Cancel Export", "Cancelar exportación"),
    ("Scan for Devices", "Buscar dispositivos"),
    ("Add Cursor Below", "Añadir cursor debajo"),
    ("Add Cursor Above", "Añadir cursor encima"),
    (
        "Add Cursor at Next Occurrence",
        "Añadir cursor en la siguiente coincidencia",
    ),
    ("Clear Extra Cursors", "Quitar cursores adicionales"),
    ("Fold / Unfold at Cursor", "Plegar / desplegar en el cursor"),
    ("Fold All Regions", "Plegar todas las regiones"),
    ("Unfold All Regions", "Desplegar todas las regiones"),
    ("Format Document (rustfmt)", "Formatear documento (rustfmt)"),
    ("Toggle Format on Save", "Formatear al guardar"),
    ("Show Tasks", "Mostrar tareas"),
    (
        "Refresh Source Control",
        "Actualizar control de código fuente",
    ),
    ("Commit Staged Changes", "Confirmar cambios preparados"),
    ("Clear Finished Tasks", "Borrar tareas terminadas"),
    ("Cancel All Tasks", "Cancelar todas las tareas"),
    ("Toggle Indent Guides", "Guías de sangría"),
    (
        "Toggle Inline Diagnostic Messages",
        "Mensajes de diagnóstico en línea",
    ),
    ("Run a Command\u{2026}", "Ejecutar un comando\u{2026}"),
    ("Complete at Caret", "Completar en el cursor"),
    ("What Is This?", "¿Qué es esto?"),
    ("Go to Definition", "Ir a la definición"),
    ("Find References", "Buscar referencias"),
    ("Replace in Files\u{2026}", "Reemplazar en archivos\u{2026}"),
    (
        "Write Customisation Templates",
        "Escribir plantillas de personalización",
    ),
    ("Close Other Tabs", "Cerrar las demás pestañas"),
    ("Close Tabs to the Right", "Cerrar pestañas a la derecha"),
    ("Close Saved Tabs", "Cerrar pestañas guardadas"),
    (
        "rust-analyzer: Start / Stop",
        "rust-analyzer: iniciar / detener",
    ),
    ("rust-analyzer: Restart", "rust-analyzer: reiniciar"),
    ("Toggle Word Wrap", "Ajuste de línea"),
    ("About vieww Studio", "Acerca de vieww Studio"),
    (
        "Copy Diagnostics for a Bug Report",
        "Copiar diagnóstico para un informe de error",
    ),
    (
        "Interface Language: Next",
        "Idioma de la interfaz: siguiente",
    ),
    ("Undo File Operation", "Deshacer operación de archivo"),
    ("Redo File Operation", "Rehacer operación de archivo"),
    ("Show Welcome", "Mostrar bienvenida"),
    ("Auto Render on Change", "Renderizar al cambiar"),
    ("Keyboard Shortcuts", "Atajos de teclado"),
    (
        "Show or Hide the Preview Note",
        "Mostrar u ocultar la nota de la vista previa",
    ),
    (
        "Create live.rs for the Live Preview",
        "Crear live.rs para la vista previa en vivo",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Menu};
    use crate::state::View;

    /// No title ships in English by accident: every command, view and menu
    /// title has an entry in every catalog.
    #[test]
    fn every_title_has_an_entry_in_every_catalog() {
        let mut titles: Vec<&'static str> = Command::ALL.iter().map(|c| c.title()).collect();
        titles.extend(View::ALL.iter().map(|v| v.title()));
        titles.extend(
            Menu::BAR
                .iter()
                .map(|m| m.title())
                .filter(|t| !t.is_empty()),
        );
        for lang in Language::ALL {
            let missing: Vec<&&str> = titles.iter().filter(|t| !has_entry(lang, t)).collect();
            assert!(
                missing.is_empty(),
                "{} has no entry for {missing:?}",
                lang.name()
            );
        }
    }

    #[test]
    fn languages_resolve_and_fall_back() {
        assert_eq!(Language::from_code("es-MX"), Language::Spanish);
        assert_eq!(Language::from_code("fr"), Language::English);
        assert_eq!(translate(Language::Spanish, "Save"), "Guardar");
        assert_eq!(translate(Language::English, "Save"), "Save");
        set_language(Language::Spanish);
        assert_eq!(t("Undo"), "Deshacer");
        set_language(Language::English);
        assert_eq!(t("Undo"), "Undo");
    }

    #[test]
    fn the_catalog_has_no_stale_keys() {
        let mut titles: Vec<&'static str> = Command::ALL.iter().map(|c| c.title()).collect();
        titles.extend(View::ALL.iter().map(|v| v.title()));
        titles.extend(Menu::BAR.iter().map(|m| m.title()));
        let stale: Vec<&str> = SPANISH
            .iter()
            .map(|(k, _)| *k)
            .filter(|k| !titles.contains(k))
            .collect();
        assert!(stale.is_empty(), "catalog keys no title uses: {stale:?}");
    }
}
