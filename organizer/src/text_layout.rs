//! Rebuilds the layout of recognised text so copied text pastes properly.
//!
//! OCR reports words and their boxes; pasting the plain lines loses table
//! columns, code indentation, list continuations, and paragraph breaks. This
//! module groups words into rows, classifies the page, and renders plain text
//! (tabs for tables) plus an HTML table when the clipboard target accepts one.
//! It is pure: no I/O and no platform calls.

use std::cmp::Ordering;

use crate::{OcrPage, TextLine, TextWord};

/// How the page's text is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    Prose,
    List,
    Code,
    Table,
}

/// The rebuilt page layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutText {
    pub kind: TextKind,
    /// Plain text to paste: tab-separated for tables.
    pub text: String,
    /// An HTML table for apps that paste cells; only for tables.
    pub html: Option<String>,
}

/// Rebuild the page's layout. Lines are taken top to bottom (sort by y, then x).
pub fn rebuild(page: &OcrPage) -> LayoutText {
    let empty = || LayoutText {
        kind: TextKind::Prose,
        text: String::new(),
        html: None,
    };
    if page.lines.is_empty() || page.lines.iter().all(is_blank) {
        return empty();
    }
    let line_height = line_height(&page.lines);
    let rows = rows_of(&page.lines, line_height);
    if rows.is_empty() {
        return empty();
    }
    let char_width = char_width(&rows);
    let cells: Vec<Vec<Cell>> = rows
        .iter()
        .map(|row| cells_of(&row.words, char_width))
        .collect();
    if let Some(columns) = table_columns(&cells, char_width) {
        return table_layout(&cells, &columns);
    }
    if is_code(&rows) {
        return LayoutText {
            kind: TextKind::Code,
            text: code_text(&rows, char_width),
            html: None,
        };
    }
    if is_list(&rows) {
        return LayoutText {
            kind: TextKind::List,
            text: list_text(&rows, char_width),
            html: None,
        };
    }
    LayoutText {
        kind: TextKind::Prose,
        text: prose_text(&rows, char_width, line_height),
        html: None,
    }
}

/// One physical row: the words of every line whose centres line up.
struct Row {
    words: Vec<TextWord>,
    top: i32,
    bottom: i32,
    left: i32,
    centre: f32,
    members: usize,
}

impl Row {
    fn text(&self) -> String {
        words_text(&self.words)
    }
}

/// One column of a row, split where the horizontal gap is wide.
struct Cell {
    text: String,
    left: i32,
}

/// A kept table column.
struct Column {
    centre: f32,
    left: i32,
}

fn is_blank(line: &TextLine) -> bool {
    line.text.trim().is_empty() && line.words.is_empty()
}

fn words_text(words: &[TextWord]) -> String {
    words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A line's words, or one pseudo-word per token spread across its box.
fn line_words(line: &TextLine) -> Vec<TextWord> {
    if !line.words.is_empty() {
        return line.words.clone();
    }
    let tokens: Vec<&str> = line.text.split_whitespace().collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    let count = tokens.len() as i32;
    let step = line.width / count;
    tokens
        .iter()
        .enumerate()
        .map(|(index, token)| {
            TextWord::new(
                *token,
                line.x + index as i32 * step,
                line.y,
                step,
                line.height,
            )
        })
        .collect()
}

fn char_width(rows: &[Row]) -> f32 {
    let mut width = 0.0f32;
    let mut chars = 0.0f32;
    for word in rows.iter().flat_map(|row| row.words.iter()) {
        width += word.width as f32;
        chars += word.text.chars().count() as f32;
    }
    if chars <= 0.0 {
        return 1.0;
    }
    (width / chars).max(1.0)
}

fn line_height(lines: &[TextLine]) -> f32 {
    let mut heights: Vec<i32> = lines.iter().map(|line| line.height.max(1)).collect();
    if heights.is_empty() {
        return 1.0;
    }
    heights.sort_unstable();
    let middle = heights.len() / 2;
    let median = if heights.len().is_multiple_of(2) {
        (heights[middle - 1] + heights[middle]) as f32 / 2.0
    } else {
        heights[middle] as f32
    };
    median.max(1.0)
}

fn rows_of(lines: &[TextLine], line_height: f32) -> Vec<Row> {
    let mut ordered: Vec<&TextLine> = lines.iter().collect();
    ordered.sort_by_key(|line| (line.y, line.x));
    let half = line_height / 2.0;
    let mut rows: Vec<Row> = Vec::new();
    for line in ordered {
        let words = line_words(line);
        if words.is_empty() {
            continue;
        }
        let centre = line.y as f32 + line.height as f32 / 2.0;
        let left = words.iter().map(|word| word.x).min().unwrap_or(line.x);
        let top = line.y;
        let bottom = line.y + line.height;
        if let Some(row) = rows.last_mut() {
            if (row.centre - centre).abs() < half {
                let previous = row.centre * row.members as f32;
                row.centre = (previous + centre) / (row.members + 1) as f32;
                row.members += 1;
                row.words.extend(words);
                row.top = row.top.min(top);
                row.bottom = row.bottom.max(bottom);
                row.left = row.left.min(left);
                continue;
            }
        }
        rows.push(Row {
            words,
            top,
            bottom,
            left,
            centre,
            members: 1,
        });
    }
    for row in &mut rows {
        row.words.sort_by_key(|word| word.x);
    }
    rows
}

fn cells_of(words: &[TextWord], char_width: f32) -> Vec<Cell> {
    let mut cells: Vec<Cell> = Vec::new();
    let mut current: Vec<&TextWord> = Vec::new();
    for word in words {
        if let Some(previous) = current.last() {
            let gap = word.x - (previous.x + previous.width);
            if gap as f32 > 2.5 * char_width {
                cells.push(cell_of(&current));
                current.clear();
            }
        }
        current.push(word);
    }
    if !current.is_empty() {
        cells.push(cell_of(&current));
    }
    cells
}

fn cell_of(words: &[&TextWord]) -> Cell {
    Cell {
        text: words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        left: words.first().map(|word| word.x).unwrap_or(0),
    }
}

/// The kept table columns, or None when the page is not a table.
fn table_columns(cells: &[Vec<Cell>], char_width: f32) -> Option<Vec<Column>> {
    if cells.len() < 3 {
        return None;
    }
    let wide_rows = cells.iter().filter(|row| row.len() >= 3).count();
    if wide_rows < 2 {
        return None;
    }
    let mut edges: Vec<(f32, usize)> = Vec::new();
    for (row_index, row_cells) in cells.iter().enumerate() {
        if row_cells.len() < 2 {
            continue;
        }
        for cell in row_cells {
            edges.push((cell.left as f32, row_index));
        }
    }
    if edges.is_empty() {
        return None;
    }
    edges.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
    let tolerance = 1.5 * char_width;
    let mut clusters: Vec<Cluster> = Vec::new();
    for (edge, row_index) in edges {
        let mut joined = false;
        if let Some(cluster) = clusters.last_mut() {
            if edge - cluster.max <= tolerance {
                cluster.max = edge;
                cluster.sum += edge;
                cluster.count += 1;
                if !cluster.rows.contains(&row_index) {
                    cluster.rows.push(row_index);
                }
                joined = true;
            }
        }
        if !joined {
            clusters.push(Cluster {
                min: edge,
                max: edge,
                sum: edge,
                count: 1,
                rows: vec![row_index],
            });
        }
    }
    let kept = clusters.iter().any(|cluster| cluster.rows.len() >= 2);
    if !kept {
        return None;
    }
    let mut total = 0usize;
    let mut aligned = 0usize;
    for row_cells in cells {
        if row_cells.len() < 2 {
            continue;
        }
        for cell in row_cells {
            total += 1;
            let edge = cell.left as f32;
            let in_kept = clusters.iter().any(|cluster| {
                cluster.rows.len() >= 2 && edge >= cluster.min && edge <= cluster.max
            });
            if in_kept {
                aligned += 1;
            }
        }
    }
    if total == 0 || (aligned as f32) < 0.6 * total as f32 {
        return None;
    }
    let mut columns: Vec<Column> = clusters
        .iter()
        .filter(|cluster| cluster.rows.len() >= 2)
        .map(|cluster| Column {
            centre: cluster.sum / cluster.count as f32,
            left: cluster.min as i32,
        })
        .collect();
    columns.sort_by_key(|column| column.left);
    Some(columns)
}

/// A cluster of cell left edges.
struct Cluster {
    min: f32,
    max: f32,
    sum: f32,
    count: usize,
    rows: Vec<usize>,
}

fn nearest_column(columns: &[Column], left: f32) -> usize {
    let mut best = 0;
    let mut best_distance = f32::MAX;
    for (index, column) in columns.iter().enumerate() {
        let distance = (column.centre - left).abs();
        if distance < best_distance {
            best = index;
            best_distance = distance;
        }
    }
    best
}

fn table_layout(cells: &[Vec<Cell>], columns: &[Column]) -> LayoutText {
    let mut lines: Vec<String> = Vec::new();
    let mut html = String::from("<table>");
    for row_cells in cells {
        let mut fields = vec![String::new(); columns.len()];
        for cell in row_cells {
            let index = nearest_column(columns, cell.left as f32);
            let field = &mut fields[index];
            if !field.is_empty() {
                field.push(' ');
            }
            field.push_str(&cell.text);
        }
        lines.push(fields.join("\t").trim_end().to_string());
        html.push_str("<tr>");
        for field in &fields {
            html.push_str("<td>");
            html.push_str(&escape_html(field));
            html.push_str("</td>");
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
    LayoutText {
        kind: TextKind::Table,
        text: lines.join("\n"),
        html: Some(html),
    }
}

fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn is_code(rows: &[Row]) -> bool {
    if rows.len() < 3 {
        return false;
    }
    let matched = rows
        .iter()
        .filter(|row| {
            let text = row.text();
            has_code_marker(&text) || has_yaml_key(&text)
        })
        .count();
    matched * 10 >= rows.len() * 3
}

fn has_code_marker(text: &str) -> bool {
    const TOKENS: &[&str] = &["{", "}", "=>", "->", "::", "==", "!=", "()"];
    const STARTS: &[&str] = &[
        "def ",
        "fn ",
        "let ",
        "const ",
        "import ",
        "from ",
        "#include",
        "class ",
        "return ",
        "if (",
        "for (",
        "function ",
        "public ",
        "private ",
        "pub ",
        "use ",
    ];
    let trimmed = text.trim();
    if trimmed.ends_with(';') {
        return true;
    }
    if TOKENS.iter().any(|token| text.contains(token)) {
        return true;
    }
    STARTS.iter().any(|start| trimmed.starts_with(start))
}

fn has_yaml_key(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        if *ch != ':' {
            continue;
        }
        let mut start = index;
        while start > 0 && is_key_char(chars[start - 1]) {
            start -= 1;
        }
        if start == index {
            continue;
        }
        let after = chars.get(index + 1).copied();
        if after.is_none() || after == Some(' ') {
            return true;
        }
    }
    false
}

fn is_key_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

fn is_list(rows: &[Row]) -> bool {
    let marked = rows
        .iter()
        .filter(|row| starts_list_item(&row.text()))
        .count();
    marked >= 2
}

fn starts_list_item(text: &str) -> bool {
    let trimmed = text.trim_start();
    let mut chars = trimmed.chars();
    if let Some(first) = chars.next() {
        if matches!(first, '•' | '·' | '–' | '▪') {
            return true;
        }
        if first == '-' || first == '*' {
            return chars.next().map(char::is_whitespace).unwrap_or(true);
        }
    }
    number_marker_len(trimmed).is_some()
}

fn number_marker_len(text: &str) -> Option<usize> {
    let digits = text.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let marker = text.chars().nth(digits)?;
    if marker != '.' && marker != ')' {
        return None;
    }
    if text.chars().nth(digits + 1)?.is_whitespace() {
        Some(digits + 1)
    } else {
        None
    }
}

fn code_text(rows: &[Row], char_width: f32) -> String {
    let left = rows.iter().map(|row| row.left).min().unwrap_or(0) as f32;
    let raw: Vec<f32> = rows
        .iter()
        .map(|row| ((row.left as f32 - left) / char_width).max(0.0))
        .collect();
    // Measured indents drift by a space either way; code indents in steps,
    // so snap to the file's step (2 or 4) taken from its smallest indent.
    let smallest = raw
        .iter()
        .copied()
        .filter(|indent| *indent >= 1.0)
        .fold(f32::MAX, f32::min);
    let step = if smallest < 3.0 { 2.0 } else { 4.0 };
    let mut lines: Vec<String> = Vec::new();
    for (row, indent) in rows.iter().zip(raw) {
        let indent = ((indent / step).round() * step) as usize;
        let mut line = " ".repeat(indent);
        line.push_str(&row.text());
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

fn list_text(rows: &[Row], char_width: f32) -> String {
    let mut items: Vec<String> = Vec::new();
    let mut item_left = 0.0f32;
    for row in rows {
        let text = row.text().trim().to_string();
        let starts = starts_list_item(&text);
        let continuation =
            !starts && !items.is_empty() && row.left as f32 > item_left + 1.5 * char_width;
        if continuation {
            if let Some(item) = items.last_mut() {
                item.push(' ');
                item.push_str(&text);
            }
        } else {
            items.push(text);
            item_left = row.left as f32;
        }
    }
    items
        .iter()
        .map(|item| item.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

fn prose_text(rows: &[Row], char_width: f32, line_height: f32) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut first_left = 0.0f32;
    let mut previous_bottom = 0i32;
    for (index, row) in rows.iter().enumerate() {
        let text = row.text().trim().to_string();
        let breaks = index == 0
            || (row.top - previous_bottom) as f32 > 0.8 * line_height
            || row.left as f32 - first_left > 2.0 * char_width;
        if breaks {
            paragraphs.push(text);
            first_left = row.left as f32;
        } else if let Some(paragraph) = paragraphs.last_mut() {
            let continues_word = text
                .chars()
                .next()
                .map(|ch| ch.is_lowercase())
                .unwrap_or(false);
            if paragraph.ends_with('-') && continues_word {
                paragraph.pop();
                paragraph.push_str(&text);
            } else {
                paragraph.push(' ');
                paragraph.push_str(&text);
            }
        }
        previous_bottom = row.bottom;
    }
    paragraphs
        .iter()
        .map(|paragraph| paragraph.trim_end())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(words: &[(&str, i32)], y: i32) -> TextLine {
        let words: Vec<TextWord> = words
            .iter()
            .map(|(text, x)| TextWord::new(*text, *x, y, 10 * text.chars().count() as i32, 20))
            .collect();
        let text = words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let left = words.iter().map(|word| word.x).min().unwrap_or(0);
        let right = words
            .iter()
            .map(|word| word.x + word.width)
            .max()
            .unwrap_or(0);
        TextLine::new(text, left, y, right - left, 20).with_words(words)
    }

    fn page(lines: Vec<TextLine>) -> OcrPage {
        OcrPage {
            image_width: 800,
            image_height: 600,
            lines,
        }
    }

    #[test]
    fn rebuilds_a_table() {
        let page = page(vec![
            line(&[("Name", 0), ("Qty", 200), ("Price", 400)], 0),
            line(&[("Apples", 0), ("3", 200), ("1.20", 400)], 20),
            line(&[("A&B", 0), ("12", 200), ("3.50", 400)], 40),
            line(&[("Pears", 0), ("7", 200), ("0.80", 400)], 60),
        ]);
        let layout = rebuild(&page);
        assert_eq!(layout.kind, TextKind::Table);
        assert_eq!(
            layout.text,
            "Name\tQty\tPrice\nApples\t3\t1.20\nA&B\t12\t3.50\nPears\t7\t0.80"
        );
        let html = layout.html.unwrap();
        assert!(html.starts_with("<table><tr><td>Name</td>"));
        assert!(html.contains("&amp;"));
    }

    #[test]
    fn merges_a_table_row_split_over_two_lines() {
        let page = page(vec![
            line(&[("Name", 0), ("Qty", 200), ("Price", 400)], 0),
            line(&[("Apples", 0)], 20),
            line(&[("3", 200), ("1.20", 400)], 20),
            line(&[("Pears", 0), ("7", 200), ("0.80", 400)], 40),
        ]);
        let layout = rebuild(&page);
        assert_eq!(layout.kind, TextKind::Table);
        assert_eq!(
            layout.text,
            "Name\tQty\tPrice\nApples\t3\t1.20\nPears\t7\t0.80"
        );
    }

    #[test]
    fn rebuilds_python_indentation() {
        let page = page(vec![
            line(&[("def", 0), ("total(items):", 40)], 0),
            line(&[("sum", 40), ("=", 70), ("0", 90)], 20),
            line(
                &[("for", 40), ("item", 70), ("in", 100), ("items:", 130)],
                40,
            ),
            line(&[("sum", 80), ("+=", 110), ("item", 130)], 60),
        ]);
        let layout = rebuild(&page);
        assert_eq!(layout.kind, TextKind::Code);
        assert_eq!(
            layout.text,
            "def total(items):\n    sum = 0\n    for item in items:\n        sum += item"
        );
    }

    #[test]
    fn rebuilds_yaml_indentation() {
        let page = page(vec![
            line(&[("name:", 0), ("apexshot", 60)], 0),
            line(&[("version:", 0), ("1", 90)], 20),
            line(&[("deps:", 0)], 40),
            line(&[("serde:", 20), ("1", 80)], 60),
        ]);
        let layout = rebuild(&page);
        assert_eq!(layout.kind, TextKind::Code);
        assert_eq!(layout.text, "name: apexshot\nversion: 1\ndeps:\n  serde: 1");
    }

    #[test]
    fn joins_hyphenated_prose_and_keeps_paragraphs() {
        let page = page(vec![
            line(&[("recog-", 0)], 0),
            line(&[("nition", 0), ("works", 70)], 20),
            line(&[("Second", 0), ("paragraph", 70)], 60),
        ]);
        let layout = rebuild(&page);
        assert_eq!(layout.kind, TextKind::Prose);
        assert_eq!(layout.text, "recognition works\n\nSecond paragraph");
    }

    #[test]
    fn rebuilds_a_numbered_list_with_continuations() {
        let page = page(vec![
            line(&[("1.", 0), ("Open", 30), ("Settings", 70)], 0),
            line(&[("2.", 0), ("Choose", 30), ("Studio", 80)], 20),
            line(&[("and", 30), ("click", 70), ("Download", 110)], 40),
        ]);
        let layout = rebuild(&page);
        assert_eq!(layout.kind, TextKind::List);
        assert_eq!(
            layout.text,
            "1. Open Settings\n2. Choose Studio and click Download"
        );
    }

    #[test]
    fn rebuilds_lines_without_word_boxes() {
        let page = page(vec![
            TextLine::new("first line here", 0, 0, 100, 20),
            TextLine::new("second line here", 0, 20, 100, 20),
        ]);
        let layout = rebuild(&page);
        assert_eq!(layout.kind, TextKind::Prose);
        assert_eq!(layout.text, "first line here second line here");
    }

    #[test]
    fn empty_page_rebuilds_to_nothing() {
        let layout = rebuild(&page(Vec::new()));
        assert_eq!(layout.kind, TextKind::Prose);
        assert_eq!(layout.text, "");
        assert_eq!(layout.html, None);
    }
}
