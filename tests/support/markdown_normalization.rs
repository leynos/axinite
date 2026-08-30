//! Helpers that normalize Markdown for conversion integration tests.

#[derive(Clone, Copy)]
struct FenceDelimiter {
    character: char,
    length: usize,
}

impl FenceDelimiter {
    fn from_line(line: &str) -> Option<Self> {
        let character = line.chars().next()?;
        if !matches!(character, '`' | '~') {
            return None;
        }

        let length = line
            .chars()
            .take_while(|&marker| marker == character)
            .count();
        (length >= 3).then_some(Self { character, length })
    }

    fn closes(self, candidate: Self) -> bool {
        self.character == candidate.character && candidate.length >= self.length
    }
}

#[derive(Default)]
struct NormalizationState {
    lines: Vec<String>,
    paragraph: String,
    fence: Option<FenceDelimiter>,
}

impl NormalizationState {
    fn push_line(&mut self, line: &str) {
        let structural_line = line.trim_start();
        if self.push_fence_line(line, structural_line) {
            return;
        }
        if self.push_fenced_line(line) {
            return;
        }
        if structural_line.is_empty() {
            self.push_blank_line();
            return;
        }
        if is_markdown_block_line(structural_line) {
            self.push_markdown_block_line(line);
            return;
        }
        if self.extends_previous_list_item() {
            self.push_list_continuation(structural_line.trim());
            return;
        }
        self.push_paragraph_text(trim_paragraph_line(structural_line));
    }

    fn push_fence_line(&mut self, line: &str, structural_line: &str) -> bool {
        let Some(candidate) = FenceDelimiter::from_line(structural_line) else {
            return false;
        };
        if self.fence.is_some_and(|opener| !opener.closes(candidate)) {
            return false;
        }

        push_paragraph(&mut self.lines, &mut self.paragraph);
        self.lines.push(line.to_string());
        self.fence = self.fence.map_or(Some(candidate), |_| None);
        true
    }

    fn push_fenced_line(&mut self, line: &str) -> bool {
        if self.fence.is_some() {
            self.lines.push(line.to_string());
            true
        } else {
            false
        }
    }

    fn extends_previous_list_item(&self) -> bool {
        self.paragraph.is_empty()
            && self
                .lines
                .last()
                .is_some_and(|previous| is_markdown_list_item(previous.trim_start()))
    }

    fn push_blank_line(&mut self) {
        push_paragraph(&mut self.lines, &mut self.paragraph);
        if self
            .lines
            .last()
            .is_some_and(|previous| !previous.is_empty())
        {
            self.lines.push(String::new());
        }
    }

    fn push_markdown_block_line(&mut self, line: &str) {
        push_paragraph(&mut self.lines, &mut self.paragraph);
        self.lines.push(line.to_string());
    }

    fn push_list_continuation(&mut self, line: &str) {
        if let Some(previous) = self.lines.last_mut() {
            previous.push(' ');
            previous.push_str(line);
        }
    }

    fn push_paragraph_text(&mut self, line: &str) {
        if needs_soft_wrap_separator(&self.paragraph, line) {
            self.paragraph.push(' ');
        }
        self.paragraph.push_str(line);
        if is_markdown_hard_break(line) {
            push_paragraph(&mut self.lines, &mut self.paragraph);
        }
    }
}

pub(crate) fn normalize(s: &str) -> String {
    let s = s.replace("\r\n", "\n");
    let s = s.trim_matches('\n');

    let mut state = NormalizationState::default();
    for line in s.lines() {
        state.push_line(line);
    }
    push_paragraph(&mut state.lines, &mut state.paragraph);
    while state.lines.last().is_some_and(String::is_empty) {
        state.lines.pop();
    }

    state.lines.join("\n")
}

fn needs_soft_wrap_separator(paragraph: &str, line: &str) -> bool {
    !paragraph.is_empty() && !(paragraph.ends_with('"') && line.starts_with('['))
}

fn trim_paragraph_line(line: &str) -> &str {
    if is_markdown_hard_break(line) {
        line
    } else {
        line.trim_end()
    }
}

fn is_markdown_hard_break(line: &str) -> bool {
    line.ends_with("  ")
        || line
            .as_bytes()
            .iter()
            .rev()
            .take_while(|&&character| character == b'\\')
            .count()
            % 2
            == 1
}

fn push_paragraph(lines: &mut Vec<String>, paragraph: &mut String) {
    if !paragraph.is_empty() {
        lines.push(std::mem::take(paragraph));
    }
}

fn is_markdown_block_line(line: &str) -> bool {
    matches!(line.chars().next(), Some('#' | '>' | '|'))
        || is_markdown_list_item(line)
        || ["---", "***"].iter().any(|prefix| line.starts_with(prefix))
}

fn is_markdown_list_item(line: &str) -> bool {
    ["- ", "* ", "+ "]
        .iter()
        .any(|prefix| line.starts_with(prefix))
        || line.split_once(". ").is_some_and(|(prefix, _)| {
            !prefix.is_empty() && prefix.chars().all(|character| character.is_ascii_digit())
        })
}
