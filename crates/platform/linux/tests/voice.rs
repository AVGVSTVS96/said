fn main() -> voice_core::Result<()> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or("runtime is required")?;
    let result = voice_platform::linux::control::run_cli(
        std::path::Path::new(&runtime),
        "voice",
        &std::env::args().skip(1).collect::<Vec<_>>(),
    )?;
    println!("{result}");
    Ok(())
}
