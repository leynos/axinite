//! Interactive prompt utilities for the setup wizard.
//!
//! Provides terminal UI components for:
//! - Single selection menus
//! - Multi-select with toggles
//! - Password/secret input (hidden)
//! - Yes/no confirmations
//! - Styled headers and step indicators

use std::io::{self, Write};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, ClearType},
};
use secrecy::SecretString;

mod render;
mod selection;

pub use selection::{select_many, select_one};

/// Password/secret input with hidden characters.
///
/// # Example
///
/// ```ignore
/// let token = secret_input("Bot token")?;
/// ```
pub fn secret_input(prompt: &str) -> io::Result<SecretString> {
    let mut stdout = io::stdout();

    print!("{}: ", prompt);
    stdout.flush()?;

    terminal::enable_raw_mode()?;
    let result = read_secret_line();
    terminal::disable_raw_mode()?;

    writeln!(stdout)?;
    result
}

fn read_secret_line() -> io::Result<SecretString> {
    let mut input = String::new();
    let mut stdout = io::stdout();

    // Drain any residual key events (e.g. Enter from a prior `read_line` prompt)
    // that are already queued before we start reading. Without this, on
    // Windows the leftover Enter is immediately consumed and the function
    // returns an empty string before the user can type anything.
    // Uses Duration::ZERO so we never block waiting for new input — only
    // events already in the queue are consumed.
    while event::poll(std::time::Duration::ZERO)? {
        let _ = event::read()?;
    }

    loop {
        if let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = event::read()?
        {
            let (next_input, effect) = apply_secret_key_event(&input, code, modifiers);
            match effect {
                SecretInputEffect::Submit => {
                    input = next_input;
                    break;
                }
                SecretInputEffect::Interrupt => {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "Ctrl-C"));
                }
                SecretInputEffect::None => {}
                SecretInputEffect::Backspace | SecretInputEffect::MaskChar => {
                    input = next_input;
                    apply_secret_input_effect(&mut stdout, &effect)?;
                }
            }
        }
    }

    Ok(SecretString::from(input))
}

/// Yes/no confirmation prompt.
///
/// # Example
///
/// ```ignore
/// if confirm("Enable Telegram channel?", false)? {
///     // ...
/// }
/// ```
pub fn confirm(prompt: &str, default: bool) -> io::Result<bool> {
    let mut stdout = io::stdout();

    let hint = if default { "[Y/n]" } else { "[y/N]" };
    print!("{} {} ", prompt, hint);
    stdout.flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    Ok(match input.as_str() {
        "" => default,
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default,
    })
}

pub fn print_header(text: &str) {
    render::print_header(text);
}

pub fn print_step(current: usize, total: usize, name: &str) {
    render::print_step(current, total, name);
}

/// Print a success message with green checkmark.
pub fn print_success(message: &str) {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, SetForegroundColor(Color::Green));
    print!("✓");
    let _ = execute!(stdout, ResetColor);
    println!(" {}", message);
}

/// Print an error message with red X.
pub fn print_error(message: &str) {
    let mut stderr = io::stderr();
    let _ = execute!(stderr, SetForegroundColor(Color::Red));
    eprint!("✗");
    let _ = execute!(stderr, ResetColor);
    eprintln!(" {}", message);
}

/// Print a warning message with yellow exclamation.
pub fn print_warning(message: &str) {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
    print!("!");
    let _ = execute!(stdout, ResetColor);
    println!(" {}", message);
}

/// Print an info message with blue info icon.
pub fn print_info(message: &str) {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, SetForegroundColor(Color::Blue));
    print!("ℹ");
    let _ = execute!(stdout, ResetColor);
    println!(" {}", message);
}

/// Read a simple line of input with a prompt.
pub fn input(prompt: &str) -> io::Result<String> {
    let mut stdout = io::stdout();
    print!("{}: ", prompt);
    stdout.flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

/// Read an optional line of input (empty returns None).
pub fn optional_input(prompt: &str, hint: Option<&str>) -> io::Result<Option<String>> {
    let mut stdout = io::stdout();

    if let Some(h) = hint {
        print!("{} ({}): ", prompt, h);
    } else {
        print!("{}: ", prompt);
    }
    stdout.flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        Ok(None)
    } else {
        Ok(Some(input.to_string()))
    }
}

#[cfg(test)]
mod tests;

#[derive(Debug, PartialEq, Eq)]
enum SecretInputEffect {
    None,
    Backspace,
    MaskChar,
    Submit,
    Interrupt,
}

fn apply_secret_input_effect<W: Write>(
    stdout: &mut W,
    effect: &SecretInputEffect,
) -> io::Result<()> {
    match effect {
        SecretInputEffect::Backspace => print_and_flush(stdout, "\x08 \x08"),
        SecretInputEffect::MaskChar => print_and_flush(stdout, "*"),
        SecretInputEffect::None | SecretInputEffect::Submit | SecretInputEffect::Interrupt => {
            Ok(())
        }
    }
}

/// Print `text` and flush immediately so masked input feedback is visible.
fn print_and_flush<W: Write>(stdout: &mut W, text: &str) -> io::Result<()> {
    execute!(stdout, Print(text))?;
    stdout.flush()
}

fn apply_secret_key_event(
    input: &str,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> (String, SecretInputEffect) {
    match code {
        KeyCode::Enter => (input.to_string(), SecretInputEffect::Submit),
        KeyCode::Backspace if !input.is_empty() => {
            let mut next_input = input.to_string();
            next_input.pop();
            (next_input, SecretInputEffect::Backspace)
        }
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            (input.to_string(), SecretInputEffect::Interrupt)
        }
        KeyCode::Char(c) => (format!("{input}{c}"), SecretInputEffect::MaskChar),
        _ => (input.to_string(), SecretInputEffect::None),
    }
}
