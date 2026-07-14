//! Selection menus for the setup wizard: single-choice numbered menus
//! and multi-select toggle lists, with their rendering and key-handling
//! helpers.

use super::*;

/// Display a numbered menu and get user selection.
///
/// Returns the index (0-based) of the selected option.
/// Pressing Enter without input selects the first option (index 0).
///
/// # Example
///
/// ```ignore
/// let choice = select_one("Choose an option:", &["Option A", "Option B"]);
/// ```
pub fn select_one(prompt: &str, options: &[&str]) -> io::Result<usize> {
    let mut stdout = io::stdout();

    // Print prompt
    writeln!(stdout, "{}", prompt)?;
    writeln!(stdout)?;

    // Print options
    for (i, option) in options.iter().enumerate() {
        writeln!(stdout, "  [{}] {}", i + 1, option)?;
    }
    writeln!(stdout)?;

    loop {
        print!("> ");
        stdout.flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        // Handle empty input as first option
        if input.is_empty() {
            return Ok(0);
        }

        // Parse number
        if let Ok(num) = input.parse::<usize>()
            && (1..=options.len()).contains(&num)
        {
            return Ok(num - 1);
        }

        writeln!(
            stdout,
            "Invalid choice. Please enter a number 1-{}.",
            options.len()
        )?;
    }
}

/// Multi-select with space to toggle, enter to confirm.
///
/// `options` is a slice of (label, initially_selected) tuples.
/// Returns indices of selected options.
///
/// # Example
///
/// ```ignore
/// let selected = select_many("Select channels:", &[
///     ("CLI/TUI", true),
///     ("HTTP webhook", false),
///     ("Telegram", false),
/// ])?;
/// ```
pub fn select_many(prompt: &str, options: &[(&str, bool)]) -> io::Result<Vec<usize>> {
    if options.is_empty() {
        return Ok(vec![]);
    }

    let mut stdout = io::stdout();
    let mut state = SelectManyState::new(options);

    terminal::enable_raw_mode()?;
    execute!(stdout, cursor::Hide)?;

    let result = (|| {
        loop {
            // Clear and redraw
            execute!(stdout, cursor::MoveToColumn(0))?;

            writeln!(stdout, "{}\r", prompt)?;
            writeln!(stdout, "\r")?;
            writeln!(
                stdout,
                "  (Use arrow keys to navigate, space to toggle, enter to confirm)\r"
            )?;
            writeln!(stdout, "\r")?;

            render_select_many_options(&mut stdout, options, &state)?;

            stdout.flush()?;

            // Read key
            if let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
            {
                match state.apply_key(code, modifiers) {
                    SelectManyAction::Confirm => break,
                    SelectManyAction::Interrupt => {
                        return Err(io::Error::new(io::ErrorKind::Interrupted, "Ctrl-C"));
                    }
                    SelectManyAction::Continue => {}
                }

                // Move cursor up to redraw
                execute!(
                    stdout,
                    cursor::MoveUp((options.len() + 4) as u16),
                    terminal::Clear(ClearType::FromCursorDown)
                )?;
            }
        }
        Ok(())
    })();

    // Cleanup
    execute!(stdout, cursor::Show)?;
    terminal::disable_raw_mode()?;
    writeln!(stdout)?;

    result?;

    Ok(state.selected_indices())
}

/// Outcome of a single key press in the multi-select prompt.
enum SelectManyAction {
    Continue,
    Confirm,
    Interrupt,
}

/// Mutable selection and cursor state for the multi-select prompt.
struct SelectManyState {
    selected: Vec<bool>,
    cursor_pos: usize,
}

impl SelectManyState {
    /// Initialize the state from the options' initial selection flags.
    fn new(options: &[(&str, bool)]) -> Self {
        Self {
            selected: options.iter().map(|(_, s)| *s).collect(),
            cursor_pos: 0,
        }
    }

    /// Apply a key press to the state, reporting how to proceed.
    fn apply_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> SelectManyAction {
        match code {
            KeyCode::Up => {
                self.cursor_pos = self.cursor_pos.saturating_sub(1);
                SelectManyAction::Continue
            }
            KeyCode::Down if self.cursor_pos < self.selected.len() - 1 => {
                self.cursor_pos += 1;
                SelectManyAction::Continue
            }
            KeyCode::Char(' ') => {
                self.selected[self.cursor_pos] = !self.selected[self.cursor_pos];
                SelectManyAction::Continue
            }
            KeyCode::Enter => SelectManyAction::Confirm,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                SelectManyAction::Interrupt
            }
            _ => SelectManyAction::Continue,
        }
    }

    /// Return the indices of the currently selected options.
    fn selected_indices(&self) -> Vec<usize> {
        self.selected
            .iter()
            .enumerate()
            .filter_map(|(i, &s)| if s { Some(i) } else { None })
            .collect()
    }
}

/// Render the multi-select option list, highlighting the cursor row.
fn render_select_many_options<W: Write>(
    stdout: &mut W,
    options: &[(&str, bool)],
    state: &SelectManyState,
) -> io::Result<()> {
    for (i, (label, _)) in options.iter().enumerate() {
        let checkbox = if state.selected[i] { "[x]" } else { "[ ]" };
        let prefix = if i == state.cursor_pos { ">" } else { " " };

        if i == state.cursor_pos {
            execute!(stdout, SetForegroundColor(Color::Cyan))?;
            writeln!(stdout, "  {} {} {}\r", prefix, checkbox, label)?;
            execute!(stdout, ResetColor)?;
        } else {
            writeln!(stdout, "  {} {} {}\r", prefix, checkbox, label)?;
        }
    }
    Ok(())
}
