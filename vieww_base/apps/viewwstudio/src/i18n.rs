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
//! The language is the `language` setting (`en`, `es`, `fr`, `de`). `VIEWWSTUDIO_LANG`
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
    /// French.
    French,
    /// German.
    German,
}

impl Language {
    /// Every language, in the settings list's order.
    pub const ALL: [Self; 4] = [Self::English, Self::Spanish, Self::French, Self::German];

    /// The setting's value: `en`, `es`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
            Self::French => "fr",
            Self::German => "de",
        }
    }

    /// Its own name, for the settings list.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Spanish => "Español",
            Self::French => "Français",
            Self::German => "Deutsch",
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
            "fr" => Self::French,
            "de" => Self::German,
            _ => Self::English,
        }
    }

    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::English => &[],
            Self::Spanish => SPANISH,
            Self::French => FRENCH,
            Self::German => GERMAN,
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

/// `template` in the current language with each `{}` replaced, in order,
/// by the next of `args`: for prose with values in it ("Ln {}, Col {}").
/// The template is the catalog key, so translations can move the values
/// (German puts a count where English does not).
#[must_use]
pub fn tf(template: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut args = args.iter();
    let mut rest = t(template);
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        if let Some(a) = args.next() {
            out.push_str(&a.to_string());
        }
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

/// A status-bar message in the current language (ST-16), whether it was
/// written as a literal or built with `format!`.
///
/// An exact catalog key is looked up. Otherwise the message is matched
/// against the catalog's templates (keys with `{}` in them, which is how
/// the source scan records a `format!` message): each `{}` captures the
/// text between the template's fixed pieces, and a captured value that is
/// itself a catalog key ("Landscape") is translated too. A message nothing
/// matches is returned as it came — names, paths and error text are the
/// user's or the system's, not the studio's.
#[must_use]
pub fn translate_message(message: &str) -> String {
    let lang = language();
    if lang == Language::English {
        return message.to_owned();
    }
    if let Some((_, v)) = lang.table().iter().find(|(k, _)| *k == message) {
        return (*v).to_owned();
    }
    // The most specific template wins: "Opened {}." also matches what
    // "Opened {}. Its Cargo.toml does not mention vieww…" produced.
    let best = lang
        .table()
        .iter()
        .filter(|(k, _)| k.contains("{}"))
        .filter_map(|(k, v)| {
            match_template(k, message).map(|args| (k.len() - 2 * k.matches("{}").count(), v, args))
        })
        .max_by_key(|(fixed, _, _)| *fixed);
    if let Some((_, v, args)) = best {
        {
            let mut out = String::new();
            let mut rest: &str = v;
            let mut args = args.into_iter();
            while let Some(at) = rest.find("{}") {
                out.push_str(&rest[..at]);
                if let Some(a) = args.next() {
                    let translated = lang
                        .table()
                        .iter()
                        .find(|(k, _)| *k == a)
                        .map_or(a, |(_, v)| v);
                    out.push_str(translated);
                }
                rest = &rest[at + 2..];
            }
            out.push_str(rest);
            return out;
        }
    }
    message.to_owned()
}

/// The values a message fills a template's `{}`s with, or `None` when it
/// is not that template. Fixed pieces are matched left to right; a value
/// is at least one character.
fn match_template<'m>(template: &str, message: &'m str) -> Option<Vec<&'m str>> {
    let pieces: Vec<&str> = template.split("{}").collect();
    let mut rest = message.strip_prefix(pieces[0])?;
    let mut args = Vec::new();
    for (i, piece) in pieces.iter().enumerate().skip(1) {
        let last = i + 1 == pieces.len();
        let at = if last {
            if piece.is_empty() {
                rest.len()
            } else {
                rest.strip_suffix(piece).map(str::len)?
            }
        } else {
            rest.get(1..)?.find(piece).map(|p| p + 1)?
        };
        if at == 0 {
            return None;
        }
        args.push(&rest[..at]);
        rest = &rest[at + piece.len()..];
    }
    rest.is_empty().then_some(args)
}

/// Whether `language` has its own entry for `english` (English always does).
#[must_use]
pub fn has_entry(language: Language, english: &str) -> bool {
    language == Language::English || language.table().iter().any(|(k, _)| *k == english)
}

const SPANISH: &[(&str, &str)] = &[
    ("Attributing a frame needs a rendered screen — press Render first.", "Atribuir un fotograma necesita una pantalla renderizada: pulsa Renderizar primero."),
    ("Frame attributed: {}", "Fotograma atribuido: {}"),
    ("Performance overlay off.", "Superposición de rendimiento desactivada."),
    ("Performance overlay on — one bar per frame, red past the budget.", "Superposición de rendimiento activada: una barra por fotograma, en rojo si supera el presupuesto."),
    ("Preview: Attribute This Frame", "Vista previa: atribuir este fotograma"),
    ("Preview: Performance Overlay", "Vista previa: superposición de rendimiento"),
    ("This frame: {} ms — text {}, shadows {}, the rest {}, floor {}", "Este fotograma: {} ms — texto {}, sombras {}, el resto {}, base {}"),
    (", skipped {}", ", omitidos {}"),
    (", {} could not be written", ", {} no se pudieron escribir"),
    ("A scratch workspace has no trash to move it to", "Un espacio de trabajo provisional no tiene papelera"),
    ("A template goes into a project — open a folder first.", "Una plantilla va dentro de un proyecto: abre primero una carpeta."),
    ("Activity bar hidden — Ctrl+Alt+U brings it back.", "Barra de actividad oculta: Ctrl+Alt+U la recupera."),
    ("Activity bar shown.", "Barra de actividad visible."),
    ("Android phone", "Teléfono Android"),
    ("Android tablet", "Tableta Android"),
    ("Auto Render off.", "Renderizado automático desactivado."),
    ("Auto Render on — the buffer renders 0.6s after you stop typing.", "Renderizado automático activado: el búfer se renderiza 0,6 s después de dejar de escribir."),
    ("Built. The Export view can install it on a connected device.", "Compilado. La vista Exportar puede instalarlo en un dispositivo conectado."),
    ("Click anything in the window to select it in the Inspector.", "Haz clic en cualquier cosa de la ventana para seleccionarla en el Inspector."),
    ("Closed {} tab", "Se cerró {} pestaña"),
    ("Closed {} tabs", "Se cerraron {} pestañas"),
    ("Closed {}, kept {} with unsaved changes", "Se cerraron {}; se conservan {} con cambios sin guardar"),
    ("Command copied.", "Comando copiado."),
    ("Completion needs a file on disk — save this buffer first", "Completar necesita un archivo en disco: guarda primero este búfer"),
    ("Copied", "Copiado"),
    ("Copied.", "Copiado."),
    ("Could not create the settings directory", "No se pudo crear la carpeta de ajustes"),
    ("Could not create {}: {}", "No se pudo crear {}: {}"),
    ("Could not write live.rs: {}", "No se pudo escribir live.rs: {}"),
    ("Could not write {}: {}", "No se pudo escribir {}: {}"),
    ("Created {}. Rename it before you build on it.", "Se creó {}. Cámbiale el nombre antes de construir sobre él."),
    ("Cut.", "Cortado."),
    ("Desktop", "Escritorio"),
    ("Dropped {} folders — drop one at a time to choose a workspace", "Soltaste {} carpetas: suelta una a la vez para elegir un espacio de trabajo"),
    ("Every one of those has unsaved changes", "Todos tienen cambios sin guardar"),
    ("Exporting tokens writes a file into a project — open a folder first.", "Exportar los tokens escribe un archivo en un proyecto: abre primero una carpeta."),
    ("Flow file loaded. Click Render to start the Live Preview.", "Archivo de flujo cargado. Pulsa Renderizar para iniciar la vista previa en vivo."),
    ("Hover needs a file on disk — save this buffer first", "La información al pasar el cursor necesita un archivo en disco: guarda primero este búfer"),
    ("Inserted “{}”.", "Se insertó «{}»."),
    ("Installing {}. Watch the Output panel.", "Instalando {}. Mira el panel Salida."),
    ("Interface language: {}", "Idioma de la interfaz: {}"),
    ("Keeping your version — saving will overwrite the change on disk", "Se conserva tu versión: al guardar se sobrescribirá el cambio en disco"),
    ("Landscape", "Horizontal"),
    ("Laptop", "Portátil"),
    ("Live preview: drawing live.rs. Render compiles a real screen.", "Vista previa en vivo: se dibuja live.rs. Renderizar compila una pantalla real."),
    ("Loaded “{}”{}. Press Render.", "Se cargó «{}»{}. Pulsa Renderizar."),
    ("Moved {} to the workspace trash", "Se movió {} a la papelera del espacio de trabajo"),
    ("No matches in this workspace", "No hay coincidencias en este espacio de trabajo"),
    ("No references to that", "No hay referencias a eso"),
    ("No terminal application found. Run this yourself: {}", "No se encontró ninguna terminal. Ejecuta esto tú mismo: {}"),
    ("No workspace is open", "No hay ningún espacio de trabajo abierto"),
    ("Not valid UTF-8 — shown as text, and saving is refused", "No es UTF-8 válido: se muestra como texto y no se puede guardar"),
    ("Nothing is selected.", "No hay nada seleccionado."),
    ("Nothing to close", "No hay nada que cerrar"),
    ("Open a folder first — live.rs goes at the workspace root.", "Abre primero una carpeta: live.rs va en la raíz del espacio de trabajo."),
    ("Opened a terminal to install {}.", "Se abrió una terminal para instalar {}."),
    ("Opened {} files", "Se abrieron {} archivos"),
    ("Opened {}.", "Se abrió {}."),
    ("Opened {}. Its Cargo.toml does not mention vieww, so Render will refuse.", "Se abrió {}. Su Cargo.toml no menciona vieww, así que Renderizar se negará."),
    ("Panes restored.", "Paneles restaurados."),
    ("Path copied", "Ruta copiada"),
    ("Phone", "Teléfono"),
    ("Phone (large)", "Teléfono (grande)"),
    ("Phone (small)", "Teléfono (pequeño)"),
    ("Portrait", "Vertical"),
    ("Preview at {}%.", "Vista previa al {} %."),
    ("Preview fitted to the pane.", "Vista previa ajustada al panel."),
    ("Preview framed at {}", "Vista previa enmarcada en {}"),
    ("Preview note hidden. Bring it back from this palette.", "Nota de la vista previa oculta. Recupérala desde esta paleta."),
    ("Preview: Add a Device to the Row", "Vista previa: añadir un dispositivo a la fila"),
    ("Preview: Several Devices Side by Side", "Vista previa: varios dispositivos lado a lado"),
    ("Preview: dark", "Vista previa: oscura"),
    ("Preview: light", "Vista previa: clara"),
    ("Preview: one device", "Vista previa: un dispositivo"),
    ("Preview: several devices", "Vista previa: varios dispositivos"),
    ("Read-only — this file cannot be saved from here", "Solo lectura: este archivo no se puede guardar desde aquí"),
    ("Recovered unsaved work: {}", "Trabajo sin guardar recuperado: {}"),
    ("Reloaded {} file from disk", "Se recargó {} archivo del disco"),
    ("Reloaded {} files from disk", "Se recargaron {} archivos del disco"),
    ("Repainted regions hidden.", "Regiones repintadas ocultas."),
    ("Repainted regions shown. Every box is a rectangle the compositor was asked for.", "Regiones repintadas visibles. Cada caja es un rectángulo que se pidió al compositor."),
    ("Replace across files needs a folder open", "Reemplazar en varios archivos necesita una carpeta abierta"),
    ("Replaced {} match in {} file", "Se reemplazó {} coincidencia en {} archivo"),
    ("Replaced {} matches in {} file", "Se reemplazaron {} coincidencias en {} archivo"),
    ("Replaced {} matches in {} files", "Se reemplazaron {} coincidencias en {} archivos"),
    ("Running a command needs a folder open", "Ejecutar un comando necesita una carpeta abierta"),
    ("Sample buffer restored. Press Render.", "Búfer de ejemplo restaurado. Pulsa Renderizar."),
    ("Semantics tree hidden.", "Árbol semántico oculto."),
    ("Semantics tree shown — role and label, as a screen reader gets them.", "Árbol semántico visible: rol y etiqueta, tal como los recibe un lector de pantalla."),
    ("Tablet", "Tableta"),
    ("That is outside the captured tree — open the Inspector and click again.", "Eso está fuera del árbol capturado: abre el Inspector y vuelve a hacer clic."),
    ("The pasteboard would not accept it.", "El portapapeles no lo aceptó."),
    ("The preview note is back under the device frame.", "La nota de la vista previa vuelve a estar bajo el marco del dispositivo."),
    ("There is no buffer to insert into.", "No hay ningún búfer donde insertar."),
    ("There is no buffer to save.", "No hay ningún búfer que guardar."),
    ("There is no text on the pasteboard.", "No hay texto en el portapapeles."),
    ("This buffer has never been saved. Open a folder — File > Open Folder — and Save will ask what to call it.", "Este búfer nunca se ha guardado. Abre una carpeta (Archivo > Abrir carpeta) y Guardar preguntará cómo llamarlo."),
    ("This needs a file on disk — save this buffer first", "Esto necesita un archivo en disco: guarda primero este búfer"),
    ("This window has no pasteboard.", "Esta ventana no tiene portapapeles."),
    ("Tokens reset to the theme's own values.", "Tokens restablecidos a los valores del tema."),
    ("Type something to find first", "Escribe primero algo que buscar"),
    ("Unbalanced quotes in that command", "Comillas desequilibradas en ese comando"),
    ("Word wrap off", "Ajuste de línea desactivado"),
    ("Word wrap on", "Ajuste de línea activado"),
    ("Wrote the file, but not mod.rs: {}", "Se escribió el archivo, pero no mod.rs: {}"),
    ("Wrote {}. Add `{}` wherever this project lists its modules.", "Se escribió {}. Añade `{}` donde este proyecto enumere sus módulos."),
    ("Wrote {}. Merge it with your own theme rather than replacing it blind.", "Se escribió {}. Combínalo con tu tema en lugar de reemplazarlo a ciegas."),
    ("Zen mode — run it again to bring the panes back.", "Modo zen: vuelve a ejecutarlo para recuperar los paneles."),
    ("live.rs is open. The Live Preview draws it as you type.", "live.rs está abierto. La vista previa en vivo lo dibuja mientras escribes."),
    ("rust-analyzer could not find a definition for that", "rust-analyzer no encontró una definición para eso"),
    ("rust-analyzer had nothing to say about that", "rust-analyzer no tenía nada que decir sobre eso"),
    ("rust-analyzer is not running — start it from the palette", "rust-analyzer no está en marcha: inícialo desde la paleta"),
    ("rust-analyzer needs a project — open a folder first.", "rust-analyzer necesita un proyecto: abre primero una carpeta."),
    ("rust-analyzer restarted.", "rust-analyzer reiniciado."),
    ("rust-analyzer starting — the first index takes a while.", "rust-analyzer arrancando: el primer índice tarda un poco."),
    ("rust-analyzer stopped.", "rust-analyzer detenido."),
    ("rust-analyzer: {}", "rust-analyzer: {}"),
    ("{} could not be opened", "No se pudo abrir {}"),
    ("{} definitions — jumped to the first", "{} definiciones: se saltó a la primera"),
    ("{} needs root, which the Output panel cannot give it — opening a terminal instead.", "{} necesita permisos de root, que el panel Salida no puede darle: se abre una terminal."),
    ("{} problem in your customisation files: {}", "{} problema en tus archivos de personalización: {}"),
    ("{} problems in your customisation files: {}", "{} problemas en tus archivos de personalización: {}"),
    ("{} render panic — the last good frame is still on screen.", "{} pánico al renderizar: el último fotograma correcto sigue en pantalla."),
    ("{} render panics — the last good frame is still on screen.", "{} pánicos al renderizar: el último fotograma correcto sigue en pantalla."),
    ("{} template written to {}", "{} plantilla escrita en {}"),
    ("{} templates written to {}", "{} plantillas escritas en {}"),
    ("{} wants a terminal, and this is not one — run it outside the studio", "{} necesita una terminal, y esto no lo es: ejecútalo fuera del estudio"),
    ("{} widget panicked — the rest of the screen still rendered.", "{} widget entró en pánico: el resto de la pantalla se renderizó."),
    ("{} widgets panicked — the rest of the screen still rendered.", "{} widgets entraron en pánico: el resto de la pantalla se renderizó."),
    ("State", "Estado"),
    ("Filter by name", "Filtrar por nombre"),
    ("{} selected · {} × {} at {}, {}", "{} seleccionados · {} × {} en {}, {}"),
    // Status bar, dialogs and build states
    ("ABI mismatch", "ABI incompatible"),
    ("ABI ok", "ABI correcta"),
    ("ABI unchecked", "ABI sin comprobar"),
    ("Building…", "Compilando…"),
    ("Cancel", "Cancelar"),
    ("Changed on disk", "Cambió en el disco"),
    ("Close", "Cerrar"),
    ("Close vieww Studio?", "¿Cerrar vieww Studio?"),
    ("Close without saving", "Cerrar sin guardar"),
    ("Compiling…", "Compilando…"),
    ("Continue", "Continuar"),
    ("Create Project In", "Crear el proyecto en"),
    ("Create live.rs", "Crear live.rs"),
    ("Describe your screens in structured English. Every line shows the Rust it becomes.", "Describe tus pantallas en inglés estructurado. Cada línea muestra el Rust en que se convierte."),
    ("Discard mine and reload", "Descartar la mía y recargar"),
    ("In {}", "En {}"),
    ("It goes to a .viewwstudio-trash folder at the workspace root, not to the wastebasket and not to nothing. You can put it back by hand.", "Va a una carpeta .viewwstudio-trash en la raíz del espacio de trabajo, no a la papelera ni a la nada. Puedes devolverlo a mano."),
    ("Keep my version", "Conservar mi versión"),
    ("Live Preview", "Vista previa en vivo"),
    ("Ln {}, Col {}", "Lín {}, Col {}"),
    ("Move to trash", "Mover a la papelera"),
    ("Move to trash?", "¿Mover a la papelera?"),
    ("New Folder", "Nueva carpeta"),
    ("New Project", "Nuevo proyecto"),
    ("No build yet", "Aún sin compilar"),
    ("No render yet", "Aún sin renderizar"),
    ("Not now", "Ahora no"),
    ("Nothing would be changed — every file that matched is being skipped.", "No se cambiaría nada: se omiten todos los archivos que coincidían."),
    ("OK", "Aceptar"),
    ("One file has changes that have not been written to disk.", "Un archivo tiene cambios que no se han escrito en el disco."),
    ("Open Folder", "Abrir carpeta"),
    ("Read-only", "Solo lectura"),
    ("Rename", "Renombrar"),
    ("Render failed", "Renderizado fallido"),
    ("Rendered", "Renderizado"),
    ("Replace in {} file", "Reemplazar en {} archivo"),
    ("Replace in {} files", "Reemplazar en {} archivos"),
    ("Replace {} match in {} file.", "Reemplazar {} coincidencia en {} archivo."),
    ("Replace {} matches across {} files.", "Reemplazar {} coincidencias en {} archivos."),
    ("Replace {} matches in {} file.", "Reemplazar {} coincidencias en {} archivo."),
    ("Replace “{}” with “{}”?", "¿Reemplazar «{}» por «{}»?"),
    ("Run", "Ejecutar"),
    ("Running…", "Ejecutando…"),
    ("Save As", "Guardar como"),
    ("Save all and close", "Guardar todo y cerrar"),
    ("Spaces: {}", "Espacios: {}"),
    ("That folder is the workspace", "Esa carpeta es el espacio de trabajo"),
    ("The Live Preview draws live.rs at the workspace root — screens, rows, and the buttons between them — without compiling it. There is no such file here, so there is nothing live to show.\n\nWriting one gives you a flow to edit; deleting it later turns the Live Preview off again.", "La vista previa en vivo dibuja live.rs en la raíz del espacio de trabajo (pantallas, filas y los botones entre ellas) sin compilarlo. Aquí no existe ese archivo, así que no hay nada en vivo que mostrar.\n\nEscribir uno te da un flujo para editar; borrarlo después vuelve a desactivar la vista previa en vivo."),
    ("The trash is a folder inside it, so the workspace cannot be moved into its own trash. Close the workspace to stop working on it, or delete the folder from outside the studio.", "La papelera es una carpeta dentro de él, así que el espacio de trabajo no puede moverse a su propia papelera. Cierra el espacio de trabajo para dejar de trabajar en él, o borra la carpeta desde fuera del estudio."),
    ("This cannot be undone: the editor's history is per file, and these files are written directly.", "No se puede deshacer: el historial del editor es por archivo, y estos archivos se escriben directamente."),
    ("This draws live.rs from this workspace without compiling it, so it follows the file as you type. It is a sketch of the flow — screens, rows and the buttons between them — and its switches and counters are the preview's, not your app's.\n\nA `mount` line places one of your own screens in the flow, as it last rendered. Render compiles a screen for real; Build and Run gives you the app.", "Esto dibuja live.rs de este espacio de trabajo sin compilarlo, así que sigue el archivo mientras escribes. Es un boceto del flujo (pantallas, filas y los botones entre ellas), y sus interruptores y contadores son de la vista previa, no de tu app.\n\nUna línea `mount` coloca una de tus pantallas en el flujo, tal como se renderizó por última vez. Renderizar compila una pantalla de verdad; Compilar y ejecutar te da la app."),
    ("This file changed on disk while you had unsaved edits to it. Only one of the two versions can survive.", "Este archivo cambió en el disco mientras tenías cambios sin guardar. Solo una de las dos versiones puede quedarse."),
    ("This workspace has no live.rs", "Este espacio de trabajo no tiene live.rs"),
    ("Write vieww's API directly. Everything Say can do, and everything it cannot.", "Escribe directamente la API de vieww. Todo lo que Say puede hacer, y todo lo que no."),
    ("no rustc", "sin rustc"),
    ("{} cancelled", "{}: cancelado"),
    ("{} cursors", "{} cursores"),
    ("{} failed", "{}: fallido"),
    ("{} files changed on disk while you had unsaved edits to them. Only one of the two versions can survive.", "{} archivos cambiaron en el disco mientras tenías cambios sin guardar. Solo una de las dos versiones puede quedarse."),
    ("{} files have changes that have not been written to disk.", "{} archivos tienen cambios que no se han escrito en el disco."),
    ("{} line folded", "{} línea plegada"),
    ("{} lines folded", "{} líneas plegadas"),
    ("{} succeeded", "{}: correcto"),
    ("{} succeeded · {} warning", "{}: correcto · {} advertencia"),
    ("{} succeeded · {} warnings", "{}: correcto · {} advertencias"),
    ("{} task", "{} tarea"),
    ("{} tasks", "{} tareas"),
    ("· compiling", "· compilando"),
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

const FRENCH: &[(&str, &str)] = &[
    ("Attributing a frame needs a rendered screen — press Render first.", "Attribuer une image nécessite un écran rendu — appuyez d’abord sur Rendu."),
    ("Frame attributed: {}", "Image attribuée : {}"),
    ("Performance overlay off.", "Superposition de performances désactivée."),
    ("Performance overlay on — one bar per frame, red past the budget.", "Superposition de performances activée — une barre par image, en rouge au-delà du budget."),
    ("Preview: Attribute This Frame", "Aperçu : attribuer cette image"),
    ("Preview: Performance Overlay", "Aperçu : superposition de performances"),
    ("This frame: {} ms — text {}, shadows {}, the rest {}, floor {}", "Cette image : {} ms — texte {}, ombres {}, le reste {}, plancher {}"),
    (", skipped {}", ", {} ignorés"),
    (", {} could not be written", ", {} n’ont pas pu être écrits"),
    ("A scratch workspace has no trash to move it to", "Un espace de travail provisoire n’a pas de corbeille"),
    ("A template goes into a project — open a folder first.", "Un modèle va dans un projet : ouvrez d’abord un dossier."),
    ("Activity bar hidden — Ctrl+Alt+U brings it back.", "Barre d’activité masquée — Ctrl+Alt+U la rétablit."),
    ("Activity bar shown.", "Barre d’activité affichée."),
    ("Android phone", "Téléphone Android"),
    ("Android tablet", "Tablette Android"),
    ("Auto Render off.", "Rendu automatique désactivé."),
    ("Auto Render on — the buffer renders 0.6s after you stop typing.", "Rendu automatique activé — le tampon est rendu 0,6 s après la dernière frappe."),
    ("Built. The Export view can install it on a connected device.", "Compilé. La vue Exporter peut l’installer sur un appareil connecté."),
    ("Click anything in the window to select it in the Inspector.", "Cliquez sur un élément de la fenêtre pour le sélectionner dans l’inspecteur."),
    ("Closed {} tab", "{} onglet fermé"),
    ("Closed {} tabs", "{} onglets fermés"),
    ("Closed {}, kept {} with unsaved changes", "{} fermés, {} conservés avec des modifications non enregistrées"),
    ("Command copied.", "Commande copiée."),
    ("Completion needs a file on disk — save this buffer first", "La complétion a besoin d’un fichier sur le disque — enregistrez d’abord ce tampon"),
    ("Copied", "Copié"),
    ("Copied.", "Copié."),
    ("Could not create the settings directory", "Impossible de créer le dossier des paramètres"),
    ("Could not create {}: {}", "Impossible de créer {} : {}"),
    ("Could not write live.rs: {}", "Impossible d’écrire live.rs : {}"),
    ("Could not write {}: {}", "Impossible d’écrire {} : {}"),
    ("Created {}. Rename it before you build on it.", "{} créé. Renommez-le avant de vous en servir."),
    ("Cut.", "Coupé."),
    ("Desktop", "Bureau"),
    ("Dropped {} folders — drop one at a time to choose a workspace", "{} dossiers déposés — déposez-en un seul pour choisir un espace de travail"),
    ("Every one of those has unsaved changes", "Tous ont des modifications non enregistrées"),
    ("Exporting tokens writes a file into a project — open a folder first.", "Exporter les jetons écrit un fichier dans un projet : ouvrez d’abord un dossier."),
    ("Flow file loaded. Click Render to start the Live Preview.", "Fichier de flux chargé. Cliquez sur Rendu pour lancer l’aperçu en direct."),
    ("Hover needs a file on disk — save this buffer first", "Le survol a besoin d’un fichier sur le disque — enregistrez d’abord ce tampon"),
    ("Inserted “{}”.", "« {} » inséré."),
    ("Installing {}. Watch the Output panel.", "Installation de {}. Suivez le panneau Sortie."),
    ("Interface language: {}", "Langue de l’interface : {}"),
    ("Keeping your version — saving will overwrite the change on disk", "Votre version est conservée — l’enregistrement écrasera la modification sur le disque"),
    ("Landscape", "Paysage"),
    ("Laptop", "Ordinateur portable"),
    ("Live preview: drawing live.rs. Render compiles a real screen.", "Aperçu en direct : live.rs est dessiné. Le rendu compile un vrai écran."),
    ("Loaded “{}”{}. Press Render.", "« {} »{} chargé. Appuyez sur Rendu."),
    ("Moved {} to the workspace trash", "{} déplacé dans la corbeille de l’espace de travail"),
    ("No matches in this workspace", "Aucune occurrence dans cet espace de travail"),
    ("No references to that", "Aucune référence à cela"),
    ("No terminal application found. Run this yourself: {}", "Aucun terminal trouvé. Exécutez ceci vous-même : {}"),
    ("No workspace is open", "Aucun espace de travail ouvert"),
    ("Not valid UTF-8 — shown as text, and saving is refused", "UTF-8 invalide — affiché comme texte, l’enregistrement est refusé"),
    ("Nothing is selected.", "Rien n’est sélectionné."),
    ("Nothing to close", "Rien à fermer"),
    ("Open a folder first — live.rs goes at the workspace root.", "Ouvrez d’abord un dossier — live.rs se place à la racine de l’espace de travail."),
    ("Opened a terminal to install {}.", "Un terminal a été ouvert pour installer {}."),
    ("Opened {} files", "{} fichiers ouverts"),
    ("Opened {}.", "{} ouvert."),
    ("Opened {}. Its Cargo.toml does not mention vieww, so Render will refuse.", "{} ouvert. Son Cargo.toml ne mentionne pas vieww, le rendu sera donc refusé."),
    ("Panes restored.", "Panneaux rétablis."),
    ("Path copied", "Chemin copié"),
    ("Phone", "Téléphone"),
    ("Phone (large)", "Téléphone (grand)"),
    ("Phone (small)", "Téléphone (petit)"),
    ("Portrait", "Portrait"),
    ("Preview at {}%.", "Aperçu à {} %."),
    ("Preview fitted to the pane.", "Aperçu ajusté au panneau."),
    ("Preview framed at {}", "Aperçu cadré en {}"),
    ("Preview note hidden. Bring it back from this palette.", "Note de l’aperçu masquée. Rétablissez-la depuis cette palette."),
    ("Preview: Add a Device to the Row", "Aperçu : ajouter un appareil à la rangée"),
    ("Preview: Several Devices Side by Side", "Aperçu : plusieurs appareils côte à côte"),
    ("Preview: dark", "Aperçu : sombre"),
    ("Preview: light", "Aperçu : clair"),
    ("Preview: one device", "Aperçu : un appareil"),
    ("Preview: several devices", "Aperçu : plusieurs appareils"),
    ("Read-only — this file cannot be saved from here", "Lecture seule — ce fichier ne peut pas être enregistré ici"),
    ("Recovered unsaved work: {}", "Travail non enregistré récupéré : {}"),
    ("Reloaded {} file from disk", "{} fichier rechargé depuis le disque"),
    ("Reloaded {} files from disk", "{} fichiers rechargés depuis le disque"),
    ("Repainted regions hidden.", "Zones redessinées masquées."),
    ("Repainted regions shown. Every box is a rectangle the compositor was asked for.", "Zones redessinées affichées. Chaque cadre est un rectangle demandé au compositeur."),
    ("Replace across files needs a folder open", "Remplacer dans plusieurs fichiers nécessite un dossier ouvert"),
    ("Replaced {} match in {} file", "{} occurrence remplacée dans {} fichier"),
    ("Replaced {} matches in {} file", "{} occurrences remplacées dans {} fichier"),
    ("Replaced {} matches in {} files", "{} occurrences remplacées dans {} fichiers"),
    ("Running a command needs a folder open", "Exécuter une commande nécessite un dossier ouvert"),
    ("Sample buffer restored. Press Render.", "Tampon d’exemple rétabli. Appuyez sur Rendu."),
    ("Semantics tree hidden.", "Arbre sémantique masqué."),
    ("Semantics tree shown — role and label, as a screen reader gets them.", "Arbre sémantique affiché — rôle et libellé, tels que les reçoit un lecteur d’écran."),
    ("Tablet", "Tablette"),
    ("That is outside the captured tree — open the Inspector and click again.", "C’est hors de l’arbre capturé — ouvrez l’inspecteur et cliquez à nouveau."),
    ("The pasteboard would not accept it.", "Le presse-papiers l’a refusé."),
    ("The preview note is back under the device frame.", "La note de l’aperçu est revenue sous le cadre de l’appareil."),
    ("There is no buffer to insert into.", "Il n’y a aucun tampon où insérer."),
    ("There is no buffer to save.", "Il n’y a aucun tampon à enregistrer."),
    ("There is no text on the pasteboard.", "Il n’y a pas de texte dans le presse-papiers."),
    ("This buffer has never been saved. Open a folder — File > Open Folder — and Save will ask what to call it.", "Ce tampon n’a jamais été enregistré. Ouvrez un dossier (Fichier > Ouvrir un dossier) et Enregistrer demandera comment le nommer."),
    ("This needs a file on disk — save this buffer first", "Cela nécessite un fichier sur le disque — enregistrez d’abord ce tampon"),
    ("This window has no pasteboard.", "Cette fenêtre n’a pas de presse-papiers."),
    ("Tokens reset to the theme's own values.", "Jetons rétablis aux valeurs du thème."),
    ("Type something to find first", "Tapez d’abord quelque chose à rechercher"),
    ("Unbalanced quotes in that command", "Guillemets non appariés dans cette commande"),
    ("Word wrap off", "Retour à la ligne désactivé"),
    ("Word wrap on", "Retour à la ligne activé"),
    ("Wrote the file, but not mod.rs: {}", "Fichier écrit, mais pas mod.rs : {}"),
    ("Wrote {}. Add `{}` wherever this project lists its modules.", "{} écrit. Ajoutez `{}` là où ce projet liste ses modules."),
    ("Wrote {}. Merge it with your own theme rather than replacing it blind.", "{} écrit. Fusionnez-le avec votre thème plutôt que de le remplacer à l’aveugle."),
    ("Zen mode — run it again to bring the panes back.", "Mode zen — relancez-le pour rétablir les panneaux."),
    ("live.rs is open. The Live Preview draws it as you type.", "live.rs est ouvert. L’aperçu en direct le dessine pendant que vous tapez."),
    ("rust-analyzer could not find a definition for that", "rust-analyzer n’a trouvé aucune définition pour cela"),
    ("rust-analyzer had nothing to say about that", "rust-analyzer n’avait rien à dire à ce sujet"),
    ("rust-analyzer is not running — start it from the palette", "rust-analyzer ne tourne pas — lancez-le depuis la palette"),
    ("rust-analyzer needs a project — open a folder first.", "rust-analyzer a besoin d’un projet : ouvrez d’abord un dossier."),
    ("rust-analyzer restarted.", "rust-analyzer redémarré."),
    ("rust-analyzer starting — the first index takes a while.", "rust-analyzer démarre — la première indexation prend un moment."),
    ("rust-analyzer stopped.", "rust-analyzer arrêté."),
    ("rust-analyzer: {}", "rust-analyzer : {}"),
    ("{} could not be opened", "Impossible d’ouvrir {}"),
    ("{} definitions — jumped to the first", "{} définitions — saut à la première"),
    ("{} needs root, which the Output panel cannot give it — opening a terminal instead.", "{} nécessite les droits root, que le panneau Sortie ne peut pas donner — ouverture d’un terminal."),
    ("{} problem in your customisation files: {}", "{} problème dans vos fichiers de personnalisation : {}"),
    ("{} problems in your customisation files: {}", "{} problèmes dans vos fichiers de personnalisation : {}"),
    ("{} render panic — the last good frame is still on screen.", "{} panique de rendu — la dernière bonne image reste affichée."),
    ("{} render panics — the last good frame is still on screen.", "{} paniques de rendu — la dernière bonne image reste affichée."),
    ("{} template written to {}", "{} modèle écrit dans {}"),
    ("{} templates written to {}", "{} modèles écrits dans {}"),
    ("{} wants a terminal, and this is not one — run it outside the studio", "{} veut un terminal, et ceci n’en est pas un — exécutez-le hors du studio"),
    ("{} widget panicked — the rest of the screen still rendered.", "{} widget a paniqué — le reste de l’écran a été rendu."),
    ("{} widgets panicked — the rest of the screen still rendered.", "{} widgets ont paniqué — le reste de l’écran a été rendu."),
    ("State", "État"),
    ("Filter by name", "Filtrer par nom"),
    ("{} selected · {} × {} at {}, {}", "{} sélectionnés · {} × {} à {}, {}"),
    ("ABI mismatch", "ABI incompatible"),
    ("ABI ok", "ABI correcte"),
    ("ABI unchecked", "ABI non vérifiée"),
    ("About vieww Studio", "À propos de vieww Studio"),
    ("Add Cursor Above", "Ajouter un curseur au-dessus"),
    ("Add Cursor Below", "Ajouter un curseur en dessous"),
    ("Add Cursor at Next Occurrence", "Ajouter un curseur à l’occurrence suivante"),
    ("Auto Render on Change", "Rendu automatique à chaque modification"),
    ("Build", "Compiler"),
    ("Build (Release)", "Compiler (release)"),
    ("Build and Run", "Compiler et exécuter"),
    ("Building…", "Compilation…"),
    ("Cancel", "Annuler"),
    ("Cancel All Tasks", "Annuler toutes les tâches"),
    ("Cancel Build", "Annuler la compilation"),
    ("Cancel Export", "Annuler l’export"),
    ("Cancel Render", "Annuler le rendu"),
    ("Changed on disk", "Modifié sur le disque"),
    ("Clear Extra Cursors", "Supprimer les curseurs supplémentaires"),
    ("Clear Finished Tasks", "Effacer les tâches terminées"),
    ("Clear Output", "Effacer la sortie"),
    ("Close", "Fermer"),
    ("Close Find", "Fermer la recherche"),
    ("Close Other Tabs", "Fermer les autres onglets"),
    ("Close Saved Tabs", "Fermer les onglets enregistrés"),
    ("Close Tab", "Fermer l’onglet"),
    ("Close Tabs to the Right", "Fermer les onglets à droite"),
    ("Close vieww Studio?", "Fermer vieww Studio ?"),
    ("Close without saving", "Fermer sans enregistrer"),
    ("Collapse All Folders", "Replier tous les dossiers"),
    ("Command Palette", "Palette de commandes"),
    ("Commit Staged Changes", "Valider les modifications indexées"),
    ("Compiling…", "Compilation…"),
    ("Complete at Caret", "Compléter au curseur"),
    ("Continue", "Continuer"),
    ("Copy", "Copier"),
    ("Copy Diagnostics for a Bug Report", "Copier les diagnostics pour un rapport de bogue"),
    ("Create Project In", "Créer le projet dans"),
    ("Create live.rs", "Créer live.rs"),
    ("Create live.rs for the Live Preview", "Créer live.rs pour l’aperçu en direct"),
    ("Cut", "Couper"),
    ("Describe your screens in structured English. Every line shows the Rust it becomes.", "Décrivez vos écrans en anglais structuré. Chaque ligne montre le Rust qu’elle devient."),
    ("Discard mine and reload", "Abandonner la mienne et recharger"),
    ("Docs", "Documentation"),
    ("Edit", "Édition"),
    ("Explorer", "Explorateur"),
    ("Export", "Exporter"),
    ("Export (run the selected format)", "Exporter (au format sélectionné)"),
    ("Export…", "Exporter…"),
    ("Fetch", "Récupérer (fetch)"),
    ("File", "Fichier"),
    ("Find", "Rechercher"),
    ("Find Next", "Rechercher le suivant"),
    ("Find Previous", "Rechercher le précédent"),
    ("Find References", "Rechercher les références"),
    ("Find and Replace", "Rechercher et remplacer"),
    ("Fold / Unfold at Cursor", "Replier / déplier au curseur"),
    ("Fold All Regions", "Replier toutes les régions"),
    ("Format Document (rustfmt)", "Formater le document (rustfmt)"),
    ("Go", "Aller"),
    ("Go to Definition", "Aller à la définition"),
    ("Go to File", "Aller au fichier"),
    ("Go to Line", "Aller à la ligne"),
    ("Go to Symbol in File", "Aller au symbole du fichier"),
    ("Help", "Aide"),
    ("In {}", "Dans {}"),
    ("Indent", "Augmenter le retrait"),
    ("Inspector", "Inspecteur"),
    ("Interface Language: Next", "Langue de l’interface : suivante"),
    ("It goes to a .viewwstudio-trash folder at the workspace root, not to the wastebasket and not to nothing. You can put it back by hand.", "Il va dans un dossier .viewwstudio-trash à la racine de l’espace de travail, ni à la corbeille ni au néant. Vous pouvez le remettre à la main."),
    ("Keep my version", "Garder ma version"),
    ("Keyboard Shortcuts", "Raccourcis clavier"),
    ("Learn", "Apprendre"),
    ("Live Preview", "Aperçu en direct"),
    ("Live Preview (No Build)", "Aperçu en direct (sans compiler)"),
    ("Ln {}, Col {}", "Ligne {}, Col {}"),
    ("Load Flow File", "Charger un fichier de flux"),
    ("Move to trash", "Mettre à la corbeille"),
    ("Move to trash?", "Mettre à la corbeille ?"),
    ("New File", "Nouveau fichier"),
    ("New Folder", "Nouveau dossier"),
    ("New Project", "Nouveau projet"),
    ("New Project…", "Nouveau projet…"),
    ("New Screen from Template", "Nouvel écran depuis un modèle"),
    ("New Widget from Template", "Nouveau widget depuis un modèle"),
    ("Next Tab", "Onglet suivant"),
    ("No build yet", "Pas encore de compilation"),
    ("No render yet", "Pas encore de rendu"),
    ("Not now", "Pas maintenant"),
    ("Nothing would be changed — every file that matched is being skipped.", "Rien ne serait modifié : tous les fichiers correspondants sont ignorés."),
    ("OK", "OK"),
    ("One file has changes that have not been written to disk.", "Un fichier a des modifications qui n’ont pas été écrites sur le disque."),
    ("Open Folder", "Ouvrir un dossier"),
    ("Open Folder…", "Ouvrir un dossier…"),
    ("Outdent", "Diminuer le retrait"),
    ("Paste", "Coller"),
    ("Pick a Widget to Inspect", "Choisir un widget à inspecter"),
    ("Preview as Android", "Aperçu en tant qu’Android"),
    ("Preview as Desktop", "Aperçu en tant que bureau"),
    ("Preview as iOS", "Aperçu en tant qu’iOS"),
    ("Preview: Fit to Pane", "Aperçu : ajuster au panneau"),
    ("Preview: Zoom In", "Aperçu : zoom avant"),
    ("Preview: Zoom Out", "Aperçu : zoom arrière"),
    ("Previous Tab", "Onglet précédent"),
    ("Problems", "Problèmes"),
    ("Pull", "Tirer (pull)"),
    ("Push", "Pousser (push)"),
    ("Read-only", "Lecture seule"),
    ("Redo", "Rétablir"),
    ("Redo File Operation", "Rétablir l’opération sur le fichier"),
    ("Refresh Source Control", "Actualiser le contrôle de source"),
    ("Rename", "Renommer"),
    ("Render", "Rendu"),
    ("Render failed", "Échec du rendu"),
    ("Rendered", "Rendu terminé"),
    ("Replace in Files…", "Remplacer dans les fichiers…"),
    ("Replace in {} file", "Remplacer dans {} fichier"),
    ("Replace in {} files", "Remplacer dans {} fichiers"),
    ("Replace {} match in {} file.", "Remplacer {} occurrence dans {} fichier."),
    ("Replace {} matches across {} files.", "Remplacer {} occurrences dans {} fichiers."),
    ("Replace {} matches in {} file.", "Remplacer {} occurrences dans {} fichier."),
    ("Replace “{}” with “{}”?", "Remplacer « {} » par « {} » ?"),
    ("Restore Sample Buffer", "Restaurer le tampon d’exemple"),
    ("Revert File", "Rétablir le fichier"),
    ("Run", "Exécuter"),
    ("Run a Command…", "Exécuter une commande…"),
    ("Running…", "Exécution…"),
    ("Save", "Enregistrer"),
    ("Save All", "Tout enregistrer"),
    ("Save As", "Enregistrer sous"),
    ("Save all and close", "Tout enregistrer et fermer"),
    ("Scan for Devices", "Rechercher des appareils"),
    ("Search", "Rechercher"),
    ("Settings", "Paramètres"),
    ("Show Explorer", "Afficher l’explorateur"),
    ("Show Problems", "Afficher les problèmes"),
    ("Show Repainted Regions", "Afficher les zones redessinées"),
    ("Show Search", "Afficher la recherche"),
    ("Show Semantics Tree", "Afficher l’arbre sémantique"),
    ("Show Settings", "Afficher les paramètres"),
    ("Show Tasks", "Afficher les tâches"),
    ("Show Toolchain", "Afficher la chaîne d’outils"),
    ("Show Welcome", "Afficher l’accueil"),
    ("Show or Hide the Preview Note", "Afficher ou masquer la note de l’aperçu"),
    ("Snippets", "Extraits"),
    ("Source Control", "Contrôle de source"),
    ("Spaces: {}", "Espaces : {}"),
    ("That folder is the workspace", "Ce dossier est l’espace de travail"),
    ("The Live Preview draws live.rs at the workspace root — screens, rows, and the buttons between them — without compiling it. There is no such file here, so there is nothing live to show.\n\nWriting one gives you a flow to edit; deleting it later turns the Live Preview off again.", "L’aperçu en direct dessine live.rs à la racine de l’espace de travail (écrans, lignes et les boutons entre eux) sans le compiler. Ce fichier n’existe pas ici, il n’y a donc rien à montrer.\n\nEn écrire un vous donne un flux à modifier ; le supprimer plus tard désactive à nouveau l’aperçu en direct."),
    ("The trash is a folder inside it, so the workspace cannot be moved into its own trash. Close the workspace to stop working on it, or delete the folder from outside the studio.", "La corbeille est un dossier à l’intérieur, l’espace de travail ne peut donc pas être déplacé dans sa propre corbeille. Fermez l’espace de travail pour arrêter d’y travailler, ou supprimez le dossier en dehors du studio."),
    ("Theme Tokens", "Jetons du thème"),
    ("This cannot be undone: the editor's history is per file, and these files are written directly.", "Impossible d’annuler : l’historique de l’éditeur est propre à chaque fichier, et ces fichiers sont écrits directement."),
    ("This draws live.rs from this workspace without compiling it, so it follows the file as you type. It is a sketch of the flow — screens, rows and the buttons between them — and its switches and counters are the preview's, not your app's.\n\nA `mount` line places one of your own screens in the flow, as it last rendered. Render compiles a screen for real; Build and Run gives you the app.", "Ceci dessine live.rs de cet espace de travail sans le compiler, il suit donc le fichier pendant que vous tapez. C’est une esquisse du flux (écrans, lignes et les boutons entre eux), et ses interrupteurs et compteurs sont ceux de l’aperçu, pas de votre app.\n\nUne ligne `mount` place l’un de vos écrans dans le flux, tel qu’il a été rendu la dernière fois. Le rendu compile vraiment un écran ; Compiler et exécuter vous donne l’app."),
    ("This file changed on disk while you had unsaved edits to it. Only one of the two versions can survive.", "Ce fichier a changé sur le disque alors que vous aviez des modifications non enregistrées. Une seule des deux versions peut être conservée."),
    ("This workspace has no live.rs", "Cet espace de travail n’a pas de live.rs"),
    ("Toggle Activity Bar", "Afficher ou masquer la barre d’activité"),
    ("Toggle Comment", "Commenter ou décommenter"),
    ("Toggle Dark Theme", "Thème sombre"),
    ("Toggle Format on Save", "Formater à l’enregistrement"),
    ("Toggle Indent Guides", "Guides de retrait"),
    ("Toggle Inline Diagnostic Messages", "Messages de diagnostic en ligne"),
    ("Toggle Panel", "Afficher ou masquer le panneau"),
    ("Toggle Preview Dark Mode", "Mode sombre de l’aperçu"),
    ("Toggle Preview Pane", "Afficher ou masquer l’aperçu"),
    ("Toggle Safe-Area Overlay", "Afficher ou masquer la zone sûre"),
    ("Toggle Sidebar", "Afficher ou masquer la barre latérale"),
    ("Toggle Word Wrap", "Retour à la ligne automatique"),
    ("Toolchain", "Chaîne d’outils"),
    ("Try a Say Screen Right Now", "Essayer un écran Say maintenant"),
    ("Undo", "Annuler"),
    ("Undo File Operation", "Annuler l’opération sur le fichier"),
    ("Unfold All Regions", "Déplier toutes les régions"),
    ("View", "Affichage"),
    ("What Is This?", "Qu’est-ce que c’est ?"),
    ("Widget Inspector", "Inspecteur de widgets"),
    ("Write Customisation Templates", "Écrire les modèles de personnalisation"),
    ("Write vieww's API directly. Everything Say can do, and everything it cannot.", "Écrivez directement l’API de vieww. Tout ce que Say sait faire, et tout ce qu’il ne sait pas faire."),
    ("Zen Mode", "Mode zen"),
    ("no rustc", "pas de rustc"),
    ("rust-analyzer: Restart", "rust-analyzer : redémarrer"),
    ("rust-analyzer: Start / Stop", "rust-analyzer : démarrer / arrêter"),
    ("{} cancelled", "{} : annulé"),
    ("{} cursors", "{} curseurs"),
    ("{} failed", "{} : échec"),
    ("{} files changed on disk while you had unsaved edits to them. Only one of the two versions can survive.", "{} fichiers ont changé sur le disque alors que vous aviez des modifications non enregistrées. Une seule des deux versions peut être conservée."),
    ("{} files have changes that have not been written to disk.", "{} fichiers ont des modifications qui n’ont pas été écrites sur le disque."),
    ("{} line folded", "{} ligne repliée"),
    ("{} lines folded", "{} lignes repliées"),
    ("{} succeeded", "{} : réussi"),
    ("{} succeeded · {} warning", "{} : réussi · {} avertissement"),
    ("{} succeeded · {} warnings", "{} : réussi · {} avertissements"),
    ("{} task", "{} tâche"),
    ("{} tasks", "{} tâches"),
    ("· compiling", "· compilation"),
];

const GERMAN: &[(&str, &str)] = &[
    ("Attributing a frame needs a rendered screen — press Render first.", "Ein Bild zuzuordnen braucht einen gerenderten Bildschirm – klicken Sie zuerst auf Rendern."),
    ("Frame attributed: {}", "Bild zugeordnet: {}"),
    ("Performance overlay off.", "Leistungsanzeige aus."),
    ("Performance overlay on — one bar per frame, red past the budget.", "Leistungsanzeige an – ein Balken pro Bild, rot über dem Budget."),
    ("Preview: Attribute This Frame", "Vorschau: dieses Bild zuordnen"),
    ("Preview: Performance Overlay", "Vorschau: Leistungsanzeige"),
    ("This frame: {} ms — text {}, shadows {}, the rest {}, floor {}", "Dieses Bild: {} ms – Text {}, Schatten {}, der Rest {}, Sockel {}"),
    (", skipped {}", ", {} übersprungen"),
    (", {} could not be written", ", {} konnten nicht geschrieben werden"),
    ("A scratch workspace has no trash to move it to", "Ein Behelfsarbeitsbereich hat keinen Papierkorb"),
    ("A template goes into a project — open a folder first.", "Eine Vorlage gehört in ein Projekt – öffnen Sie zuerst einen Ordner."),
    ("Activity bar hidden — Ctrl+Alt+U brings it back.", "Aktivitätsleiste ausgeblendet – Strg+Alt+U holt sie zurück."),
    ("Activity bar shown.", "Aktivitätsleiste eingeblendet."),
    ("Android phone", "Android-Telefon"),
    ("Android tablet", "Android-Tablet"),
    ("Auto Render off.", "Automatisches Rendern aus."),
    ("Auto Render on — the buffer renders 0.6s after you stop typing.", "Automatisches Rendern an – der Puffer wird 0,6 s nach dem letzten Tastendruck gerendert."),
    ("Built. The Export view can install it on a connected device.", "Erstellt. Die Ansicht Exportieren kann es auf einem verbundenen Gerät installieren."),
    ("Click anything in the window to select it in the Inspector.", "Klicken Sie auf etwas im Fenster, um es im Inspektor auszuwählen."),
    ("Closed {} tab", "{} Tab geschlossen"),
    ("Closed {} tabs", "{} Tabs geschlossen"),
    ("Closed {}, kept {} with unsaved changes", "{} geschlossen, {} mit ungespeicherten Änderungen behalten"),
    ("Command copied.", "Befehl kopiert."),
    ("Completion needs a file on disk — save this buffer first", "Vervollständigung braucht eine Datei auf der Festplatte – speichern Sie zuerst diesen Puffer"),
    ("Copied", "Kopiert"),
    ("Copied.", "Kopiert."),
    ("Could not create the settings directory", "Einstellungsordner konnte nicht angelegt werden"),
    ("Could not create {}: {}", "{} konnte nicht angelegt werden: {}"),
    ("Could not write live.rs: {}", "live.rs konnte nicht geschrieben werden: {}"),
    ("Could not write {}: {}", "{} konnte nicht geschrieben werden: {}"),
    ("Created {}. Rename it before you build on it.", "{} angelegt. Benennen Sie es um, bevor Sie darauf aufbauen."),
    ("Cut.", "Ausgeschnitten."),
    ("Desktop", "Desktop"),
    ("Dropped {} folders — drop one at a time to choose a workspace", "{} Ordner abgelegt – legen Sie einen nach dem anderen ab, um einen Arbeitsbereich zu wählen"),
    ("Every one of those has unsaved changes", "Alle haben ungespeicherte Änderungen"),
    ("Exporting tokens writes a file into a project — open a folder first.", "Das Exportieren der Tokens schreibt eine Datei in ein Projekt – öffnen Sie zuerst einen Ordner."),
    ("Flow file loaded. Click Render to start the Live Preview.", "Ablaufdatei geladen. Klicken Sie auf Rendern, um die Live-Vorschau zu starten."),
    ("Hover needs a file on disk — save this buffer first", "Hover-Infos brauchen eine Datei auf der Festplatte – speichern Sie zuerst diesen Puffer"),
    ("Inserted “{}”.", "„{}“ eingefügt."),
    ("Installing {}. Watch the Output panel.", "{} wird installiert. Beobachten Sie den Bereich Ausgabe."),
    ("Interface language: {}", "Sprache der Oberfläche: {}"),
    ("Keeping your version — saving will overwrite the change on disk", "Ihre Version bleibt – beim Speichern wird die Änderung auf der Festplatte überschrieben"),
    ("Landscape", "Querformat"),
    ("Laptop", "Laptop"),
    ("Live preview: drawing live.rs. Render compiles a real screen.", "Live-Vorschau: live.rs wird gezeichnet. Rendern kompiliert einen echten Bildschirm."),
    ("Loaded “{}”{}. Press Render.", "„{}“{} geladen. Klicken Sie auf Rendern."),
    ("Moved {} to the workspace trash", "{} in den Papierkorb des Arbeitsbereichs verschoben"),
    ("No matches in this workspace", "Keine Treffer in diesem Arbeitsbereich"),
    ("No references to that", "Keine Verweise darauf"),
    ("No terminal application found. Run this yourself: {}", "Kein Terminal gefunden. Führen Sie dies selbst aus: {}"),
    ("No workspace is open", "Kein Arbeitsbereich geöffnet"),
    ("Not valid UTF-8 — shown as text, and saving is refused", "Kein gültiges UTF-8 – als Text angezeigt, Speichern wird verweigert"),
    ("Nothing is selected.", "Nichts ausgewählt."),
    ("Nothing to close", "Nichts zu schließen"),
    ("Open a folder first — live.rs goes at the workspace root.", "Öffnen Sie zuerst einen Ordner – live.rs gehört in den Arbeitsbereich."),
    ("Opened a terminal to install {}.", "Ein Terminal wurde geöffnet, um {} zu installieren."),
    ("Opened {} files", "{} Dateien geöffnet"),
    ("Opened {}.", "{} geöffnet."),
    ("Opened {}. Its Cargo.toml does not mention vieww, so Render will refuse.", "{} geöffnet. Seine Cargo.toml erwähnt vieww nicht, daher verweigert Rendern."),
    ("Panes restored.", "Bereiche wiederhergestellt."),
    ("Path copied", "Pfad kopiert"),
    ("Phone", "Telefon"),
    ("Phone (large)", "Telefon (groß)"),
    ("Phone (small)", "Telefon (klein)"),
    ("Portrait", "Hochformat"),
    ("Preview at {}%.", "Vorschau bei {} %."),
    ("Preview fitted to the pane.", "Vorschau an den Bereich angepasst."),
    ("Preview framed at {}", "Vorschau im Rahmen {}"),
    ("Preview note hidden. Bring it back from this palette.", "Vorschauhinweis ausgeblendet. Holen Sie ihn über diese Palette zurück."),
    ("Preview: Add a Device to the Row", "Vorschau: ein Gerät zur Reihe hinzufügen"),
    ("Preview: Several Devices Side by Side", "Vorschau: mehrere Geräte nebeneinander"),
    ("Preview: dark", "Vorschau: dunkel"),
    ("Preview: light", "Vorschau: hell"),
    ("Preview: one device", "Vorschau: ein Gerät"),
    ("Preview: several devices", "Vorschau: mehrere Geräte"),
    ("Read-only — this file cannot be saved from here", "Schreibgeschützt – diese Datei kann von hier nicht gespeichert werden"),
    ("Recovered unsaved work: {}", "Ungespeicherte Arbeit wiederhergestellt: {}"),
    ("Reloaded {} file from disk", "{} Datei von der Festplatte neu geladen"),
    ("Reloaded {} files from disk", "{} Dateien von der Festplatte neu geladen"),
    ("Repainted regions hidden.", "Neu gezeichnete Bereiche ausgeblendet."),
    ("Repainted regions shown. Every box is a rectangle the compositor was asked for.", "Neu gezeichnete Bereiche eingeblendet. Jeder Kasten ist ein Rechteck, das beim Compositor angefordert wurde."),
    ("Replace across files needs a folder open", "Ersetzen in mehreren Dateien braucht einen geöffneten Ordner"),
    ("Replaced {} match in {} file", "{} Treffer in {} Datei ersetzt"),
    ("Replaced {} matches in {} file", "{} Treffer in {} Datei ersetzt"),
    ("Replaced {} matches in {} files", "{} Treffer in {} Dateien ersetzt"),
    ("Running a command needs a folder open", "Ein Befehl braucht einen geöffneten Ordner"),
    ("Sample buffer restored. Press Render.", "Beispielpuffer wiederhergestellt. Klicken Sie auf Rendern."),
    ("Semantics tree hidden.", "Semantikbaum ausgeblendet."),
    ("Semantics tree shown — role and label, as a screen reader gets them.", "Semantikbaum eingeblendet – Rolle und Beschriftung, wie ein Screenreader sie erhält."),
    ("Tablet", "Tablet"),
    ("That is outside the captured tree — open the Inspector and click again.", "Das liegt außerhalb des erfassten Baums – öffnen Sie den Inspektor und klicken Sie erneut."),
    ("The pasteboard would not accept it.", "Die Zwischenablage hat es nicht angenommen."),
    ("The preview note is back under the device frame.", "Der Vorschauhinweis ist wieder unter dem Geräterahmen."),
    ("There is no buffer to insert into.", "Es gibt keinen Puffer zum Einfügen."),
    ("There is no buffer to save.", "Es gibt keinen Puffer zum Speichern."),
    ("There is no text on the pasteboard.", "In der Zwischenablage ist kein Text."),
    ("This buffer has never been saved. Open a folder — File > Open Folder — and Save will ask what to call it.", "Dieser Puffer wurde nie gespeichert. Öffnen Sie einen Ordner (Datei > Ordner öffnen), dann fragt Speichern nach dem Namen."),
    ("This needs a file on disk — save this buffer first", "Dafür braucht es eine Datei auf der Festplatte – speichern Sie zuerst diesen Puffer"),
    ("This window has no pasteboard.", "Dieses Fenster hat keine Zwischenablage."),
    ("Tokens reset to the theme's own values.", "Tokens auf die Werte des Themes zurückgesetzt."),
    ("Type something to find first", "Geben Sie zuerst einen Suchbegriff ein"),
    ("Unbalanced quotes in that command", "Unausgeglichene Anführungszeichen in diesem Befehl"),
    ("Word wrap off", "Zeilenumbruch aus"),
    ("Word wrap on", "Zeilenumbruch an"),
    ("Wrote the file, but not mod.rs: {}", "Datei geschrieben, aber nicht mod.rs: {}"),
    ("Wrote {}. Add `{}` wherever this project lists its modules.", "{} geschrieben. Fügen Sie `{}` dort ein, wo dieses Projekt seine Module auflistet."),
    ("Wrote {}. Merge it with your own theme rather than replacing it blind.", "{} geschrieben. Führen Sie es mit Ihrem Theme zusammen, statt es blind zu ersetzen."),
    ("Zen mode — run it again to bring the panes back.", "Zen-Modus – erneut ausführen, um die Bereiche zurückzuholen."),
    ("live.rs is open. The Live Preview draws it as you type.", "live.rs ist geöffnet. Die Live-Vorschau zeichnet es beim Tippen."),
    ("rust-analyzer could not find a definition for that", "rust-analyzer hat dafür keine Definition gefunden"),
    ("rust-analyzer had nothing to say about that", "rust-analyzer hatte dazu nichts zu sagen"),
    ("rust-analyzer is not running — start it from the palette", "rust-analyzer läuft nicht – starten Sie ihn über die Palette"),
    ("rust-analyzer needs a project — open a folder first.", "rust-analyzer braucht ein Projekt – öffnen Sie zuerst einen Ordner."),
    ("rust-analyzer restarted.", "rust-analyzer neu gestartet."),
    ("rust-analyzer starting — the first index takes a while.", "rust-analyzer startet – die erste Indizierung dauert eine Weile."),
    ("rust-analyzer stopped.", "rust-analyzer gestoppt."),
    ("rust-analyzer: {}", "rust-analyzer: {}"),
    ("{} could not be opened", "{} konnte nicht geöffnet werden"),
    ("{} definitions — jumped to the first", "{} Definitionen – zur ersten gesprungen"),
    ("{} needs root, which the Output panel cannot give it — opening a terminal instead.", "{} braucht Root-Rechte, die der Bereich Ausgabe nicht geben kann – stattdessen wird ein Terminal geöffnet."),
    ("{} problem in your customisation files: {}", "{} Problem in Ihren Anpassungsdateien: {}"),
    ("{} problems in your customisation files: {}", "{} Probleme in Ihren Anpassungsdateien: {}"),
    ("{} render panic — the last good frame is still on screen.", "{} Render-Panik – das letzte gute Bild bleibt sichtbar."),
    ("{} render panics — the last good frame is still on screen.", "{} Render-Paniken – das letzte gute Bild bleibt sichtbar."),
    ("{} template written to {}", "{} Vorlage nach {} geschrieben"),
    ("{} templates written to {}", "{} Vorlagen nach {} geschrieben"),
    ("{} wants a terminal, and this is not one — run it outside the studio", "{} will ein Terminal, und das hier ist keins – führen Sie es außerhalb des Studios aus"),
    ("{} widget panicked — the rest of the screen still rendered.", "{} Widget ist abgestürzt – der Rest des Bildschirms wurde gerendert."),
    ("{} widgets panicked — the rest of the screen still rendered.", "{} Widgets sind abgestürzt – der Rest des Bildschirms wurde gerendert."),
    ("State", "Zustand"),
    ("Filter by name", "Nach Namen filtern"),
    ("{} selected · {} × {} at {}, {}", "{} ausgewählt · {} × {} bei {}, {}"),
    ("ABI mismatch", "ABI passt nicht"),
    ("ABI ok", "ABI in Ordnung"),
    ("ABI unchecked", "ABI ungeprüft"),
    ("About vieww Studio", "Über vieww Studio"),
    ("Add Cursor Above", "Cursor darüber hinzufügen"),
    ("Add Cursor Below", "Cursor darunter hinzufügen"),
    ("Add Cursor at Next Occurrence", "Cursor beim nächsten Vorkommen hinzufügen"),
    ("Auto Render on Change", "Bei Änderung automatisch rendern"),
    ("Build", "Erstellen"),
    ("Build (Release)", "Erstellen (Release)"),
    ("Build and Run", "Erstellen und ausführen"),
    ("Building…", "Wird erstellt…"),
    ("Cancel", "Abbrechen"),
    ("Cancel All Tasks", "Alle Aufgaben abbrechen"),
    ("Cancel Build", "Erstellen abbrechen"),
    ("Cancel Export", "Export abbrechen"),
    ("Cancel Render", "Rendern abbrechen"),
    ("Changed on disk", "Auf der Festplatte geändert"),
    ("Clear Extra Cursors", "Zusätzliche Cursor entfernen"),
    ("Clear Finished Tasks", "Erledigte Aufgaben entfernen"),
    ("Clear Output", "Ausgabe leeren"),
    ("Close", "Schließen"),
    ("Close Find", "Suche schließen"),
    ("Close Other Tabs", "Andere Tabs schließen"),
    ("Close Saved Tabs", "Gespeicherte Tabs schließen"),
    ("Close Tab", "Tab schließen"),
    ("Close Tabs to the Right", "Tabs rechts schließen"),
    ("Close vieww Studio?", "vieww Studio schließen?"),
    ("Close without saving", "Schließen ohne zu speichern"),
    ("Collapse All Folders", "Alle Ordner einklappen"),
    ("Command Palette", "Befehlspalette"),
    ("Commit Staged Changes", "Vorgemerkte Änderungen committen"),
    ("Compiling…", "Wird kompiliert…"),
    ("Complete at Caret", "An der Einfügemarke vervollständigen"),
    ("Continue", "Weiter"),
    ("Copy", "Kopieren"),
    ("Copy Diagnostics for a Bug Report", "Diagnose für einen Fehlerbericht kopieren"),
    ("Create Project In", "Projekt anlegen in"),
    ("Create live.rs", "live.rs anlegen"),
    ("Create live.rs for the Live Preview", "live.rs für die Live-Vorschau anlegen"),
    ("Cut", "Ausschneiden"),
    ("Describe your screens in structured English. Every line shows the Rust it becomes.", "Beschreiben Sie Ihre Bildschirme in strukturiertem Englisch. Jede Zeile zeigt das Rust, zu dem sie wird."),
    ("Discard mine and reload", "Meine verwerfen und neu laden"),
    ("Docs", "Dokumentation"),
    ("Edit", "Bearbeiten"),
    ("Explorer", "Explorer"),
    ("Export", "Exportieren"),
    ("Export (run the selected format)", "Exportieren (im gewählten Format)"),
    ("Export…", "Exportieren…"),
    ("Fetch", "Abrufen (fetch)"),
    ("File", "Datei"),
    ("Find", "Suchen"),
    ("Find Next", "Weitersuchen"),
    ("Find Previous", "Rückwärts suchen"),
    ("Find References", "Verweise suchen"),
    ("Find and Replace", "Suchen und ersetzen"),
    ("Fold / Unfold at Cursor", "Am Cursor ein-/ausklappen"),
    ("Fold All Regions", "Alle Bereiche einklappen"),
    ("Format Document (rustfmt)", "Dokument formatieren (rustfmt)"),
    ("Go", "Gehe zu"),
    ("Go to Definition", "Gehe zur Definition"),
    ("Go to File", "Gehe zu Datei"),
    ("Go to Line", "Gehe zu Zeile"),
    ("Go to Symbol in File", "Gehe zu Symbol in der Datei"),
    ("Help", "Hilfe"),
    ("In {}", "In {}"),
    ("Indent", "Einrücken"),
    ("Inspector", "Inspektor"),
    ("Interface Language: Next", "Sprache der Oberfläche: nächste"),
    ("It goes to a .viewwstudio-trash folder at the workspace root, not to the wastebasket and not to nothing. You can put it back by hand.", "Es kommt in einen Ordner .viewwstudio-trash im Arbeitsbereich, nicht in den Papierkorb und nicht ins Nichts. Sie können es von Hand zurücklegen."),
    ("Keep my version", "Meine Version behalten"),
    ("Keyboard Shortcuts", "Tastenkürzel"),
    ("Learn", "Lernen"),
    ("Live Preview", "Live-Vorschau"),
    ("Live Preview (No Build)", "Live-Vorschau (ohne Erstellen)"),
    ("Ln {}, Col {}", "Z. {}, Sp. {}"),
    ("Load Flow File", "Ablaufdatei laden"),
    ("Move to trash", "In den Papierkorb"),
    ("Move to trash?", "In den Papierkorb verschieben?"),
    ("New File", "Neue Datei"),
    ("New Folder", "Neuer Ordner"),
    ("New Project", "Neues Projekt"),
    ("New Project…", "Neues Projekt…"),
    ("New Screen from Template", "Neuer Bildschirm aus Vorlage"),
    ("New Widget from Template", "Neues Widget aus Vorlage"),
    ("Next Tab", "Nächster Tab"),
    ("No build yet", "Noch nicht erstellt"),
    ("No render yet", "Noch nicht gerendert"),
    ("Not now", "Nicht jetzt"),
    ("Nothing would be changed — every file that matched is being skipped.", "Nichts würde geändert – jede passende Datei wird übersprungen."),
    ("OK", "OK"),
    ("One file has changes that have not been written to disk.", "Eine Datei hat Änderungen, die noch nicht gespeichert sind."),
    ("Open Folder", "Ordner öffnen"),
    ("Open Folder…", "Ordner öffnen…"),
    ("Outdent", "Ausrücken"),
    ("Paste", "Einfügen"),
    ("Pick a Widget to Inspect", "Widget zum Untersuchen wählen"),
    ("Preview as Android", "Vorschau als Android"),
    ("Preview as Desktop", "Vorschau als Desktop"),
    ("Preview as iOS", "Vorschau als iOS"),
    ("Preview: Fit to Pane", "Vorschau: an Bereich anpassen"),
    ("Preview: Zoom In", "Vorschau: vergrößern"),
    ("Preview: Zoom Out", "Vorschau: verkleinern"),
    ("Previous Tab", "Vorheriger Tab"),
    ("Problems", "Probleme"),
    ("Pull", "Holen (pull)"),
    ("Push", "Hochladen (push)"),
    ("Read-only", "Schreibgeschützt"),
    ("Redo", "Wiederholen"),
    ("Redo File Operation", "Dateivorgang wiederholen"),
    ("Refresh Source Control", "Versionskontrolle aktualisieren"),
    ("Rename", "Umbenennen"),
    ("Render", "Rendern"),
    ("Render failed", "Rendern fehlgeschlagen"),
    ("Rendered", "Gerendert"),
    ("Replace in Files…", "In Dateien ersetzen…"),
    ("Replace in {} file", "In {} Datei ersetzen"),
    ("Replace in {} files", "In {} Dateien ersetzen"),
    ("Replace {} match in {} file.", "{} Treffer in {} Datei ersetzen."),
    ("Replace {} matches across {} files.", "{} Treffer in {} Dateien ersetzen."),
    ("Replace {} matches in {} file.", "{} Treffer in {} Datei ersetzen."),
    ("Replace “{}” with “{}”?", "„{}“ durch „{}“ ersetzen?"),
    ("Restore Sample Buffer", "Beispielpuffer wiederherstellen"),
    ("Revert File", "Datei zurücksetzen"),
    ("Run", "Ausführen"),
    ("Run a Command…", "Befehl ausführen…"),
    ("Running…", "Läuft…"),
    ("Save", "Speichern"),
    ("Save All", "Alle speichern"),
    ("Save As", "Speichern unter"),
    ("Save all and close", "Alle speichern und schließen"),
    ("Scan for Devices", "Nach Geräten suchen"),
    ("Search", "Suche"),
    ("Settings", "Einstellungen"),
    ("Show Explorer", "Explorer anzeigen"),
    ("Show Problems", "Probleme anzeigen"),
    ("Show Repainted Regions", "Neu gezeichnete Bereiche anzeigen"),
    ("Show Search", "Suche anzeigen"),
    ("Show Semantics Tree", "Semantikbaum anzeigen"),
    ("Show Settings", "Einstellungen anzeigen"),
    ("Show Tasks", "Aufgaben anzeigen"),
    ("Show Toolchain", "Toolchain anzeigen"),
    ("Show Welcome", "Willkommen anzeigen"),
    ("Show or Hide the Preview Note", "Vorschauhinweis ein- oder ausblenden"),
    ("Snippets", "Snippets"),
    ("Source Control", "Versionskontrolle"),
    ("Spaces: {}", "Leerzeichen: {}"),
    ("That folder is the workspace", "Dieser Ordner ist der Arbeitsbereich"),
    ("The Live Preview draws live.rs at the workspace root — screens, rows, and the buttons between them — without compiling it. There is no such file here, so there is nothing live to show.\n\nWriting one gives you a flow to edit; deleting it later turns the Live Preview off again.", "Die Live-Vorschau zeichnet live.rs im Arbeitsbereich – Bildschirme, Zeilen und die Schaltflächen dazwischen –, ohne es zu kompilieren. Hier gibt es keine solche Datei, also gibt es nichts zu zeigen.\n\nWenn Sie eine schreiben, haben Sie einen Ablauf zum Bearbeiten; löschen Sie sie später, ist die Live-Vorschau wieder aus."),
    ("The trash is a folder inside it, so the workspace cannot be moved into its own trash. Close the workspace to stop working on it, or delete the folder from outside the studio.", "Der Papierkorb ist ein Ordner darin, daher kann der Arbeitsbereich nicht in seinen eigenen Papierkorb verschoben werden. Schließen Sie den Arbeitsbereich, um nicht mehr daran zu arbeiten, oder löschen Sie den Ordner außerhalb des Studios."),
    ("Theme Tokens", "Theme-Tokens"),
    ("This cannot be undone: the editor's history is per file, and these files are written directly.", "Das lässt sich nicht rückgängig machen: Der Verlauf des Editors gilt je Datei, und diese Dateien werden direkt geschrieben."),
    ("This draws live.rs from this workspace without compiling it, so it follows the file as you type. It is a sketch of the flow — screens, rows and the buttons between them — and its switches and counters are the preview's, not your app's.\n\nA `mount` line places one of your own screens in the flow, as it last rendered. Render compiles a screen for real; Build and Run gives you the app.", "Dies zeichnet live.rs aus diesem Arbeitsbereich, ohne es zu kompilieren, und folgt der Datei beim Tippen. Es ist eine Skizze des Ablaufs – Bildschirme, Zeilen und die Schaltflächen dazwischen –, und seine Schalter und Zähler gehören der Vorschau, nicht Ihrer App.\n\nEine `mount`-Zeile setzt einen Ihrer eigenen Bildschirme in den Ablauf, so wie er zuletzt gerendert wurde. Rendern kompiliert einen Bildschirm wirklich; Erstellen und ausführen liefert die App."),
    ("This file changed on disk while you had unsaved edits to it. Only one of the two versions can survive.", "Diese Datei wurde auf der Festplatte geändert, während Sie ungespeicherte Änderungen hatten. Nur eine der beiden Versionen kann bleiben."),
    ("This workspace has no live.rs", "Dieser Arbeitsbereich hat keine live.rs"),
    ("Toggle Activity Bar", "Aktivitätsleiste ein-/ausblenden"),
    ("Toggle Comment", "Kommentar umschalten"),
    ("Toggle Dark Theme", "Dunkles Design umschalten"),
    ("Toggle Format on Save", "Beim Speichern formatieren umschalten"),
    ("Toggle Indent Guides", "Einrückungslinien umschalten"),
    ("Toggle Inline Diagnostic Messages", "Diagnosemeldungen im Text umschalten"),
    ("Toggle Panel", "Bereich ein-/ausblenden"),
    ("Toggle Preview Dark Mode", "Dunkelmodus der Vorschau umschalten"),
    ("Toggle Preview Pane", "Vorschau ein-/ausblenden"),
    ("Toggle Safe-Area Overlay", "Sicheren Bereich ein-/ausblenden"),
    ("Toggle Sidebar", "Seitenleiste ein-/ausblenden"),
    ("Toggle Word Wrap", "Zeilenumbruch umschalten"),
    ("Toolchain", "Toolchain"),
    ("Try a Say Screen Right Now", "Jetzt einen Say-Bildschirm ausprobieren"),
    ("Undo", "Rückgängig"),
    ("Undo File Operation", "Dateivorgang rückgängig machen"),
    ("Unfold All Regions", "Alle Bereiche ausklappen"),
    ("View", "Ansicht"),
    ("What Is This?", "Was ist das?"),
    ("Widget Inspector", "Widget-Inspektor"),
    ("Write Customisation Templates", "Anpassungsvorlagen schreiben"),
    ("Write vieww's API directly. Everything Say can do, and everything it cannot.", "Schreiben Sie die API von vieww direkt. Alles, was Say kann, und alles, was es nicht kann."),
    ("Zen Mode", "Zen-Modus"),
    ("no rustc", "kein rustc"),
    ("rust-analyzer: Restart", "rust-analyzer: neu starten"),
    ("rust-analyzer: Start / Stop", "rust-analyzer: starten / stoppen"),
    ("{} cancelled", "{} abgebrochen"),
    ("{} cursors", "{} Cursor"),
    ("{} failed", "{} fehlgeschlagen"),
    ("{} files changed on disk while you had unsaved edits to them. Only one of the two versions can survive.", "{} Dateien wurden auf der Festplatte geändert, während Sie ungespeicherte Änderungen hatten. Nur eine der beiden Versionen kann bleiben."),
    ("{} files have changes that have not been written to disk.", "{} Dateien haben Änderungen, die noch nicht gespeichert sind."),
    ("{} line folded", "{} Zeile eingeklappt"),
    ("{} lines folded", "{} Zeilen eingeklappt"),
    ("{} succeeded", "{} erfolgreich"),
    ("{} succeeded · {} warning", "{} erfolgreich · {} Warnung"),
    ("{} succeeded · {} warnings", "{} erfolgreich · {} Warnungen"),
    ("{} task", "{} Aufgabe"),
    ("{} tasks", "{} Aufgaben"),
    ("· compiling", "· kompiliert"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Menu};
    use crate::state::{NameKind, PickerMode, PreviewState, View};

    /// A Rust string literal's value, from just after its opening quote.
    fn literal(src: &[u8], mut at: usize) -> Option<String> {
        let mut out = String::new();
        while at < src.len() {
            match src[at] {
                b'"' => return Some(out),
                b'\\' => {
                    at += 1;
                    match src.get(at)? {
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'\'' => out.push('\''),
                        b'u' => {
                            let close = at + src[at..].iter().position(|c| *c == b'}')?;
                            let hex = std::str::from_utf8(&src[at + 2..close]).ok()?;
                            out.push(char::from_u32(u32::from_str_radix(hex, 16).ok()?)?);
                            at = close;
                        }
                        b'\n' => {
                            while src.get(at + 1).is_some_and(u8::is_ascii_whitespace) {
                                at += 1;
                            }
                        }
                        _ => return None,
                    }
                    at += 1;
                }
                _ => {
                    let ch = std::str::from_utf8(&src[at..]).ok()?.chars().next()?;
                    out.push(ch);
                    at += ch.len_utf8();
                }
            }
        }
        None
    }

    /// Every string literal passed straight to `t("…")` or `tf("…")` in the
    /// studio's sources: the prose the chrome draws (status bar, dialogs,
    /// build states). Parsed as Rust string literals, escapes and line
    /// continuations included, so the key matches the `&'static str` the
    /// call passes.
    fn source_keys() -> Vec<String> {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).expect("src").flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut keys = Vec::new();
        for f in files {
            if f.ends_with("i18n.rs") {
                continue;
            }
            let text = std::fs::read(&f).expect("source");
            for name in [&b"t("[..], &b"tf("[..]] {
                let mut from = 0;
                while let Some(off) = text[from..].windows(name.len()).position(|w| w == name) {
                    let at = from + off;
                    from = at + name.len();
                    let before = at.checked_sub(1).map(|k| text[k]);
                    if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.') {
                        continue;
                    }
                    let mut k = at + name.len();
                    while text.get(k).is_some_and(u8::is_ascii_whitespace) {
                        k += 1;
                    }
                    if text.get(k) == Some(&b'"') {
                        if let Some(lit) = literal(&text, k + 1) {
                            keys.push(lit);
                        }
                    }
                }
            }
        }
        keys.extend(notify_keys());
        keys.sort();
        keys.dedup();
        keys
    }

    /// `{name}`, `{0}`, `{:.1}` → `{}`: a `format!` template as the catalog
    /// records it.
    fn normalize(template: &str) -> String {
        let mut out = String::new();
        let mut chars = template.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    out.push('{');
                    continue;
                }
                for d in chars.by_ref() {
                    if d == '}' {
                        break;
                    }
                }
                out.push_str("{}");
            } else if c == '}' && chars.peek() == Some(&'}') {
                chars.next();
                out.push('}');
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Every string literal inside a `notify(…)` call — the message, a
    /// `format!` template, and literal values like "Landscape" — except
    /// in calls that go through `t`/`tf` (already collected), and except
    /// literals with no words in them (a lone `{}`).
    fn notify_keys() -> Vec<String> {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).expect("src").flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut keys = Vec::new();
        for f in files {
            let text = std::fs::read_to_string(&f).expect("source");
            let bytes = text.as_bytes();
            let mut from = 0;
            while let Some(off) = text[from..].find(".notify(") {
                let start = from + off + ".notify(".len();
                from = start;
                // The call's argument: up to the matching parenthesis,
                // string literals and char literals skipped.
                let mut depth = 1;
                let mut end = start;
                let mut in_str = false;
                let mut literals: Vec<(usize, usize)> = Vec::new();
                let mut lit_start = 0;
                while end < bytes.len() && depth > 0 {
                    match bytes[end] {
                        b'\\' if in_str => end += 1,
                        b'\'' if !in_str && bytes.get(end + 2) == Some(&b'\'') => end += 2,
                        b'"' => {
                            if in_str {
                                literals.push((lit_start, end));
                            } else {
                                lit_start = end + 1;
                            }
                            in_str = !in_str;
                        }
                        b'(' if !in_str => depth += 1,
                        b')' if !in_str => depth -= 1,
                        _ => {}
                    }
                    end += 1;
                }
                let arg = &text[start..end.saturating_sub(1)];
                if arg.contains("i18n::t") || arg.contains("tf(") {
                    continue;
                }
                for (a, _) in literals {
                    if let Some(lit) = literal(bytes, a) {
                        let key = normalize(&lit);
                        if key.chars().any(char::is_alphabetic) {
                            keys.push(key);
                        }
                    }
                }
            }
        }
        keys
    }

    /// Everything the chrome passes through the catalog.
    fn all_keys() -> Vec<String> {
        let mut keys: Vec<String> = Command::ALL.iter().map(|c| c.title().to_owned()).collect();
        keys.extend(View::ALL.iter().map(|v| v.title().to_owned()));
        keys.extend(
            Menu::BAR
                .iter()
                .map(|m| m.title().to_owned())
                .filter(|t| !t.is_empty()),
        );
        // Enum labels the chrome passes to `t` through a variable.
        for s in [
            PreviewState::Empty,
            PreviewState::Compiling,
            PreviewState::Rendered,
            PreviewState::Failed,
            PreviewState::AbiRefused,
        ] {
            keys.push(s.status_label().to_owned());
        }
        for k in [
            NameKind::NewFile,
            NameKind::NewFolder,
            NameKind::Rename,
            NameKind::NewProject,
            NameKind::SaveAs,
        ] {
            keys.push(k.title().to_owned());
        }
        for m in [PickerMode::Open, PickerMode::NewProject] {
            keys.push(m.title().to_owned());
        }
        for k in [
            crate::builds::Kind::Build,
            crate::builds::Kind::BuildAndRun,
            crate::builds::Kind::Run,
        ] {
            keys.push(k.label().to_owned());
        }
        keys.extend(crate::state::Device::ALL.iter().map(|d| d.name.to_owned()));
        for k in [
            crate::scaffold::ProjectKind::Say,
            crate::scaffold::ProjectKind::Rust,
        ] {
            keys.push(k.blurb().to_owned());
        }
        keys.extend(source_keys());
        keys.sort();
        keys.dedup();
        keys
    }

    /// No string ships in English by accident: every title, label and
    /// piece of prose the chrome translates has an entry in every catalog.
    #[test]
    fn every_title_has_an_entry_in_every_catalog() {
        let keys = all_keys();
        assert!(
            keys.len() > 150,
            "the key scan found only {} keys",
            keys.len()
        );
        for lang in Language::ALL {
            let missing: Vec<&String> = keys.iter().filter(|t| !has_entry(lang, t)).collect();
            assert!(
                missing.is_empty(),
                "{} has no entry for {missing:?}",
                lang.name()
            );
        }
    }

    /// The source scan sees the prose, so a new `t("…")` without a
    /// translation fails the test above rather than shipping English.
    #[test]
    fn the_scan_finds_the_status_bar_and_dialog_prose() {
        let keys = source_keys();
        for k in [
            "Read-only",
            "Ln {}, Col {}",
            "Move to trash?",
            "{} succeeded",
        ] {
            assert!(keys.iter().any(|x| x == k), "{k:?} not found by the scan");
        }
        assert!(
            keys.iter().any(|k| k
                .starts_with("This cannot be undone: the editor's history is per file, and these")),
            "a line continuation is read as the literal it makes"
        );
    }

    #[test]
    fn languages_resolve_and_fall_back() {
        assert_eq!(Language::from_code("es-MX"), Language::Spanish);
        assert_eq!(Language::from_code("fr_CA"), Language::French);
        assert_eq!(Language::from_code("de"), Language::German);
        assert_eq!(Language::from_code("ja"), Language::English);
        assert_eq!(translate(Language::Spanish, "Save"), "Guardar");
        assert_eq!(translate(Language::French, "Save"), "Enregistrer");
        assert_eq!(translate(Language::German, "Save"), "Speichern");
        assert_eq!(translate(Language::English, "Save"), "Save");
        set_language(Language::German);
        assert_eq!(tf("Ln {}, Col {}", &[&3, &9]), "Z. 3, Sp. 9");
        set_language(Language::Spanish);
        assert_eq!(t("Undo"), "Deshacer");
        set_language(Language::English);
        assert_eq!(t("Undo"), "Undo");
        assert_eq!(tf("Ln {}, Col {}", &[&3, &9]), "Ln 3, Col 9");
    }

    /// Placeholders survive translation: a template's `{}` count is the
    /// same in every language.
    #[test]
    fn every_translation_keeps_its_placeholders() {
        for lang in Language::ALL {
            for (k, v) in lang.table() {
                assert_eq!(
                    k.matches("{}").count(),
                    v.matches("{}").count(),
                    "{}: {k:?} -> {v:?}",
                    lang.name()
                );
            }
        }
    }

    #[test]
    fn the_catalog_has_no_stale_keys() {
        let keys = all_keys();
        for lang in Language::ALL {
            let stale: Vec<&str> = lang
                .table()
                .iter()
                .map(|(k, _)| *k)
                .filter(|k| !keys.iter().any(|x| x == k))
                .collect();
            assert!(
                stale.is_empty(),
                "{}: keys nothing uses: {stale:?}",
                lang.name()
            );
        }
    }

    /// Prints the keys a catalog lacks (run with --ignored --nocapture).
    #[test]
    #[ignore = "a helper for translators"]
    fn print_missing_keys() {
        for lang in Language::ALL {
            for k in all_keys().iter().filter(|t| !has_entry(lang, t)) {
                println!("{}\t{k:?}", lang.code());
            }
        }
    }
}
