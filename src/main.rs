use anyhow::Result;

fn main() -> Result<()> {
    // Wiring lands with the CLI module; the renderer and shell are built first.
    println!("marquee-markdown {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
