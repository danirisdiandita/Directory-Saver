mod paste;
mod store;
mod text_input;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gpui::{
    App, Application, Bounds, ClickEvent, Context, Entity, FocusHandle, Focusable, FontWeight,
    Hsla, KeyBinding, PathPromptOptions, ScrollStrategy, SharedString, Subscription,
    TitlebarOptions, UniformListScrollHandle, Window, WindowBounds, WindowOptions, actions, div,
    prelude::*, px, rgb, rgba, size, uniform_list,
};

use store::{Directory, Store};
use text_input::{InputColors, TextInput};

const BG: u32 = 0x0F1011;
const SURFACE: u32 = 0x151618;
const BORDER: u32 = 0x23252A;
const TEXT: u32 = 0xF7F8F8;
const MUTED: u32 = 0x8A8F98;
const SUBTLE: u32 = 0x62666D;
const ACCENT: u32 = 0x5E6AD2;
const SELECTED: u32 = 0x1C1D21;
const HOVER: u32 = 0x18191B;
const DANGER: u32 = 0xEB5757;
const FONT: &str = "Inter";
const ROW_HEIGHT: f32 = 44.;

actions!(
    directory_saver,
    [SelectPrev, SelectNext, Confirm, Cancel, OpenFolder, PastePath, RemoveSelected, Quit]
);

const CONTEXT: &str = "DirectorySaver";

enum Notice {
    Error(SharedString),
    Info(SharedString),
}

/// What the one text input is for right now.
#[derive(Clone, Copy, PartialEq)]
enum InputMode {
    Search,
    AddPath,
}

impl InputMode {
    fn placeholder(self) -> &'static str {
        match self {
            InputMode::Search => "Search directories…",
            InputMode::AddPath => "Paste a directory path…",
        }
    }
}

enum PathCheck {
    Empty,
    Ready(PathBuf),
    Invalid(&'static str),
}

/// Which row should be selected once results are reloaded.
enum Keep {
    First,
    Id(i64),
    Index,
}

struct DirectorySaver {
    store: Store,
    input: Entity<TextInput>,
    mode: InputMode,
    results: Vec<Directory>,
    selected: Option<usize>,
    notice: Option<Notice>,
    scroll: UniformListScrollHandle,
    home: Option<PathBuf>,
    _input_changed: Subscription,
}

impl DirectorySaver {
    fn new(store: Store, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(
                InputMode::Search.placeholder(),
                InputColors {
                    text: rgb(TEXT).into(),
                    placeholder: rgb(SUBTLE).into(),
                    cursor: rgb(ACCENT).into(),
                    selection: rgba(0x5E6AD266).into(),
                },
                cx,
            )
        });
        let _input_changed = cx.subscribe(&input, |this, _, _: &text_input::Changed, cx| {
            this.notice = None;
            match this.mode {
                InputMode::Search => this.refresh(Keep::First, cx),
                InputMode::AddPath => cx.notify(),
            }
        });
        let mut this = Self {
            store,
            input,
            mode: InputMode::Search,
            results: Vec::new(),
            selected: None,
            notice: None,
            scroll: UniformListScrollHandle::new(),
            home: dirs::home_dir(),
            _input_changed,
        };
        this.refresh(Keep::First, cx);
        this
    }

    fn refresh(&mut self, keep: Keep, cx: &mut Context<Self>) {
        let query = match self.mode {
            InputMode::Search => self.input.read(cx).content().to_owned(),
            InputMode::AddPath => String::new(),
        };
        match self.store.search(&query) {
            Ok(results) => self.results = results,
            Err(err) => {
                self.results.clear();
                self.notice = Some(Notice::Error(format!("Search failed: {err:#}").into()));
            }
        }
        let last = self.results.len().checked_sub(1);
        self.selected = match keep {
            Keep::First => last.map(|_| 0),
            Keep::Id(id) => self
                .results
                .iter()
                .position(|d| d.id == id)
                .or(last.map(|_| 0)),
            Keep::Index => last.map(|last| self.selected.unwrap_or(0).min(last)),
        };
        if let Some(ix) = self.selected {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
        }
        cx.notify();
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.notice = None;
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
        cx.notify();
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == InputMode::AddPath {
            return;
        }
        if let Some(ix) = self.selected {
            self.select(ix.saturating_sub(1), cx);
        }
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == InputMode::AddPath {
            return;
        }
        if let Some(ix) = self.selected {
            self.select((ix + 1).min(self.results.len() - 1), cx);
        }
    }

    /// Clears the input and switches what it is for. Callers refresh the results.
    fn set_mode(&mut self, mode: InputMode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.notice = None;
        self.input.update(cx, |input, cx| {
            input.set_placeholder(mode.placeholder(), cx);
            input.set_content("", cx);
        });
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            InputMode::Search => self.open_selected(cx),
            InputMode::AddPath => self.submit_path(cx),
        }
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        let keep = match self.mode {
            InputMode::Search if self.input.read(cx).content().is_empty() => Keep::Index,
            InputMode::Search => Keep::First,
            InputMode::AddPath => Keep::Index,
        };
        self.set_mode(InputMode::Search, cx);
        self.refresh(keep, cx);
    }

    fn open_selected(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.selected.and_then(|ix| self.results.get(ix)).cloned() else {
            return;
        };
        if !dir.path.is_dir() {
            self.notice = Some(Notice::Error(
                format!("{} no longer exists", self.display_path(&dir.path)).into(),
            ));
            cx.notify();
            return;
        }
        cx.open_with_system(&dir.path);
        self.notice = Some(match self.store.mark_opened(dir.id) {
            Ok(()) => Notice::Info(format!("Opened {}", dir.name).into()),
            Err(err) => Notice::Error(format!("Opened, but could not record it: {err:#}").into()),
        });
        self.refresh(Keep::Id(dir.id), cx);
    }

    fn paste_path(&mut self, _: &PastePath, _: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(InputMode::AddPath, cx);
        let clipboard = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .and_then(|text| paste::parse_pasted_path(&text, self.home.as_deref()))
            .filter(|path| path.is_dir());
        if let Some(path) = clipboard {
            let text = path.to_string_lossy().into_owned();
            self.input.update(cx, |input, cx| input.set_content(text, cx));
        }
        self.refresh(Keep::Index, cx);
    }

    fn check_path(&self, cx: &App) -> PathCheck {
        let raw = self.input.read(cx).content();
        if raw.trim().is_empty() {
            return PathCheck::Empty;
        }
        match paste::parse_pasted_path(raw, self.home.as_deref()) {
            None => PathCheck::Invalid("Not an absolute path"),
            Some(path) if path.is_dir() => PathCheck::Ready(path),
            Some(path) if path.exists() => PathCheck::Invalid("Not a directory"),
            Some(_) => PathCheck::Invalid("Path does not exist"),
        }
    }

    fn submit_path(&mut self, cx: &mut Context<Self>) {
        match self.check_path(cx) {
            PathCheck::Empty => {}
            PathCheck::Invalid(reason) => {
                self.notice = Some(Notice::Error(reason.into()));
                cx.notify();
            }
            PathCheck::Ready(path) => {
                self.set_mode(InputMode::Search, cx);
                self.add_paths(Ok(Some(vec![path])), cx);
            }
        }
    }

    fn open_folder(&mut self, _: &OpenFolder, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == InputMode::AddPath {
            self.set_mode(InputMode::Search, cx);
            self.refresh(Keep::Index, cx);
        }
        self.notice = None;
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = picked.await.map_err(anyhow::Error::from).and_then(|r| r);
            this.update(cx, |this, cx| this.add_paths(picked, cx)).ok();
        })
        .detach();
    }

    fn add_paths(&mut self, picked: Result<Option<Vec<PathBuf>>>, cx: &mut Context<Self>) {
        let paths = match picked {
            Ok(Some(paths)) => paths,
            Ok(None) => return,
            Err(err) => {
                self.notice = Some(Notice::Error(format!("Could not open picker: {err:#}").into()));
                cx.notify();
                return;
            }
        };
        let mut added = Vec::new();
        let mut failures = Vec::new();
        for path in &paths {
            match self.store.add(path) {
                Ok(dir) => added.push(dir),
                Err(err) => failures.push(format!("{err:#}")),
            }
        }
        self.notice = match (added.as_slice(), failures.first()) {
            (_, Some(first)) => Some(Notice::Error(first.clone().into())),
            ([one], None) => Some(Notice::Info(format!("Added {}", one.name).into())),
            (many, None) => Some(Notice::Info(format!("Added {} directories", many.len()).into())),
        };
        self.input.update(cx, |input, cx| input.set_content("", cx));
        let keep = added.last().map_or(Keep::Index, |d| Keep::Id(d.id));
        self.refresh(keep, cx);
    }

    fn remove_selected(&mut self, _: &RemoveSelected, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == InputMode::AddPath {
            return;
        }
        if let Some(ix) = self.selected {
            self.remove_at(ix, cx);
        }
    }

    fn remove_at(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(dir) = self.results.get(ix).cloned() else {
            return;
        };
        self.notice = Some(match self.store.remove(dir.id) {
            Ok(()) => Notice::Info(format!("Removed {}", dir.name).into()),
            Err(err) => Notice::Error(format!("Could not remove: {err:#}").into()),
        });
        self.selected = Some(ix);
        self.refresh(Keep::Index, cx);
    }

    fn display_path(&self, path: &Path) -> String {
        match self.home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => "~".into(),
            Some(rest) => format!("~/{}", rest.display()),
            None => path.display().to_string(),
        }
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let dir = &self.results[ix];
        let missing = !dir.path.is_dir();
        let selected = self.selected == Some(ix);
        let group: SharedString = format!("row-{}", dir.id).into();
        let initial: String = dir.name.chars().next().map_or_else(String::new, |c| c.to_uppercase().collect());
        let meta = match dir.last_opened_at {
            Some(at) => format!("opened {}", relative_time(store::now() - at)),
            None => "never opened".into(),
        };

        div()
            .id(("row", dir.id as u64))
            .group(group.clone())
            .w_full()
            .h(px(ROW_HEIGHT))
            .px(px(8.))
            .flex()
            .items_center()
            .child(
                div()
                    .size_full()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .rounded(px(6.))
                    .relative()
                    .when(selected, |el| el.bg(rgb(SELECTED)))
                    .when(!selected, |el| el.hover(|s| s.bg(rgb(HOVER))))
                    .when(selected, |el| {
                        el.child(
                            div()
                                .absolute()
                                .left_0()
                                .top(px(10.))
                                .bottom(px(10.))
                                .w(px(2.))
                                .rounded(px(1.))
                                .bg(rgb(ACCENT)),
                        )
                    })
                    .child(
                        div()
                            .size(px(24.))
                            .flex_none()
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .when(missing, |el| el.bg(rgb(0x1E1F23)).text_color(rgb(SUBTLE)))
                            .when(!missing, |el| {
                                el.bg(rgba(0x5E6AD240)).text_color(rgb(0xB4BAF5))
                            })
                            .child(initial),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(if missing { MUTED } else { TEXT }))
                                            .child(dir.name.clone()),
                                    )
                                    .when(missing, |el| {
                                        el.child(
                                            div()
                                                .flex_none()
                                                .px(px(5.))
                                                .rounded(px(4.))
                                                .border_1()
                                                .border_color(rgb(BORDER))
                                                .text_size(px(11.))
                                                .line_height(px(16.))
                                                .text_color(rgb(SUBTLE))
                                                .child("missing"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(rgb(if missing { SUBTLE } else { MUTED }))
                                    .child(self.display_path(&dir.path)),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(rgb(SUBTLE))
                            .group_hover(group.clone(), |s| s.invisible())
                            .child(meta),
                    )
                    .child(
                        div()
                            .id(("remove", dir.id as u64))
                            .absolute()
                            .right(px(8.))
                            .px(px(8.))
                            .h(px(24.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .invisible()
                            .group_hover(group, |s| s.visible())
                            .hover(|s| s.bg(rgb(0x26272C)).text_color(rgb(DANGER)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.remove_at(ix, cx);
                            }))
                            .child("Remove"),
                    ),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                this.select(ix, cx);
                if event.click_count() >= 2 {
                    this.open_selected(cx);
                }
            }))
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(48.))
            .px(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().size(px(14.)).rounded(px(4.)).bg(rgb(ACCENT)))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child("Directory Saver"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        button("paste-path")
                            .bg(rgb(SURFACE))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .hover(|s| s.bg(rgb(HOVER)))
                            .text_color(rgb(TEXT))
                            .child("Paste path")
                            .child(kbd("Ctrl L", rgb(0x1E1F23), rgb(MUTED)))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.paste_path(&PastePath, window, cx)
                            })),
                    )
                    .child(
                        button("open-folder")
                            .bg(rgb(ACCENT))
                            .hover(|s| s.bg(rgb(0x6C78E0)))
                            .text_color(rgb(0xFFFFFF))
                            .child("Open folder")
                            .child(kbd("Ctrl N", rgba(0xFFFFFF26), rgba(0xFFFFFFCC)))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.open_folder(&OpenFolder, window, cx)
                            })),
                    ),
            )
    }

    fn render_search(&self, cx: &App) -> impl IntoElement {
        let show_esc = self.mode == InputMode::AddPath || !self.input.read(cx).content().is_empty();
        div().px(px(16.)).pb(px(12.)).child(
            div()
                .h(px(36.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(6.))
                .bg(rgb(SURFACE))
                .border_1()
                .border_color(rgb(BORDER))
                .text_size(px(13.))
                .line_height(px(20.))
                .child(match self.mode {
                    InputMode::Search => search_glyph().into_any_element(),
                    InputMode::AddPath => add_path_chip().into_any_element(),
                })
                .child(self.input.clone())
                .when(show_esc, |el| {
                    el.child(kbd("Esc", rgb(0x1E1F23), rgb(SUBTLE)))
                }),
        )
    }
}

fn button(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(28.))
        .pl(px(10.))
        .pr(px(4.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(6.))
        .cursor_pointer()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
}

fn add_path_chip() -> impl IntoElement {
    div()
        .flex_none()
        .h(px(20.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(rgba(0x5E6AD240))
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(0xB4BAF5))
        .child("Add path")
}

fn kbd(label: &'static str, bg: impl Into<Hsla>, fg: impl Into<Hsla>) -> impl IntoElement {
    div()
        .h(px(20.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(bg.into())
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg.into())
        .child(label)
}

fn search_glyph() -> impl IntoElement {
    div()
        .relative()
        .size(px(14.))
        .flex_none()
        .child(
            div()
                .absolute()
                .top(px(1.))
                .left(px(1.))
                .size(px(9.))
                .rounded_full()
                .border(px(1.5))
                .border_color(rgb(SUBTLE)),
        )
        // gpui divs cannot rotate, so the handle is a short staircase of pixels.
        .children((0..3).map(|i| {
            div()
                .absolute()
                .top(px(9. + i as f32 * 1.5))
                .left(px(9. + i as f32 * 1.5))
                .size(px(2.))
                .bg(rgb(SUBTLE))
        }))
}

fn relative_time(seconds: i64) -> String {
    const UNITS: [(i64, &str); 6] = [
        (365 * 86400, "y"),
        (30 * 86400, "mo"),
        (7 * 86400, "w"),
        (86400, "d"),
        (3600, "h"),
        (60, "m"),
    ];
    UNITS
        .iter()
        .find(|(unit, _)| seconds >= *unit)
        .map_or_else(|| "just now".into(), |(unit, suffix)| format!("{}{suffix} ago", seconds / unit))
}

impl Focusable for DirectorySaver {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for DirectorySaver {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query_empty =
            self.mode == InputMode::AddPath || self.input.read(cx).content().is_empty();
        let body = if !self.results.is_empty() {
            uniform_list(
                "results",
                self.results.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range.map(|ix| this.render_row(ix, cx)).collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .py(px(4.))
            .flex_1()
            .into_any_element()
        } else {
            let (title, hint) = if query_empty {
                ("No directories yet", "Press Ctrl N to pick a folder, or Ctrl L to paste a path")
            } else {
                ("No results", "Try a different search, or press Esc to clear it")
            };
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(rgb(MUTED)).child(title))
                .child(div().text_size(px(12.)).text_color(rgb(SUBTLE)).child(hint))
                .into_any_element()
        };

        let (status, status_color): (SharedString, _) = match (&self.notice, self.mode) {
            (Some(Notice::Error(msg)), _) => (msg.clone(), rgb(DANGER)),
            (Some(Notice::Info(msg)), _) => (msg.clone(), rgb(MUTED)),
            (None, InputMode::AddPath) => match self.check_path(cx) {
                PathCheck::Empty => ("Paste or type the full path of a folder".into(), rgb(SUBTLE)),
                PathCheck::Ready(path) => (
                    format!("Press Enter to add {}", self.display_path(&path)).into(),
                    rgb(MUTED),
                ),
                PathCheck::Invalid(reason) => (reason.into(), rgb(SUBTLE)),
            },
            (None, InputMode::Search) if self.results.len() == 1 => ("1 directory".into(), rgb(SUBTLE)),
            (None, InputMode::Search) => {
                (format!("{} directories", self.results.len()).into(), rgb(SUBTLE))
            }
        };
        let hints: &[&str] = match self.mode {
            InputMode::Search => &["↑↓ navigate", "↵ open", "Ctrl ⌫ remove"],
            InputMode::AddPath => &["↵ add", "Esc cancel"],
        };

        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::open_folder))
            .on_action(cx.listener(Self::paste_path))
            .on_action(cx.listener(Self::remove_selected))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .font_family(FONT)
            .text_size(px(13.))
            .text_color(rgb(TEXT))
            .child(self.render_header(cx))
            .child(self.render_search(cx))
            .child(div().h(px(1.)).bg(rgb(BORDER)))
            .child(body)
            .child(
                div()
                    .h(px(32.))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .text_size(px(12.))
                    .child(div().truncate().text_color(status_color).child(status))
                    .child(
                        div()
                            .flex()
                            .gap(px(14.))
                            .text_color(rgb(SUBTLE))
                            .children(hints.iter().copied()),
                    ),
            )
    }
}

fn db_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("DIRECTORY_SAVER_DB") {
        return Ok(path.into());
    }
    Ok(dirs::data_dir()
        .context("no data directory for this user")?
        .join("directory-saver/directories.db"))
}

fn main() {
    let store = match db_path().and_then(|path| Store::open(&path)) {
        Ok(store) => store,
        Err(err) => {
            eprintln!("directory-saver: {err:#}");
            std::process::exit(1);
        }
    };

    Application::new().run(move |cx: &mut App| {
        text_input::bind_keys(cx);
        cx.bind_keys([
            KeyBinding::new("up", SelectPrev, Some(CONTEXT)),
            KeyBinding::new("down", SelectNext, Some(CONTEXT)),
            KeyBinding::new("enter", Confirm, Some(CONTEXT)),
            KeyBinding::new("escape", Cancel, Some(CONTEXT)),
            KeyBinding::new("ctrl-n", OpenFolder, Some(CONTEXT)),
            KeyBinding::new("ctrl-l", PastePath, Some(CONTEXT)),
            KeyBinding::new("ctrl-backspace", RemoveSelected, Some(CONTEXT)),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let bounds = Bounds::centered(None, size(px(760.), px(520.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Directory Saver".into()),
                        ..Default::default()
                    }),
                    app_id: Some("directory-saver".into()),
                    window_background: gpui::WindowBackgroundAppearance::Opaque,
                    ..Default::default()
                },
                |_, cx| cx.new(|cx| DirectorySaver::new(store, cx)),
            )
            .expect("failed to open window");
        window
            .update(cx, |view, window, cx| {
                window.focus(&view.focus_handle(cx));
                cx.activate(true);
            })
            .ok();
    });
}
