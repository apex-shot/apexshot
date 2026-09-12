use super::selection::*;
use super::*;
use color::*;
use image::RgbaImage;
use state::EditorState;
use types::*;
use window::cursor_name_for_view_point;

#[path = "tests/tools.rs"]
mod tools;

#[path = "tests/cursor.rs"]
mod cursor;

#[path = "tests/colors.rs"]
mod colors;

#[path = "tests/tool_style.rs"]
mod tool_style;

#[path = "tests/history.rs"]
mod history;

#[path = "tests/selection.rs"]
mod selection;

#[path = "tests/text.rs"]
mod text;

#[path = "tests/numbering.rs"]
mod numbering;

#[path = "tests/effects.rs"]
mod effects;

#[path = "tests/export.rs"]
mod export;

#[path = "tests/crop.rs"]
mod crop;

#[path = "tests/drag_draw.rs"]
mod drag_draw;

#[path = "tests/transform.rs"]
mod transform;

#[path = "tests/arrows.rs"]
mod arrows;
