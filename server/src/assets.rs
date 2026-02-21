use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../static/pwa/"]
pub struct PwaAssets;
