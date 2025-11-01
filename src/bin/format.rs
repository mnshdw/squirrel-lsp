use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process;

use squirrel_lsp::formatter::{FormatOptions, IndentStyle, format_document};

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mut options = FormatOptions::default();
    let mut in_place = false;
    let mut input_path: Option<PathBuf> = None;
    let mut output_path: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tabs" => options.indent_style = IndentStyle::Tabs,
            "--spaces" => {
                let spaces = args
                    .next()
                    .ok_or_else(|| "--spaces requires a value".to_string())?
                    .parse()
                    .map_err(|_| "--spaces must be > 0".to_string())?;
                options.indent_style = IndentStyle::Spaces(spaces);
            },
            "--no-final-newline" => options.insert_final_newline = false,
            "--keep-trailing-whitespace" => options.trim_trailing_whitespace = false,
            "--max-width" => {
                let width = args
                    .next()
                    .ok_or_else(|| "--max-width requires a value".to_string())?
                    .parse()
                    .map_err(|_| "--max-width must be > 0".to_string())?;
                options.max_width = width;
            },
            "--in-place" => in_place = true,
            "--output" => {
                let path = args
                    .next()
                    .ok_or_else(|| "--output requires a path".to_string())?;
                output_path = Some(PathBuf::from(path));
            },
            other => {
                if input_path.is_some() {
                    return Err(format!("unexpected argument '{other}'").into());
                }
                input_path = Some(PathBuf::from(other));
            },
        }
    }

    if in_place && output_path.is_some() {
        return Err("--in-place and --output cannot be used together".into());
    }

    let mut source = String::new();

    if let Some(path) = input_path.as_deref() {
        source = fs::read_to_string(path)?;
    } else {
        io::stdin().read_to_string(&mut source)?;
    }

    let formatted = format_document(&source, &options)?;

    if in_place {
        let path = input_path.ok_or_else(|| "--in-place requires an input file".to_string())?;
        fs::write(path, formatted)?;
    } else if let Some(path) = output_path {
        fs::write(path, formatted)?;
    } else {
        io::stdout().write_all(formatted.as_bytes())?;
    }

    Ok(())
}
