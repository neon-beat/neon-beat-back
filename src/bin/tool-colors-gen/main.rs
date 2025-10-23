//! Helper tool to generate colors to be used for backend's teams

#[cfg(feature = "tool-colors-gen")]
mod colors_gen;

fn main() -> anyhow::Result<()> {
    #[cfg(feature = "tool-colors-gen")]
    {
        colors_gen::run()?;
    }
    Ok(())
}
