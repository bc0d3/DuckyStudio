use std::io;

fn main() -> io::Result<()> {
    // Solo compilar recursos para Windows
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        winres::WindowsResource::new()
            .set_icon("icon.ico")
            .compile()?;
    }
    Ok(())
}