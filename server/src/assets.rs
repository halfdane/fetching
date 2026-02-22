use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../frontend/build/"]
pub struct PwaAssets;
